//! How worker threads (file watchers, the icon, media, audio, network and tray workers) reach the
//! main thread: a channel, plus a thread message that wakes its loop.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_QUIT,
};

use crate::custom::Output;
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
    /// The wallpaper engine wrote a new snapshot (wallpaper glass).
    Backdrop,
    /// A custom module's command finished.
    Custom(String, Output),
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

/// Gives the calling thread its message queue, so posts to it land from now on. A worker with a
/// message loop calls it before it tells its starter it's ready.
pub fn make_queue() {
    let mut msg = MSG::default();
    // SAFETY: a peek that removes nothing; Windows creates the queue on this first call.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
}

/// Asks a worker's message loop to quit and waits for the thread. The post is retried while it
/// fails (a thread without a queue yet) and the thread still runs, so the quit is never lost.
pub fn quit_thread(thread: u32, handle: JoinHandle<()>) {
    // SAFETY: posting to a thread we started.
    while unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_err()
        && !handle.is_finished()
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = handle.join();
}
