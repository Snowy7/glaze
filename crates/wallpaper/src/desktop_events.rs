//! Explorer creates and destroys child windows even while the desktop is unchanged. Its
//! out-of-context WinEvent callbacks wake the thread that installed them, before filtering.
//! Keep that traffic off the frame loop; forward only desktop loss and recovery signals.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use glaze_desktop::{DesktopEvents as Observer, DesktopHost};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_NOREMOVE,
    PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT, WM_NULL, WM_QUIT,
};

#[derive(Default)]
struct Signals {
    lost: AtomicBool,
    ready: AtomicBool,
    forget: AtomicBool,
}

impl Signals {
    /// Retain each lifecycle signal until the consumer takes it; unrelated Explorer traffic
    /// must neither clear an outstanding signal nor wake the consumer.
    fn publish(&self, lost: bool, ready: bool) -> bool {
        if lost {
            self.lost.store(true, Ordering::Release);
        }
        if ready {
            self.ready.store(true, Ordering::Release);
        }
        lost || ready
    }
}

pub struct DesktopEvents {
    signals: Arc<Signals>,
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
}

impl DesktopEvents {
    pub fn new(host: DesktopHost) -> Result<Self> {
        // HWNDs are process-independent identifiers, passed as integers across the thread
        // boundary. The observer only queries them; it owns neither Explorer window.
        let (layered, a, b) = match host {
            DesktopHost::Layered { progman, defview } => {
                (true, progman.0 as usize, defview.0 as usize)
            }
            DesktopHost::Legacy { workerw, icons } => (false, workerw.0 as usize, icons.0 as usize),
        };
        // SAFETY: a read of this consumer thread's id.
        let consumer = unsafe { GetCurrentThreadId() };
        let signals = Arc::new(Signals::default());
        let shared = signals.clone();
        let (started, startup) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("desktop-events".into())
            .spawn(move || {
                let a = HWND(a as *mut _);
                let b = HWND(b as *mut _);
                let host = if layered {
                    DesktopHost::Layered {
                        progman: a,
                        defview: b,
                    }
                } else {
                    DesktopHost::Legacy {
                        workerw: a,
                        icons: b,
                    }
                };
                let observer = match Observer::new(host) {
                    Ok(observer) => observer,
                    Err(error) => {
                        let _ = started.send(Err(format!("{error:#}")));
                        return;
                    }
                };
                let mut msg = MSG::default();
                // SAFETY: establish this worker's message queue before publishing its id.
                let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
                // SAFETY: a read of this worker's id.
                let _ = started.send(Ok(unsafe { GetCurrentThreadId() }));
                loop {
                    // SAFETY: drain only this worker's queue; the observer's window and
                    // out-of-context hooks were created here and are dropped here too.
                    unsafe {
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            if msg.message == WM_QUIT {
                                return;
                            }
                            DispatchMessageW(&msg);
                        }
                    }
                    if shared.forget.swap(false, Ordering::AcqRel) {
                        observer.set_host(None);
                    }
                    let lost = observer.take_lost();
                    let ready = observer.take_ready();
                    if shared.publish(lost, ready) {
                        post(consumer, WM_NULL);
                    }
                    // SAFETY: sleep indefinitely until this worker's next hook/window message.
                    unsafe {
                        MsgWaitForMultipleObjectsEx(
                            None,
                            INFINITE,
                            QS_ALLINPUT,
                            MWMO_INPUTAVAILABLE,
                        );
                    }
                }
            })
            .context("starting desktop observer")?;
        let thread_id = startup
            .recv()
            .context("desktop observer startup")?
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            signals,
            thread: Some(thread),
            thread_id,
        })
    }

    pub fn take_lost(&self) -> bool {
        self.signals.lost.swap(false, Ordering::AcqRel)
    }

    pub fn take_ready(&self) -> bool {
        self.signals.ready.swap(false, Ordering::AcqRel)
    }

    pub fn forget_host(&self) {
        self.signals.forget.store(true, Ordering::Release);
        post(self.thread_id, WM_NULL);
    }
}

impl Drop for DesktopEvents {
    fn drop(&mut self) {
        post(self.thread_id, WM_QUIT);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn post(thread: u32, message: u32) {
    // SAFETY: posting a thread message is safe; a gone recipient simply rejects it.
    let _ = unsafe { PostThreadMessageW(thread, message, WPARAM(0), LPARAM(0)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_noise_is_quiet_and_cannot_erase_pending_lifecycle_signals() {
        let signals = Signals::default();
        assert!(!signals.publish(false, false));
        assert!(signals.publish(true, false));
        assert!(!signals.publish(false, false));
        assert!(signals.publish(false, true));
        assert!(signals.lost.swap(false, Ordering::AcqRel));
        assert!(signals.ready.swap(false, Ordering::AcqRel));
        assert!(!signals.lost.load(Ordering::Acquire));
        assert!(!signals.ready.load(Ordering::Acquire));
    }

    #[test]
    fn observer_owns_its_hook_thread_and_stops_with_a_queued_reset() {
        // Null host identifiers exercise observer startup/shutdown without touching Explorer.
        // The observer creates only its own hidden, no-activate notification window.
        let observer = DesktopEvents::new(DesktopHost::Layered {
            progman: HWND::default(),
            defview: HWND::default(),
        })
        .unwrap();
        assert_ne!(
            observer.thread.as_ref().unwrap().thread().id(),
            std::thread::current().id()
        );
        observer.forget_host();
        drop(observer);
    }
}
