//! Core's activity publication wakes the existing shell loop. The publisher process is also
//! in its wait set, so a crash resumes consumers even without a final file notification.
use mypaper_tweaks::game_activity::{self, Owner};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::WaitForSingleObject;

pub const WM_GAME_ACTIVITY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;

pub struct GameControl {
    watch: Option<(notify::RecommendedWatcher, Arc<AtomicBool>)>,
    enabled: bool,
    owner: Option<Owner>,
    pub active: bool,
}

impl GameControl {
    pub fn new() -> Self {
        Self {
            watch: None,
            enabled: false,
            owner: None,
            active: false,
        }
    }

    /// Follows the shell's existing config watcher. Explicit off releases our directory
    /// notification worker and process handle too; no separate config watcher is needed.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.watch = None;
        self.owner = None;
        self.active = false;
        if !enabled {
            return;
        }
        let dirty = Arc::new(AtomicBool::new(true));
        let changed = dirty.clone();
        let watch = game_activity::watch(move || {
            if !changed.swap(true, Ordering::AcqRel) {
                crate::inbox::post(WM_GAME_ACTIVITY, 0);
            }
        })
        .inspect_err(|e| log::warn!("game activity watcher: {e:#}"))
        .ok();
        self.watch = watch.map(|watch| (watch, dirty));
    }

    pub fn handle(&self) -> Option<HANDLE> {
        self.owner.as_ref().map(Owner::handle)
    }

    pub fn refresh(&mut self) {
        // SAFETY: the Owner keeps this query-only process handle alive.
        let exited = self
            .handle()
            .is_some_and(|h| unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0);
        let dirty = self
            .watch
            .as_ref()
            .is_some_and(|(_, dirty)| dirty.swap(false, Ordering::AcqRel));
        if !dirty && !exited {
            return;
        }
        let current = game_activity::read();
        self.active = current
            .as_ref()
            .is_some_and(|(s, _)| s.activity.pause_background);
        self.owner = current.map(|(_, owner)| owner);
    }
}
