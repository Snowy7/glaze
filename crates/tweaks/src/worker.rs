//! One latest-request mailbox and a Windows message pump. The worker sleeps indefinitely
//! when idle; the short debounce exists only while a change is pending. WinEvent hooks live
//! here too, so a slow settings receiver never blocks core's supervisor/panic-hotkey thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_FAILED};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage,
};

pub const DEBOUNCE: Duration = Duration::from_millis(35);

struct Event(HANDLE);

// SAFETY: Windows event handles support waiting/signalling from different threads; Arc keeps
// this handle alive until both the worker and its sender have finished using it.
unsafe impl Send for Event {}
// SAFETY: the event has no Rust mutable state, and SetEvent is thread-safe.
unsafe impl Sync for Event {}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: this is the event we created, closed only by its last owner.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Mailbox<T> {
    latest: Option<(u64, T)>,
    revision: u64,
    stop: bool,
}

struct Shared<T> {
    mailbox: Mutex<Mailbox<T>>,
    event: Event,
    completed: AtomicU64,
}

/// Also used by the isolated timing fixture, with fake registry/windows/receivers.
pub struct Worker<T> {
    shared: Arc<Shared<T>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Worker<T> {
    /// Initializes the controller on the worker, before accepting any requests.
    pub fn start<F: FnMut(T) + 'static>(
        init: impl FnOnce() -> Result<F> + Send + 'static,
    ) -> Result<Self> {
        // SAFETY: unnamed auto-reset event, owned by Shared.
        let event = Event(unsafe { CreateEventW(None, false, false, None)? });
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox { latest: None, revision: 0, stop: false }),
            event,
            completed: AtomicU64::new(0),
        });
        let worker = shared.clone();
        let (ready, receive) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("windows-style".into()).spawn(move || {
            let mut apply = match init() {
                Ok(apply) => { let _ = ready.send(Ok(())); apply }
                Err(error) => { let _ = ready.send(Err(error)); return; }
            };
            let mut deadline = None::<Instant>;
            let mut seen = 0;
            loop {
                let timeout = deadline.map_or(INFINITE, |at| {
                    at.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32
                });
                // SAFETY: the event remains open; this thread owns the message queue/hooks.
                let wait = unsafe {
                    MsgWaitForMultipleObjects(Some(&[worker.event.0]), false, timeout, QS_ALLINPUT)
                };
                if wait == WAIT_FAILED {
                    log::error!("Windows styling worker wait failed");
                    break;
                }
                let mut message = MSG::default();
                // SAFETY: removes and dispatches only this worker's messages. PeekMessage
                // also delivers out-of-context WinEvent callbacks on their owning thread.
                unsafe {
                    while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                let task = {
                    let mut mailbox = worker.mailbox.lock().unwrap();
                    if mailbox.stop { break; }
                    if mailbox.revision != seen {
                        seen = mailbox.revision;
                        deadline = Some(Instant::now() + DEBOUNCE);
                    }
                    if deadline.is_some_and(|at| Instant::now() >= at) {
                        deadline = None;
                        mailbox.latest.take()
                    } else { None }
                };
                if let Some((revision, task)) = task {
                    apply(task);
                    worker.completed.store(revision, Ordering::Release);
                }
            }
            // `apply` owns the controller: dropping it restores before this thread exits.
        }).context("starting Windows styling worker")?;
        receive.recv().context("Windows styling worker initialization")??;
        Ok(Self { shared, thread: Some(thread) })
    }

    /// Replaces an unprocessed request. No IO or receiver waits run on the caller.
    pub fn submit(&self, task: T) {
        {
            let mut mailbox = self.shared.mailbox.lock().unwrap();
            mailbox.revision += 1;
            mailbox.latest = Some((mailbox.revision, task));
        }
        // SAFETY: Shared owns this event for the entire call.
        let _ = unsafe { SetEvent(self.shared.event.0) };
    }

    pub fn is_idle(&self) -> bool {
        self.shared.completed.load(Ordering::Acquire) == self.shared.mailbox.lock().unwrap().revision
    }
}

impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        self.shared.mailbox.lock().unwrap().stop = true;
        // SAFETY: Shared still owns the event while the worker finishes.
        let _ = unsafe { SetEvent(self.shared.event.0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_request_replaces_changes_arriving_during_a_slow_receiver() {
        let (seen, receive) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = Worker::start(move || Ok(move |value| {
            seen.send(value).unwrap();
            if value == 0 { gate.recv().unwrap(); }
        })).unwrap();
        worker.submit(0);
        assert_eq!(receive.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
        // The consumer is deliberately blocked. None of these submissions may wait on it.
        for value in 1..=100 { worker.submit(value); }
        release.send(()).unwrap();
        assert_eq!(receive.recv_timeout(Duration::from_secs(2)).unwrap(), 100);
        drop(worker);
        assert!(receive.try_recv().is_err());
    }
}
