//! How worker threads (file watchers, the icon, media, audio, network and tray workers) reach the
//! main thread: a channel, plus a thread message that wakes its loop.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

use crate::icons::{Described, Source};
use crate::media::MediaInfo;
use crate::tray::TrayItem;

/// Posted to the main thread when [`Msg`]s are waiting.
pub const WM_WAKE: u32 = WM_APP + 10;

pub enum Msg {
    /// `bars.json` or `config.json` changed.
    Config,
    /// The theme's files or the palette changed.
    Theme,
    /// `bars/shell/dist` was rebuilt.
    Page,
    /// The icon worker described an app, a shortcut or a file.
    Described(Source, Described),
    /// What's playing changed (SMTC).
    Media(Option<MediaInfo>),
    /// The output volume, or the default output device, changed.
    Volume,
    /// A network interface or the Wi-Fi connection changed.
    Network,
    /// The notification-area icons (the tray host's, or UI Automation's).
    Tray(Vec<TrayItem>),
    /// A flyout provider finished something (docs/bars.md, "Flyouts").
    Flyout(crate::flyouts::Done),
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
