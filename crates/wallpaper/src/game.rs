//! Core's game signal (docs/system.md, "Games"), followed while `engine.during_games` is `pause`.
//! A directory notification wakes the main loop (no timer), and the publisher's process handle is
//! in the loop's wait set, so a core that dies mid-game resumes the wallpaper too. Off, nothing
//! is watched or held.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mypaper_tweaks::game_activity::{self, Owner};
use windows::Win32::Foundation::{HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{GetCurrentThreadId, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_NULL};

#[derive(Default)]
pub struct GameSignal {
    /// The folder watch, and whether the signal changed since it was last read.
    watch: Option<(mypaper_fswatch::Watch, Arc<AtomicBool>)>,
    /// The signal's publisher (core), while it publishes.
    owner: Option<Owner>,
    active: bool,
}

impl GameSignal {
    /// Starts following the signal (`on`), waking this thread when it changes, or lets go of the
    /// watch and the publisher's handle.
    pub fn set_enabled(&mut self, on: bool) {
        if on == self.watch.is_some() {
            return;
        }
        self.watch = None;
        self.owner = None;
        self.active = false;
        if !on {
            return;
        }
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let dirty = Arc::new(AtomicBool::new(true));
        let changed = dirty.clone();
        self.watch = game_activity::watch(move || {
            if !changed.swap(true, Ordering::AcqRel) {
                // SAFETY: a no-op thread message that wakes the main loop; a gone thread fails it.
                let _ = unsafe { PostThreadMessageW(thread, WM_NULL, WPARAM(0), LPARAM(0)) };
            }
        })
        .inspect_err(|e| log::warn!("not following the game signal: {e:#}"))
        .ok()
        .map(|watch| (watch, dirty));
    }

    /// The publisher's process, for the main loop's wait set (it ending means no game).
    pub fn handle(&self) -> Option<HANDLE> {
        self.owner.as_ref().map(Owner::handle)
    }

    /// Whether a game is on. Reads the signal only after a notification or the publisher's exit.
    pub fn refresh(&mut self) -> bool {
        // SAFETY: the Owner keeps this query-only process handle alive.
        let exited = self
            .handle()
            .is_some_and(|h| unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0);
        let dirty = self
            .watch
            .as_ref()
            .is_some_and(|(_, dirty)| dirty.swap(false, Ordering::AcqRel));
        if dirty || exited {
            // Missing, corrupt or dead publisher: no game.
            let current = game_activity::read();
            self.active = current
                .as_ref()
                .is_some_and(|(snapshot, _)| snapshot.activity.pause_background);
            self.owner = current.map(|(_, owner)| owner);
        }
        self.active
    }
}
