//! File reloads use native change signals. Workers sleep until a directory changes or the
//! owner drops the watch; callbacks run only when the file's bytes change, including removal.
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
        FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    },
    System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects},
};

struct Stop(usize);
impl Drop for Stop {
    fn drop(&mut self) {
        // SAFETY: the last owner closes this shared event, after any waiting worker exits.
        let _ = unsafe { CloseHandle(HANDLE(self.0 as _)) };
    }
}

/// Watch one file across writes, atomic replacements, removals and recreations. Drop requests
/// cancellation without blocking the UI thread; an already running callback may finish later.
pub struct FileWatch {
    stop: Arc<Stop>,
}
impl FileWatch {
    pub fn new(path: &Path, changed: impl Fn() + Send + 'static) -> Result<Self> {
        let parent = path.parent().context("watched file has no parent")?;
        std::fs::create_dir_all(parent)?;
        // SAFETY: the worker owns the directory notification. Its shared event stays open
        // until both the owner and worker release it, even when cancellation overlaps a wait.
        unsafe {
            let signal = FindFirstChangeNotificationW(
                &windows::core::HSTRING::from(parent.as_os_str()),
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
            )?;
            let stop = Arc::new(Stop(
                CreateEventW(None, true, false, None)
                    .inspect_err(|_| {
                        let _ = FindCloseChangeNotification(signal);
                    })?
                    .0 as usize,
            ));
            let initial = std::fs::read(path).ok();
            let path = path.to_owned();
            let signal_id = signal.0 as usize;
            let worker_stop = stop.clone();
            std::thread::Builder::new()
                .name("ui-file-change".into())
                .stack_size(256 * 1024)
                .spawn(move || {
                    let (signal, stop) = (HANDLE(signal_id as _), HANDLE(worker_stop.0 as _));
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
                })?;
            Ok(Self { stop })
        }
    }
}
impl Drop for FileWatch {
    fn drop(&mut self) {
        // Windows terminates other threads before running process-exit TLS destructors.
        // Joining there panics in Rust (or deadlocks under the loader lock). Cancellation is
        // nonblocking; the detached worker closes its own notification/event on normal exit.
        // SAFETY: this owner's Arc keeps the shared stop event alive throughout SetEvent.
        let _ = unsafe { SetEvent(HANDLE(self.stop.0 as _)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_thread_local_watch_can_be_dropped_during_process_exit() {
        const CHILD: &str = "MYPAPER_UI_TLS_WATCH_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            thread_local! { static WATCH: std::cell::RefCell<Option<FileWatch>> = const { std::cell::RefCell::new(None) }; }
            WATCH.with_borrow_mut(|slot| {
                *slot = Some(FileWatch::new(&Path::new(&root).join("config.json"), || {}).unwrap());
            });
            // Exercises Windows' real process-exit TLS ordering, with another thread alive.
            std::process::exit(0);
        }
        let root = std::env::temp_dir().join(format!("mypaper-watch-exit-{}", std::process::id()));
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "watch::tests::a_thread_local_watch_can_be_dropped_during_process_exit",
                "--nocapture",
            ])
            .env(CHILD, &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
