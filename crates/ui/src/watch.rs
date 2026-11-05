//! File reloads use native change signals. Workers sleep until a directory changes or the
//! owner drops the watch; callbacks run only when the file's bytes change, including removal.
use anyhow::{Context, Result};
use std::{path::Path, thread::JoinHandle};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    },
    System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects},
};

/// Watch one file across writes, atomic replacements, removals and recreations.
pub struct FileWatch {
    stop: HANDLE,
    worker: Option<JoinHandle<()>>,
}
impl FileWatch {
    pub fn new(path: &Path, changed: impl Fn() + Send + 'static) -> Result<Self> {
        let parent = path.parent().context("watched file has no parent")?;
        std::fs::create_dir_all(parent)?;
        // SAFETY: the native watch owns its directory notification and stop event. Only the
        // worker waits on them; Drop signals stop, joins it, then closes the event.
        unsafe {
            let signal = FindFirstChangeNotificationW(
                &windows::core::HSTRING::from(parent.as_os_str()),
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
            )?;
            let stop = CreateEventW(None, true, false, None).inspect_err(|_| {
                let _ = FindCloseChangeNotification(signal);
            })?;
            let initial = std::fs::read(path).ok();
            let path = path.to_owned();
            let (signal_id, stop_id) = (signal.0 as usize, stop.0 as usize);
            let worker = std::thread::Builder::new()
                .name("ui-file-change".into())
                .stack_size(256 * 1024)
                .spawn(move || {
                    let (signal, stop) = (HANDLE(signal_id as _), HANDLE(stop_id as _));
                    let mut seen = initial;
                    loop {
                        if WaitForMultipleObjects(&[stop, signal], false, INFINITE).0
                            != WAIT_OBJECT_0.0 + 1
                        {
                            break;
                        }
                        // Rearm before reading, so an atomic replacement during the read also wakes us.
                        if FindNextChangeNotification(signal).is_err() {
                            break;
                        }
                        let next = std::fs::read(&path).ok();
                        if next != seen {
                            seen = next;
                            changed();
                        }
                    }
                    let _ = FindCloseChangeNotification(signal);
                })
                .inspect_err(|_| {
                    let _ = FindCloseChangeNotification(signal);
                    let _ = CloseHandle(stop);
                })?;
            Ok(Self {
                stop,
                worker: Some(worker),
            })
        }
    }
}
impl Drop for FileWatch {
    fn drop(&mut self) {
        // SAFETY: stop remains open until its only waiting thread has exited.
        unsafe {
            let _ = SetEvent(self.stop);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            let _ = CloseHandle(self.stop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_replacement_removal_and_recreation_reload_without_a_poll_timer() {
        let root =
            std::env::temp_dir().join(format!("mypaper-widget-watch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("widgets.json");
        std::fs::write(&path, b"{}").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let watch = FileWatch::new(&path, move || {
            let _ = tx.send(());
        })
        .unwrap();
        let wait = || rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let temp = root.join("widgets.json.tmp");
        std::fs::write(&temp, b"{\"overlay\":false}").unwrap();
        std::fs::rename(temp, &path).unwrap();
        wait();
        std::fs::remove_file(&path).unwrap();
        wait();
        std::fs::write(&path, b"{}").unwrap();
        wait();
        drop(watch);
        std::fs::remove_dir_all(root).unwrap();
    }
}
