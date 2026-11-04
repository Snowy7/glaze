//! How worker threads (file watchers, plugin pipes, media, launches) reach
//! the main thread: a channel, plus a thread message that wakes its loop.

use std::sync::mpsc::Sender;

use serde_json::Value;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, PostThreadMessageW, WM_APP};

use crate::bridge::{Reply, Target};

/// Posted to the main thread when [`Msg`]s are waiting.
pub const WM_WAKE: u32 = WM_APP + 10;

pub enum Msg {
    /// A line from a plugin's stdout, or its exit (`None`).
    Plugin {
        id: String,
        generation: u64,
        line: Option<String>,
    },
    /// The media worker answered a request (`token`) or saw a change (`None`).
    Media {
        token: Option<u64>,
        result: Result<Value, String>,
    },
    /// A worker thread finished a call (a launch, `user.info`, `appIcon`).
    Answered(Target, Reply),
    /// The audio capture thread made a spectrum frame.
    Audio {
        generation: u64,
        frame: Box<crate::audio::Frame>,
    },
    /// A file dialog finished.
    Picked(crate::pick::Done),
}

#[derive(Clone)]
pub struct Waker {
    tx: Sender<Msg>,
    thread: u32,
    window: Option<(isize, u32)>,
}

impl Waker {
    pub fn new(tx: Sender<Msg>, thread: u32) -> Self {
        Self {
            tx,
            thread,
            window: None,
        }
    }

    /// Prefer a window message in a COM UI loop: COM may dispatch and discard thread messages.
    pub fn for_window(tx: Sender<Msg>, window: HWND, message: u32) -> Self {
        Self {
            tx,
            thread: 0,
            window: Some((window.0 as isize, message)),
        }
    }

    pub fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            // SAFETY: posting to a thread id is always safe; failure means it is gone.
            let _ = unsafe {
                if let Some((window, message)) = self.window {
                    PostMessageW(Some(HWND(window as *mut _)), message, WPARAM(0), LPARAM(0))
                } else {
                    PostThreadMessageW(self.thread, WM_WAKE, WPARAM(0), LPARAM(0))
                }
            };
        }
    }
}
