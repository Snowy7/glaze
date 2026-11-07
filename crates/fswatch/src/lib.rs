//! Native Windows directory notifications with no idle polling.
//!
//! One small thread per watch blocks on overlapped directory I/O and a stop event. Only real
//! changes start a debounce deadline; dropping a watch cancels its I/O and joins the thread.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{
    ERROR_DIRECTORY, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NOTIFY_ENUM_DIR,
    ERROR_PATH_NOT_FOUND, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};
use windows::core::{HSTRING, PCWSTR};

const BUFFER_BYTES: usize = 64 * 1024; // Also works on network directories.
const MAX_PATHS: usize = 8192;
const DEBOUNCE: Duration = Duration::from_millis(60);
const MAX_LATENCY: Duration = Duration::from_millis(250);

/// Paths are absolute and include both old and new names for a rename. `Rescan` means names
/// were lost (overflow, invalid records or I/O recovery); reload the watched state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Paths(Vec<PathBuf>),
    Rescan,
}

/// A live directory watch. Drop it outside its callback to stop and join its worker.
pub struct Watch {
    stop: Arc<OwnedHandle>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// Watch a directory, optionally including descendants. Keep the callback short (post a
    /// message to an existing event loop). Bursts coalesce for 60 ms, capped at 250 ms latency.
    /// The first native read is armed before this returns, so immediate writes are observed.
    pub fn directory(
        dir: &Path,
        recursive: bool,
        changed: impl FnMut(Event) + Send + 'static,
    ) -> Result<Self> {
        Self::start(dir, recursive, Vec::new(), BUFFER_BYTES, DEBOUNCE, changed)
    }

    /// Watch named files in a directory (including atomic replacement/removal). Names use
    /// ASCII case-insensitive matching, as the config/theme callers do. Unrelated changes
    /// don't start a debounce timer. A lost batch always invokes `changed` to reload state.
    pub fn files(
        dir: &Path,
        names: &[&str],
        mut changed: impl FnMut() + Send + 'static,
    ) -> Result<Self> {
        Self::start(
            dir,
            false,
            names.iter().map(OsString::from).collect(),
            BUFFER_BYTES,
            DEBOUNCE,
            move |_| changed(),
        )
    }

