//! A blocking game gate and a clock that stops with it. Queries/watches remain available.
use crate::watch::Change;
use anyhow::Result;
use std::sync::{Arc, Condvar, Mutex, mpsc::SyncSender};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};
use windows::core::PCWSTR;

struct Clock {
    paused_at: Option<Instant>,
    stopped: Duration,
}
pub(crate) struct Quiet {
    epoch: Instant,
    clock: Mutex<Clock>,
    resumed: Condvar,
}
struct Event(usize);
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: exclusively owned event; shared references are gone before drop.
        let _ = unsafe { CloseHandle(HANDLE(self.0 as *mut _)) };
    }
}
impl Default for Quiet {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            clock: Mutex::new(Clock {
                paused_at: None,
                stopped: Duration::ZERO,
            }),
            resumed: Condvar::new(),
        }
    }
}
impl Quiet {
    fn set(&self, paused: bool) {
        let mut clock = self.clock.lock().expect("quiet clock");
        if paused {
            clock.paused_at.get_or_insert_with(Instant::now);
        } else if let Some(at) = clock.paused_at.take() {
            clock.stopped += at.elapsed();
        }
        self.resumed.notify_all();
    }
    pub fn paused(&self) -> bool {
        self.clock.lock().expect("quiet clock").paused_at.is_some()
    }
    pub fn now(&self) -> Duration {
        let clock = self.clock.lock().expect("quiet clock");
        clock
            .paused_at
            .unwrap_or_else(Instant::now)
            .duration_since(self.epoch)
            .saturating_sub(clock.stopped)
    }
    pub fn wait(&self) {
        let mut clock = self.clock.lock().expect("quiet clock");
        while clock.paused_at.is_some() {
            clock = self.resumed.wait(clock).expect("quiet resume");
        }
    }
    pub fn start(tx: SyncSender<Change>) -> Result<Arc<Self>> {
        let gate = Arc::new(Self::default());
        // SAFETY: auto-reset event; notification and owner-death waits share this event.
        let event = Arc::new(Event(
            unsafe { CreateEventW(None, false, false, PCWSTR::null())? }.0 as usize,
        ));
        let callback_event = event.clone();
        let watcher = mypaper_tweaks::game_activity::watch(move || {
            // SAFETY: callback holds the event alive.
            let _ = unsafe { SetEvent(HANDLE(callback_event.0 as *mut _)) };
        })?;
        let monitor = gate.clone();
        // Apply startup policy before starting a scan, not after the monitor gets scheduled.
        gate.set(
            mypaper_tweaks::game_activity::read()
                .is_some_and(|(snapshot, _)| snapshot.activity.pause_background),
        );
        std::thread::Builder::new()
            .name("index-game".into())
            .spawn(move || {
                let _watcher = watcher;
                loop {
                    let publisher = mypaper_tweaks::game_activity::read();
                    monitor.set(
                        publisher
                            .as_ref()
                            .is_some_and(|(snapshot, _)| snapshot.activity.pause_background),
                    );
                    let _ = tx.try_send(Change::Activity);
                    let mut handles = vec![HANDLE(event.0 as *mut _)];
                    if let Some((_, owner)) = &publisher {
                        handles.push(owner.handle());
                    }
                    // SAFETY: event and optional checked publisher handle outlive the blocking wait.
                    let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                    if result == windows::Win32::Foundation::WAIT_FAILED {
                        monitor.set(false);
                        let _ = tx.try_send(Change::Activity);
                        break;
                    }
                }
            })?;
        Ok(gate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_clock_and_blocked_worker_resume_together() {
        let gate = Arc::new(Quiet::default());
        gate.set(true);
        let at = gate.now();
        let worker = gate.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            worker.wait();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        assert_eq!(gate.now(), at);
        gate.set(false);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert!(gate.now() >= at);
    }
}
