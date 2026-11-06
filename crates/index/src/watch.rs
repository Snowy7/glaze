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
use windows::core::PCWSTR;

use crate::scan::{Background, long};

#[derive(Debug)]
pub enum Change {
    InitialDone,
    Queried,
    Paths {
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

#[derive(Default)]
struct Pending {
    paths: HashSet<PathBuf>,
    overflow: bool,
    stopped: Option<String>,
}

impl Pending {
    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.paths.extend(paths);
        if self.paths.len() > MAX_PENDING {
            self.paths.clear();
            self.overflow = true;
        }
    }
}

pub fn start(root: PathBuf, tx: SyncSender<Change>) -> Result<()> {
    let name = long(&root);
    // SAFETY: directory-list access only, with all sharing modes; no elevation required.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )?
    };
    // SAFETY: transfers a successful native handle to its Send-capable owner.
    let owner = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
    let delivery = pending.clone();
    let watched = root.clone();
    std::thread::Builder::new()
        .name(format!("index-delivery-{}", root.display()))
        .spawn(move || {
            let _background = Background::enter();
            loop {
                let (paths, overflow, stopped) = {
                    let mut state = delivery.0.lock().expect("watch handoff");
                    while state.paths.is_empty() && !state.overflow && state.stopped.is_none() {
                        state = delivery.1.wait(state).expect("watch wake");
                    }
                    (
                        std::mem::take(&mut state.paths)
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
                if let Some(error) = stopped {
                    let _ = tx.send(Change::Failed {
                        root: watched.clone(),
                        error,
                    });
                    break;
                }
            }
        })?;
    std::thread::Builder::new()
        .name(format!("index-watch-{}", root.display()))
        .spawn(move || {
            let handle = HANDLE(owner.as_raw_handle());
            let mut buffer = vec![0u64; BUFFER_BYTES / 8];
            let mut extended = true;
            let filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
            loop {
                let mut bytes = 0;
                // SAFETY: aligned 2 MiB output remains alive throughout synchronous I/O.
                let read = unsafe {
                    if extended {
                        ReadDirectoryChangesExW(
                            handle,
                            buffer.as_mut_ptr().cast(),
                            BUFFER_BYTES as u32,
                            true,
                            filter,
                            Some(&mut bytes),
                            None,
                            None,
                            ReadDirectoryNotifyInformation,
                        )
                    } else {
                        ReadDirectoryChangesW(
                            handle,
                            buffer.as_mut_ptr().cast(),
                            BUFFER_BYTES as u32,
                            true,
                            filter,
                            Some(&mut bytes),
                            None,
                            None,
                        )
                    }
                };
                if let Err(error) = &read {
                    if extended
                        && (error.code() == ERROR_INVALID_FUNCTION.to_hresult()
                            || error.code() == ERROR_INVALID_PARAMETER.to_hresult())
                    {
                        extended = false;
                        continue;
                    }
                    if error.code()
                        != windows::Win32::Foundation::ERROR_NOTIFY_ENUM_DIR.to_hresult()
                    {
                        pending.0.lock().expect("watch handoff").stopped = Some(error.to_string());
                        pending.1.notify_one();
                        break;
                    }
                }
                if read.is_err() || bytes == 0 {
                    pending.0.lock().expect("watch handoff").overflow = true;
                    pending.1.notify_one();
                    continue;
                }
                let data: &[u8] = bytemuck::cast_slice(&buffer);
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
                    paths.push(root.join(String::from_utf16_lossy(&name)));
                    if next < 12 {
                        break;
                    }
                    at += next;
                }
                pending.0.lock().expect("watch handoff").add(paths);
                pending.1.notify_one();
            }
            drop(owner);
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
