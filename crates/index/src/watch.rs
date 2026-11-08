//! Blocking notifications with a coalesced handoff independent of slow metadata work.
//! Readers never block on the service queue and drain kernel notifications at normal CPU
//! priority. Enumeration and delivery work remain background priority.

use std::collections::HashSet;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc::SyncSender};

use anyhow::Result;
use windows::Win32::Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, HANDLE};
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};
use windows::core::PCWSTR;

use crate::scan::{Background, long};

#[derive(Debug)]
pub enum Change {
    InitialDone(Vec<PathBuf>),
    Rescan,
    Queried,
    RulesChanged,
    Activity,
    VolumesChanged,
    Paths {
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
    Directories {
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// Lost names cannot identify a narrower subtree than the watched root.
    Overflow(PathBuf),
    Failed {
        root: PathBuf,
        error: String,
    },
}

const MAX_PENDING: usize = 128_000;
const BUFFER_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_DIRTY: usize = 8192;

#[derive(Default)]
struct Pending {
    paths: HashSet<PathBuf>,
    directories: HashSet<PathBuf>,
    overflow: bool,
    stopped: Option<String>,
    failed: bool,
    closed: bool,
}

impl Pending {
    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        if self.overflow {
            return;
        }
        self.paths.extend(paths);
        if self.paths.len() > MAX_PENDING {
            self.paths.clear();
            self.overflow = true;
        }
    }

    fn dirty(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        if self.overflow {
            return;
        }
        for path in paths {
            if let Some(parent) = path.parent() {
                self.directories.insert(parent.to_owned());
                if self.directories.len() > MAX_DIRTY {
                    self.paths.clear();
                    self.directories.clear();
                    self.overflow = true;
                    break;
                }
            }
        }
    }
}

pub(crate) struct Watch {
    stop: Arc<OwnedHandle>,
    reader: Option<std::thread::JoinHandle<()>>,
    pending: Arc<(Mutex<Pending>, Condvar)>,
}

impl Watch {
    pub fn failed(&self) -> bool {
        self.pending.0.lock().expect("watch handoff").failed
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: owned stop event remains alive until the reader has cancelled its I/O.
        let _ = unsafe { SetEvent(HANDLE(self.stop.as_raw_handle())) };
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.pending.0.lock().expect("watch handoff").closed = true;
        self.pending.1.notify_one();
    }
}

// Owns the overlapped storage; all exits drain cancellation before freeing the buffer.
struct Read {
    folder: OwnedHandle,
    done: OwnedHandle,
    operation: Box<OVERLAPPED>,
    buffer: Vec<u64>,
}

impl Drop for Read {
    fn drop(&mut self) {
        let mut bytes = 0;
        // SAFETY: this directory owns the I/O; both allocations outlive its completion.
        unsafe {
            let _ = CancelIoEx(
                HANDLE(self.folder.as_raw_handle()),
                Some(self.operation.as_ref()),
            );
            let _ = GetOverlappedResult(
                HANDLE(self.folder.as_raw_handle()),
                self.operation.as_ref(),
                &mut bytes,
                true,
            );
        }
    }
}

fn event() -> Result<OwnedHandle> {
    // SAFETY: transfers a newly created manual-reset event into its sole owner.
    Ok(unsafe { OwnedHandle::from_raw_handle(CreateEventW(None, true, false, PCWSTR::null())?.0) })
}

pub(crate) fn start(
    root: PathBuf,
    tx: SyncSender<Change>,
    rules: Arc<std::sync::RwLock<crate::rules::Rules>>,
    quiet: Arc<crate::quiet::Quiet>,
) -> Result<Watch> {
    let name = long(&root);
    // SAFETY: directory-list access only, with all sharing modes; no elevation required.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            None,
        )?
    };
    // SAFETY: transfers a successful native handle to its Send-capable owner.
    let owner = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let stop = Arc::new(event()?);
    let reader_stop = stop.clone();
    let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
    let delivery = pending.clone();
    let watched = root.clone();
    std::thread::Builder::new()
        .name(format!("index-delivery-{}", root.display()))
        .spawn(move || {
            let _background = Background::enter();
            loop {
                let (paths, directories, overflow, stopped) = {
                    let mut state = delivery.0.lock().expect("watch handoff");
                    while state.paths.is_empty()
                        && state.directories.is_empty()
                        && !state.overflow
                        && state.stopped.is_none()
                        && !state.closed
                    {
                        state = delivery.1.wait(state).expect("watch wake");
                    }
                    if state.closed {
                        break;
                    }
                    (
                        std::mem::take(&mut state.paths)
                            .into_iter()
                            .collect::<Vec<_>>(),
                        std::mem::take(&mut state.directories)
                            .into_iter()
                            .collect::<Vec<_>>(),
                        std::mem::take(&mut state.overflow),
                        state.stopped.take(),
                    )
                };
                // Only delivery can wait on the bounded queue. Its reader keeps coalescing paths
                // while enumeration or checkpointing is busy.
                if overflow && tx.send(Change::Overflow(watched.clone())).is_err() {
                    break;
                }
                if !paths.is_empty()
                    && tx
                        .send(Change::Paths {
                            root: watched.clone(),
                            paths,
                        })
                        .is_err()
                {
                    break;
                }
                if !directories.is_empty()
                    && tx
                        .send(Change::Directories {
                            root: watched.clone(),
                            paths: directories,
                        })
                        .is_err()
                {
                    break;
                }
                if let Some(error) = stopped {
                    let _ = tx.send(Change::Failed {
                        root: watched.clone(),
                        error,
                    });
                    break;
                }
            }
        })?;
    let reader_pending = pending.clone();
    let (ready, armed) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name(format!("index-watch-{}", root.display()))
        .spawn(move || {
            let pending = reader_pending;
            let done = match event() {
                Ok(event) => event,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let mut read = Read {
                folder: owner,
                operation: Box::new(OVERLAPPED {
                    hEvent: HANDLE(done.as_raw_handle()),
                    ..Default::default()
                }),
                done,
                buffer: vec![0u64; BUFFER_BYTES / 8],
            };
            let handle = HANDLE(read.folder.as_raw_handle());
            let mut ready = Some(ready);
            let mut extended = true;
            let filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
            loop {
                let mut bytes = 0;
                // SAFETY: owned aligned buffer and boxed operation stay alive through completion.
                let armed = unsafe {
                    let _ = ResetEvent(HANDLE(read.done.as_raw_handle()));
                    if extended {
                        ReadDirectoryChangesExW(
                            handle,
                            read.buffer.as_mut_ptr().cast(),
                            BUFFER_BYTES as u32,
                            true,
                            filter,
                            None,
                            Some(read.operation.as_mut()),
                            None,
                            ReadDirectoryNotifyInformation,
                        )
                    } else {
                        ReadDirectoryChangesW(
                            handle,
                            read.buffer.as_mut_ptr().cast(),
                            BUFFER_BYTES as u32,
                            true,
                            filter,
                            None,
                            Some(read.operation.as_mut()),
                            None,
                        )
                    }
                };
                if let Err(error) = &armed {
                    if extended
                        && (error.code() == ERROR_INVALID_FUNCTION.to_hresult()
                            || error.code() == ERROR_INVALID_PARAMETER.to_hresult())
                    {
                        extended = false;
                        continue;
                    }
                    if error.code()
                        == windows::Win32::Foundation::ERROR_NOTIFY_ENUM_DIR.to_hresult()
                    {
                        pending.0.lock().expect("watch handoff").overflow = true;
                        pending.1.notify_one();
                        continue;
                    }
                    if error.code() != windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() {
                        let mut state = pending.0.lock().expect("watch handoff");
                        state.failed = true;
                        state.stopped = Some(error.to_string());
                        drop(state);
                        pending.1.notify_one();
                        break;
                    }
                }
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Ok(()));
                }
                // Stop wins over a completed batch. There is no timer, including in games.
                // SAFETY: both wait handles and pending I/O storage remain owned by this thread.
                let completed = unsafe {
                    WaitForMultipleObjects(
                        &[
                            HANDLE(reader_stop.as_raw_handle()),
                            HANDLE(read.done.as_raw_handle()),
                        ],
                        false,
                        INFINITE,
                    )
                };
                if completed == windows::Win32::Foundation::WAIT_OBJECT_0 {
                    break;
                }
                let result = if completed.0 == windows::Win32::Foundation::WAIT_OBJECT_0.0 + 1 {
                    // SAFETY: the read's completion event is signalled; output storage is valid.
                    unsafe {
                        GetOverlappedResult(handle, read.operation.as_ref(), &mut bytes, false)
                    }
                } else {
                    Err(windows::core::Error::from_thread())
                };
                if let Err(error) = &result
                    && error.code()
                        != windows::Win32::Foundation::ERROR_NOTIFY_ENUM_DIR.to_hresult()
                {
                    let mut state = pending.0.lock().expect("watch handoff");
                    state.failed = true;
                    state.stopped = Some(error.to_string());
                    drop(state);
                    pending.1.notify_one();
                    break;
                }
                if result.is_err() || bytes == 0 {
                    pending.0.lock().expect("watch handoff").overflow = true;
                    pending.1.notify_one();
                    continue;
                }
                let paused = quiet.paused();
                let data: &[u8] = bytemuck::cast_slice(&read.buffer);
                let mut at = 0usize;
                let mut paths = Vec::new();
                while at + 12 <= bytes as usize {
                    let next =
                        u32::from_le_bytes(data[at..at + 4].try_into().expect("next")) as usize;
                    let name_len =
                        u32::from_le_bytes(data[at + 8..at + 12].try_into().expect("length"))
                            as usize;
                    if name_len % 2 != 0 || at + 12 + name_len > bytes as usize {
                        break;
                    }
                    let name: Vec<u16> = data[at + 12..at + 12 + name_len]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    let path = root.join(String::from_utf16_lossy(&name));
                    if paused || rules.read().expect("index rules").notify(&path) {
                        paths.push(path);
                    }
                    if next < 12 {
                        break;
                    }
                    at += next;
                }
                let mut state = pending.0.lock().expect("watch handoff");
                if paused {
                    state.dirty(paths);
                } else {
                    state.add(paths);
                }
                drop(state);
                pending.1.notify_one();
            }
        })?;
    let watch = Watch {
        stop,
        reader: Some(reader),
        pending,
    };
    armed
        .recv()
        .map_err(|_| anyhow::anyhow!("watch stopped before arming"))??;
    Ok(watch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_reader_drains_during_pause_without_metadata_or_exact_path_queueing() {
        let root =
            std::env::temp_dir().join(format!("glaze-paused-watch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(256);
        let rules = Arc::new(std::sync::RwLock::new(
            crate::rules::Rules::new(&Default::default(), std::path::Path::new("")).unwrap(),
        ));
        let quiet = Arc::new(crate::quiet::Quiet::default());
        quiet.set_paused_for_test(true);
        let watch = start(root.clone(), tx, rules, quiet.clone()).unwrap();
        // Even before resume, a completed read must reach the directory handoff. The old
        // quiet.wait() reader parked here indefinitely, retaining only Windows' raw buffer.
        std::fs::write(root.join("paused-file.txt"), b"123").unwrap();
        let change = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let Change::Directories {
            root: watched,
            paths,
        } = change
        else {
            panic!("expected dirty directories during pause: {change:?}");
        };
        assert_eq!(watched, root);
        assert_eq!(paths, vec![root.clone()]);
        assert!(quiet.paused());
        drop(watch); // Stop must also work while the background gate remains closed.
        quiet.set_paused_for_test(false);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleted_root_stops_only_its_watch_and_idle_watch_cancels_on_drop() {
        let base = std::env::temp_dir().join(format!("glaze-watch-roots-{}", std::process::id()));
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(256);
        let rules = Arc::new(std::sync::RwLock::new(
            crate::rules::Rules::new(&Default::default(), std::path::Path::new("")).unwrap(),
        ));
        let quiet = Arc::new(crate::quiet::Quiet::default());
        let first = start(a.clone(), tx.clone(), rules.clone(), quiet.clone()).unwrap();
        let second = start(b.clone(), tx, rules, quiet).unwrap();
        std::fs::remove_dir(&a).unwrap();
        loop {
            if let Change::Failed { root, .. } =
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
            {
                assert_eq!(root, a);
                break;
            }
        }
        assert!(first.failed());
        drop(first);
        let file = b.join("still-alive.txt");
        std::fs::write(&file, b"123").unwrap();
        loop {
            if let Change::Paths { root, paths } =
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
            {
                assert_eq!(root, b);
                if paths.contains(&file) {
                    break;
                }
            }
        }
        drop(second);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn paused_burst_retains_directories_and_bounds_distinct_directory_loss() {
        let mut pending = Pending::default();
        pending.dirty((0..200_000).map(|i| PathBuf::from(format!(r"C:\scratch\{i}.txt"))));
        assert_eq!(pending.directories.len(), 1);
        assert!(pending.paths.is_empty());
        assert!(!pending.overflow);
        pending.dirty((0..MAX_DIRTY).map(|i| PathBuf::from(format!(r"C:\scratch\{i}\file.txt"))));
        assert!(pending.overflow);
        assert!(pending.directories.is_empty());
    }

    #[test]
    fn slow_consumers_coalesce_paths_and_rescan_only_on_real_loss() {
        let mut pending = Pending::default();
        for _ in 0..1000 {
            pending.add([PathBuf::from(r"C:\build\same.obj")]);
        }
        assert_eq!(pending.paths.len(), 1);
        assert!(!pending.overflow);
        pending.add((0..MAX_PENDING).map(|i| PathBuf::from(format!(r"C:\build\{i}.obj"))));
        assert!(pending.overflow);
        assert!(pending.paths.is_empty());
    }
}
