//! Watching a folder without polling. One thread per watch sleeps in an overlapped
//! `ReadDirectoryChangesW` until the folder changes or the watch is dropped, so a watch costs no
//! wakes while nothing changes. notify 8's Windows backend, which this replaces for mypaper's own
//! watches, wakes its thread every 100 ms whatever happens: ~10 wakes a second per watch, in every
//! process, while the screens are covered, the engine is released or a game runs (2026-11-06).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, ERROR_NOTIFY_ENUM_DIR, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};
use windows::core::{HSTRING, PCWSTR};

/// Names, sizes, writes and creation; never reads (access times).
const FILTER: FILE_NOTIFY_CHANGE = FILE_NOTIFY_CHANGE(
    FILE_NOTIFY_CHANGE_FILE_NAME.0
        | FILE_NOTIFY_CHANGE_DIR_NAME.0
        | FILE_NOTIFY_CHANGE_SIZE.0
        | FILE_NOTIFY_CHANGE_LAST_WRITE.0
        | FILE_NOTIFY_CHANGE_CREATION.0,
);
/// 64 KiB of change records per wake (the most a network folder allows).
const BUFFER_WORDS: usize = 16 * 1024;

/// A folder being watched; dropping it stops the watch (and joins its thread).
pub struct DirWatch {
    stop: Owned,
    thread: Option<JoinHandle<()>>,
}

impl DirWatch {
    /// Calls `on_change` on the watch's thread with the paths (relative to `dir`) that changed in
    /// it, or in its subfolders too with `recursive`; with `None` when too much changed at once to
    /// list (treat everything as changed). Keep `on_change` tiny, e.g. post a message.
    pub fn new(
        dir: &Path,
        recursive: bool,
        mut on_change: impl FnMut(Option<&[PathBuf]>) + Send + 'static,
    ) -> Result<Self> {
        let path = HSTRING::from(dir.as_os_str());
        // SAFETY: plain handle creation; each handle is owned (and closed) by an `Owned`.
        let (folder, stop) = unsafe {
            let folder = Owned(
                CreateFileW(
                    &path,
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    None,
                )
                .with_context(|| format!("opening {} to watch it", dir.display()))?,
            );
            (folder, Owned(CreateEventW(None, true, false, PCWSTR::null())?))
        };
        let stop_raw = stop.0.0 as isize;
        let thread = std::thread::Builder::new()
            .name("dir-watch".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let stop = HANDLE(stop_raw as *mut _);
                if let Err(e) = run(&folder, stop, recursive, &mut on_change) {
                    log::warn!("directory watch ended: {e:#}");
                }
            })
            .context("starting a directory watch")?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        // SAFETY: our own event, alive until after the thread is joined (field drop order).
        let _ = unsafe { SetEvent(self.stop.0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Waits for changes until `stop` is set. Each wake reads one batch of change records.
fn run(
    folder: &Owned,
    stop: HANDLE,
    recursive: bool,
    on_change: &mut dyn FnMut(Option<&[PathBuf]>),
) -> Result<()> {
    // SAFETY: an unnamed manual-reset event this thread owns.
    let done = Owned(unsafe { CreateEventW(None, true, false, PCWSTR::null())? });
    // u32s: the records must be DWORD-aligned.
    let mut buffer = vec![0u32; BUFFER_WORDS];
    loop {
        let mut overlapped = OVERLAPPED {
            hEvent: done.0,
            ..Default::default()
        };
        let mut bytes = 0u32;
        // SAFETY: `buffer` and `overlapped` outlive the operation: below, it either completed or
        // was cancelled and waited for before either is touched again.
        unsafe {
            ResetEvent(done.0)?;
            ReadDirectoryChangesW(
                folder.0,
                buffer.as_mut_ptr().cast(),
                (BUFFER_WORDS * 4) as u32,
                recursive,
                FILTER,
                None,
                Some(&mut overlapped),
                None,
            )
            .context("ReadDirectoryChangesW")?;
            if WaitForMultipleObjects(&[done.0, stop], false, INFINITE) != WAIT_OBJECT_0 {
                let _ = CancelIoEx(folder.0, Some(&overlapped));
                let _ = GetOverlappedResult(folder.0, &overlapped, &mut bytes, true);
                return Ok(());
            }
            if let Err(e) = GetOverlappedResult(folder.0, &overlapped, &mut bytes, false) {
                if e.code() != ERROR_NOTIFY_ENUM_DIR.to_hresult() {
                    return Err(e).context("directory watch");
                }
                bytes = 0;
            }
        }
        if bytes == 0 {
            // More changed than the buffer holds.
            on_change(None);
            continue;
        }
        // SAFETY: the system wrote `bytes` (at most the buffer's size) bytes of records.
        let records = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), bytes as usize)
        };
        on_change(Some(&paths(records)));
    }
}

