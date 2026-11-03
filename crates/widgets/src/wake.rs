//! How worker threads (file watchers, plugin pipes, media, launches) reach
//! the main thread: a channel, plus a thread message that wakes its loop.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde_json::Value;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

use crate::bridge::{Reply, Target};

/// Posted to the main thread when [`Msg`]s are waiting.
pub const WM_WAKE: u32 = WM_APP + 10;

pub enum Msg {
    /// `config.json` changed (pause settings, theme).
    Config,
    /// `widgets.json` changed.
    Layout,
    /// `palette.json` changed.
    Palette,
    /// `theme.json` or a style sheet in the active theme's folder changed.
    Theme,
    /// Files changed in these widget folders.
    Widgets(Vec<String>),
    /// The shell was rebuilt (`widgets/shell/dist`).
    Shell,
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
    /// A launch finished.
    Launched(Target, Reply),
    /// A file watcher failed and stopped (logged).
    WatchError(PathBuf, String),
}

#[derive(Clone)]
pub struct Waker {
    tx: Sender<Msg>,
    thread: u32,
}

impl Waker {
    pub fn new(tx: Sender<Msg>, thread: u32) -> Self {
        Self { tx, thread }
    }

    pub fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            // SAFETY: posting to a thread id is always safe; failure means it is gone.
            let _ = unsafe { PostThreadMessageW(self.thread, WM_WAKE, WPARAM(0), LPARAM(0)) };
        }
    }
}
