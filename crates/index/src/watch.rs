//! Blocking directory notifications: no polling or periodic wakeups. A 1 MiB kernel buffer
//! per watched root survives between reads. Bounded delivery turns backlog into reconciliation.

use std::path::PathBuf;
use std::sync::mpsc::SyncSender;

use anyhow::Result;
use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER};
use windows::Win32::Storage::FileSystem::*;
use windows::core::PCWSTR;

use crate::scan::{Background, long};

#[derive(Debug)]
pub enum Change {
    Paths {
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// Notifications no longer contain the lost names. The affected watched subtree is root.
    Overflow(PathBuf),
}

pub fn start(root: PathBuf, tx: SyncSender<Change>) -> Result<()> {
    let name = long(&root);
    // SAFETY: opens only directory-list access, with all sharing modes; no elevation required.
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
    // A raw owned HANDLE is sent as an integer; the worker closes it exactly once.
    let raw = handle.0 as usize;
    std::thread::Builder::new()
        .name(format!("index-watch-{}", root.display()))
        .spawn(move || {
            let _background = Background::enter();
            let handle = windows::Win32::Foundation::HANDLE(raw as *mut _);
            let mut buffer = vec![0u64; 1024 * 1024 / 8];
            let mut extended = true;
            let filter = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_SIZE
                | FILE_NOTIFY_CHANGE_LAST_WRITE
                | FILE_NOTIFY_CHANGE_ATTRIBUTES;
            loop {
                let mut bytes = 0;
                // SAFETY: aligned, live 1 MiB output; synchronous I/O keeps it alive until return.
                let read = unsafe {
                    if extended {
                        ReadDirectoryChangesExW(
                            handle,
                            buffer.as_mut_ptr().cast(),
                            (buffer.len() * 8) as u32,
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
                            (buffer.len() * 8) as u32,
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
                        eprintln!("watch {} stopped: {error}", root.display());
                        break;
                    }
                }
                if read.is_err() || bytes == 0 {
                    if tx.send(Change::Overflow(root.clone())).is_err() {
                        break;
                    }
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
                match tx.try_send(Change::Paths {
                    root: root.clone(),
                    paths,
                }) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        if tx.send(Change::Overflow(root.clone())).is_err() {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
            // SAFETY: this worker owns the directory handle.
            let _ = unsafe { CloseHandle(handle) };
        })?;
    Ok(())
}