    fn start(
        dir: &Path,
        recursive: bool,
        names: Vec<OsString>,
        buffer_bytes: usize,
        debounce: Duration,
        mut changed: impl FnMut(Event) + Send + 'static,
    ) -> Result<Self> {
        let root = std::path::absolute(dir)?;
        // SAFETY: a new stop event, owned until the worker has joined.
        let stop = Arc::new(unsafe { owned(CreateEventW(None, true, false, PCWSTR::null())?) });
        let worker_stop = stop.clone();
        let (ready, armed) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name(format!(
                "fswatch-{}",
                dir.file_name().unwrap_or_default().to_string_lossy()
            ))
            .stack_size(256 * 1024)
            .spawn(move || {
                let mut read = match Read::open(&root, buffer_bytes).and_then(|mut read| {
                    read.arm(recursive)?;
                    Ok(read)
                }) {
                    Ok(read) => read,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                loop {
                    if let Err(error) = run(
                        &mut read,
                        raw(&worker_stop),
                        &root,
                        recursive,
                        &names,
                        debounce,
                        &mut changed,
                    ) {
                        log::warn!("watch {} recovering: {error:#}", root.display());
                    } else {
                        return; // Stop was signalled.
                    }
                    // Close the old directory before waiting: its delete-pending handle can
                    // otherwise prevent a replacement at the same path.
                    drop(read);
                    changed(Event::Rescan);
                    let Some(recovered) =
                        recover(&root, raw(&worker_stop), recursive, buffer_bytes)
                    else {
                        return;
                    };
                    read = recovered;
                    changed(Event::Rescan); // Includes publications made before re-arming.
                }
            })
            .context("start directory watcher")?;
        let watch = Self {
            stop,
            thread: Some(thread),
        };
        armed.recv().context("directory watcher startup")??;
        Ok(watch)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the event stays alive until after the worker has returned.
        let _ = unsafe { SetEvent(raw(&self.stop)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// Transfers a newly created, valid Win32 handle into its standard Send + Sync owner.
unsafe fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: caller transfers sole ownership of a successful handle creation.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

/// Owns the pending I/O's storage. Every exit path cancels and waits before freeing it.
struct Read {
    folder: OwnedHandle,
    done: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    buffer: Vec<u32>,
    active: bool,
}

impl Read {
    fn open(root: &Path, buffer_bytes: usize) -> Result<Self> {
        let path = HSTRING::from(root.as_os_str());
        // SAFETY: each successfully created handle is immediately owned; pending I/O is
        // cancelled and drained by Read::drop before its handles or storage are freed.
        unsafe {
            let folder = owned(
                CreateFileW(
                    &path,
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    None,
                )
                .with_context(|| format!("watch {}", root.display()))?,
            );
            let done = owned(CreateEventW(None, true, false, PCWSTR::null())?);
            Ok(Self::new(folder, done, buffer_bytes))
        }
    }

    fn new(folder: OwnedHandle, done: OwnedHandle, bytes: usize) -> Self {
        let overlapped = Box::new(OVERLAPPED {
            hEvent: raw(&done),
            ..Default::default()
        });
        Self {
            folder,
            done,
            overlapped,
            buffer: vec![0; bytes / 4],
            active: false,
        }
    }

    fn arm(&mut self, recursive: bool) -> Result<()> {
        debug_assert!(!self.active);
        // SAFETY: buffer and boxed OVERLAPPED stay at fixed addresses until completion or
        // cancellation; Drop waits for that completion before releasing either allocation.
        unsafe {
            ResetEvent(raw(&self.done))?;
            let result = ReadDirectoryChangesW(
                raw(&self.folder),
                self.buffer.as_mut_ptr().cast(),
                (self.buffer.len() * 4) as u32,
                recursive,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_CREATION
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES,
                None,
                Some(self.overlapped.as_mut()),
                None,
            );
            if let Err(error) = result
                && error.code() != ERROR_IO_PENDING.to_hresult()
            {
                return Err(error).context("ReadDirectoryChangesW");
            }
        }
        self.active = true;
        Ok(())
    }

    fn complete(&mut self) -> Result<Option<Vec<PathBuf>>> {
        let mut bytes = 0;
        // SAFETY: called only after the completion event is signaled; storage is still alive.
        let result =
            unsafe { GetOverlappedResult(raw(&self.folder), &*self.overlapped, &mut bytes, false) };
        self.active = false;
        if let Err(error) = result
            && error.code() != ERROR_NOTIFY_ENUM_DIR.to_hresult()
        {
            return Err(error).context("directory notification completion");
        }
        if bytes == 0 {
            return Ok(None);
        }
        // SAFETY: DWORD-aligned buffer contains `bytes` bytes written by the completed read.
        let records =
            unsafe { std::slice::from_raw_parts(self.buffer.as_ptr().cast(), bytes as usize) };
        Ok(paths(records))
    }
}

impl Drop for Read {
    fn drop(&mut self) {
        if self.active {
            let mut bytes = 0;
            // SAFETY: exclusively owned I/O, and both allocations remain alive until after
            // cancellation completes (including ERROR_NOT_FOUND when completion won the race).
            unsafe {
                let _ = CancelIoEx(raw(&self.folder), Some(&*self.overlapped));
                let _ = GetOverlappedResult(raw(&self.folder), &*self.overlapped, &mut bytes, true);
            }
        }
    }
}

fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<windows::core::Error>()
        .is_some_and(|error| {
            [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_DIRECTORY]
                .iter()
                .any(|code| error.code() == code.to_hresult())
        })
}

/// Reopen after failed I/O. Transient failures back off from 25 ms to 1 s; an absent directory
/// sleeps on its nearest surviving ancestor instead, with no timed existence checks.
fn recover(root: &Path, stop: HANDLE, recursive: bool, buffer_bytes: usize) -> Option<Read> {
    let mut delay = 25;
    loop {
        // SAFETY: the worker owns a live stop event throughout recovery.
        if unsafe { WaitForMultipleObjects(&[stop], false, 0) } != WAIT_TIMEOUT {
            return None;
        }
        let opened = Read::open(root, buffer_bytes).and_then(|mut read| {
            read.arm(recursive)?;
            Ok(read)
        });
        match opened {
            Ok(read) => return Some(read),
            Err(error) if missing(&error) => {
                match wait_for_directory(root, stop, buffer_bytes) {
                    Ok(true) => continue,
                    Ok(false) => return None,
                    Err(_) => {} // Ancestor vanished or is inaccessible; retry with backoff.
                }
            }
            Err(_) => {}
        }
        // SAFETY: the stop event also makes a transient-error backoff immediately cancellable.
        if unsafe { WaitForMultipleObjects(&[stop], false, delay) } != WAIT_TIMEOUT {
            return None;
        }
        delay = (delay * 2).min(1000);
    }
}

/// Arm before rechecking existence so a recreation cannot fall between the check and wait.
/// Recursive watching also covers the case where several levels of parent were removed.
fn wait_for_directory(root: &Path, stop: HANDLE, buffer_bytes: usize) -> Result<bool> {
    for ancestor in root.ancestors().skip(1) {
        let mut parent = match Read::open(ancestor, buffer_bytes).and_then(|mut read| {
            read.arm(true)?;
            Ok(read)
        }) {
            Ok(read) => read,
            Err(error) if missing(&error) => continue,
            Err(error) => return Err(error),
        };
        if root.is_dir() {
            return Ok(true);
        }
        // SAFETY: both events are owned throughout this wait; Drop drains the parent read.
        let result = unsafe { WaitForMultipleObjects(&[stop, raw(&parent.done)], false, INFINITE) };
        if result == WAIT_OBJECT_0 {
            return Ok(false);
        }
        if result.0 == WAIT_OBJECT_0.0 + 1 {
            parent.complete()?;
            return Ok(true);
        }
        bail!(
            "parent directory wait failed: {:?}",
            windows::core::Error::from_thread()
        );
    }
    bail!("no surviving parent for {}", root.display())
}

#[derive(Default)]
struct Pending {
    paths: BTreeSet<PathBuf>,
    rescan: bool,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Pending {
    fn add(&mut self, paths: Option<Vec<PathBuf>>, root: &Path, names: &[OsString], now: Instant) {
        match paths {
            None => {
                self.paths.clear();
                self.rescan = true;
            }
            Some(paths) => {
                let selected = paths.into_iter().filter(|p| {
                    names.is_empty()
                        || names
                            .iter()
                            .any(|name| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)))
                });
                if !self.rescan {
                    self.paths.extend(selected.map(|p| root.join(p)));
                    if self.paths.len() > MAX_PATHS {
                        self.paths.clear();
                        self.rescan = true;
                    }
                }
            }
        }
        if self.rescan || !self.paths.is_empty() {
            self.first.get_or_insert(now);
            self.last = Some(now);
        }
    }

    fn timeout(&self, debounce: Duration) -> u32 {
        let (Some(first), Some(last)) = (self.first, self.last) else {
            return INFINITE;
        };
        let deadline = (last + debounce).min(first + MAX_LATENCY);
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(INFINITE - 1)) as u32
    }

    fn take(&mut self) -> Event {
        let batch = std::mem::take(self);
        if batch.rescan {
            Event::Rescan
        } else {
            Event::Paths(batch.paths.into_iter().collect())
        }
    }
}

fn run(
    read: &mut Read,
    stop: HANDLE,
    root: &Path,
    recursive: bool,
    names: &[OsString],
    debounce: Duration,
    changed: &mut dyn FnMut(Event),
) -> Result<()> {
    let mut pending = Pending::default();
    loop {
        // Stop is first so an endless stream of filesystem changes cannot starve shutdown.
        // SAFETY: both events remain alive for the whole loop; no polling when pending is empty.
        let result = unsafe {
            WaitForMultipleObjects(&[stop, raw(&read.done)], false, pending.timeout(debounce))
        };
        if result == WAIT_OBJECT_0 {
            return Ok(());
        }
        if result == WAIT_TIMEOUT {
            changed(pending.take());
        } else if result.0 == WAIT_OBJECT_0.0 + 1 {
            let paths = read.complete()?;
            read.arm(recursive)?; // Drain promptly, including while a callback runs.
            pending.add(paths, root, names, Instant::now());
            // Continuous changes still deliver at the bounded latency, even if I/O always wins.
            if pending.timeout(debounce) == 0 {
                changed(pending.take());
            }
        } else {
            bail!(
                "directory wait failed: {:?}",
                windows::core::Error::from_thread()
            );
        }
    }
}

/// Decode FILE_NOTIFY_INFORMATION without aligned casts. A malformed/truncated batch is a
/// rescan, never a partial list (a missing rename/delete would leave caller state stale).
fn paths(records: &[u8]) -> Option<Vec<PathBuf>> {
    let word = |at: usize| {
        records
            .get(at..at.checked_add(4)?)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    };
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let next = word(at)?;
        let length = word(at.checked_add(8)?)?;
        if length == 0 || length % 2 != 0 {
            return None;
        }
        let start = at.checked_add(12)?;
        let end = start.checked_add(length)?;
        let name = records.get(start..end)?;
        let wide: Vec<u16> = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let path = PathBuf::from(OsString::from_wide(&wide));
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        out.push(path);
        if next == 0 {
            return Some(out);
        }
        if next % 4 != 0 || next < 12 + length {
            return None;
        }
        at = at.checked_add(next)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(next: u32, name: &str) -> Vec<u8> {
        let wide: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut bytes = Vec::new();
        bytes.extend(next.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend((wide.len() as u32).to_le_bytes());
        bytes.extend(wide);
        bytes
    }

    #[test]
    fn records_preserve_unicode_and_rename_names_and_reject_truncation() {
        let next = record(0, "old.json").len().next_multiple_of(4);
        let mut chain = record(next as u32, "old.json");
        chain.resize(next, 0);
        chain.extend(record(0, r"sub\新.json"));
        assert_eq!(
            paths(&chain),
            Some(vec!["old.json".into(), r"sub\新.json".into()])
        );
        for length in 0..12 {
            assert_eq!(paths(&chain[..length]), None);
        }
        assert_eq!(paths(&chain[..chain.len() - 1]), None);
        assert_eq!(paths(&record(4, "bad-offset")), None);
        assert_eq!(paths(&record(0, r"..\escape")), None);
        let mut odd = record(0, "odd");
        odd[8..12].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(paths(&odd), None);
    }

    #[test]
    fn filters_coalesce_and_overflow_requests_a_rescan() {
        let mut pending = Pending::default();
        let root = Path::new(r"C:\fixture");
        let names = [OsString::from("config.json")];
        assert_eq!(pending.timeout(DEBOUNCE), INFINITE);
        pending.add(
            Some(vec!["other.json".into()]),
            root,
            &names,
            Instant::now(),
        );
        assert_eq!(
            pending.timeout(DEBOUNCE),
            INFINITE,
            "unrelated files never arm timers"
        );
        pending.add(
            Some(vec!["CONFIG.JSON".into(), "CONFIG.JSON".into()]),
            root,
            &names,
            Instant::now(),
        );
        assert_eq!(pending.take(), Event::Paths(vec![root.join("CONFIG.JSON")]));
        assert_eq!(pending.timeout(DEBOUNCE), INFINITE);
        pending.add(None, root, &names, Instant::now());
        pending.add(
            Some(vec!["config.json".into()]),
            root,
            &names,
            Instant::now(),
        );
        assert_eq!(pending.take(), Event::Rescan);
        pending.add(
            Some(
                (0..=MAX_PATHS)
                    .map(|i| PathBuf::from(i.to_string()))
                    .collect(),
            ),
            root,
            &[],
            Instant::now(),
        );
        assert_eq!(pending.take(), Event::Rescan, "pending memory is bounded");
    }

    #[test]
    fn a_busy_directory_cannot_postpone_delivery_forever() {
        let now = Instant::now();
        let pending = Pending {
            first: Some(now - MAX_LATENCY),
            last: Some(now),
            ..Default::default()
        };
        assert_eq!(pending.timeout(DEBOUNCE), 0);
    }

    #[test]
    fn real_kernel_overflow_is_reported_and_watching_continues() {
        let dir =
            std::env::temp_dir().join(format!("mypaper-fswatch-overflow-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let mut first = true;
        // A small native buffer plus a blocked callback deterministically overruns the kernel
        // queue. The production loop is unchanged; this is a real ReadDirectoryChangesW loss.
        let watch = Watch::start(&dir, false, vec![], 1024, Duration::ZERO, move |event| {
            let _ = tx.send(event);
            if first {
                first = false;
                let _ = blocked.recv_timeout(Duration::from_secs(10));
            }
        })
        .unwrap();
        std::fs::write(dir.join("first"), "a").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for i in 0..800 {
            std::fs::write(dir.join(format!("overflow-{i:04}")), "x").unwrap();
        }
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut overflow = false;
        while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if event == Event::Rescan {
                overflow = true;
                break;
            }
        }
        assert!(overflow, "lost kernel records must trigger a rescan");
        while rx.try_recv().is_ok() {}
        std::fs::write(dir.join("after-overflow"), "a").unwrap();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if matches!(event, Event::Paths(ref paths) if paths.contains(&dir.join("after-overflow")))
            {
                break;
            }
        }
        drop(watch);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