/// The paths in a chain of `FILE_NOTIFY_INFORMATION` records: `NextEntryOffset`, `Action`,
/// `FileNameLength` (bytes), then the UTF-16 name. Stops at anything out of bounds.
fn paths(records: &[u8]) -> Vec<PathBuf> {
    let word = |at: usize| {
        records
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let mut out = Vec::new();
    let mut at = 0;
    while let (Some(next), Some(length)) = (word(at), word(at + 8)) {
        let Some(name) = records.get(at + 12..at + 12 + length) else {
            break;
        };
        let wide: Vec<u16> = name
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        out.push(PathBuf::from(OsString::from_wide(&wide)));
        if next == 0 {
            break;
        }
        at += next;
    }
    out
}

/// A kernel handle closed on drop; any thread may use it.
struct Owned(HANDLE);

// SAFETY: kernel handles are process-wide; nothing here is thread-affine.
unsafe impl Send for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: a handle we own.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn record(next: u32, name: &str) -> Vec<u8> {
        let wide: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut bytes = Vec::new();
        bytes.extend(next.to_le_bytes());
        bytes.extend(1u32.to_le_bytes()); // FILE_ACTION_ADDED
        bytes.extend((wide.len() as u32).to_le_bytes());
        bytes.extend(wide);
        bytes
    }

    #[test]
    fn records_parse_into_relative_paths_and_bad_lengths_stop_the_walk() {
        let next = record(0, "config.json").len().next_multiple_of(4);
        let mut chain = record(next as u32, "config.json");
        chain.resize(next, 0);
        chain.extend(record(0, r"sub\é.txt"));
        assert_eq!(
            paths(&chain),
            [PathBuf::from("config.json"), PathBuf::from(r"sub\é.txt")]
        );
        assert!(paths(&chain[..10]).is_empty());
        let mut cut = record(0, "config.json");
        cut.truncate(cut.len() - 2);
        assert!(paths(&cut).is_empty(), "a name past the end is not read");
    }

    #[test]
    fn a_watch_reports_writes_renames_and_subfolders_and_stops_at_once() {
        let dir = std::env::temp_dir().join(format!("mypaper-dir-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
        let watch = DirWatch::new(&dir, true, move |changes| {
            let _ = tx.send(changes.map(<[PathBuf]>::to_vec).unwrap_or_default());
        })
        .unwrap();
        let wait_for = |name: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while let Ok(paths) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                if paths.iter().any(|p| p == Path::new(name)) {
                    return true;
                }
            }
            false
        };
        std::fs::write(dir.join("config.json"), "{}").unwrap();
        assert!(wait_for("config.json"), "a write");
        // An atomic save: a temp file renamed over the real one.
        std::fs::write(dir.join("config.json.tmp"), "{ }").unwrap();
        std::fs::rename(dir.join("config.json.tmp"), dir.join("config.json")).unwrap();
        assert!(wait_for("config.json"), "a rename over it");
        std::fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
        assert!(wait_for(r"sub\a.txt"), "a subfolder");
        let started = Instant::now();
        drop(watch);
        assert!(started.elapsed() < Duration::from_secs(1), "stops without waiting for a change");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
