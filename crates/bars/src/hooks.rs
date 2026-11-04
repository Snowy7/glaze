//! WinEvent hooks on top-level windows: how the bars hear about new, closed, renamed, minimized
//! and focused windows, and about moves (a window going fullscreen). Nothing polls.
//!
//! Events arrive through the installing thread's message loop; the callback only sets bits that
//! the main loop takes with [`Hooks::take`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW,
    EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GA_PARENT, GetAncestor, GetDesktopWindow, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

/// The window list may have changed (created, destroyed, shown, hidden, cloaked, renamed,
/// minimized or restored).
pub const WINDOWS: u32 = 1;
/// Another window came to the front.
pub const FOREGROUND: u32 = 2;
/// A top-level window moved or resized.
pub const MOVED: u32 = 4;
/// A taskbar window was created, shown or moved (Explorer re-showing, or sliding in, a taskbar we
/// hid, or building a new one).
pub const TASKBAR_SHOWN: u32 = 8;

static PENDING: AtomicU32 = AtomicU32::new(0);
/// Whether taskbar moves count as [`TASKBAR_SHOWN`]: only while the taskbar should be hidden, so
/// other moves cost no class lookup.
static WATCH_TASKBAR: AtomicBool = AtomicBool::new(false);

/// Watch the taskbar windows' moves too (`taskbar.mode` is `hide`).
pub fn watch_taskbar(on: bool) {
    WATCH_TASKBAR.store(on, Ordering::Relaxed);
}

pub struct Hooks {
    hooks: Vec<HWINEVENTHOOK>,
}

impl Hooks {
    pub fn install() -> Self {
        let ranges = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ];
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let hooks = ranges
            .into_iter()
            // SAFETY: out-of-context hooks with a 'static callback, removed on drop.
            .map(|(min, max)| unsafe {
                SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags)
            })
            .filter(|hook| !hook.is_invalid())
            .collect();
        PENDING.fetch_or(WINDOWS | FOREGROUND | MOVED, Ordering::Relaxed);
        Self { hooks }
    }

    /// What happened since the last call (a set of the constants above).
    pub fn take(&self) -> u32 {
        PENDING.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: a hook from SetWinEventHook.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

fn top_level(hwnd: HWND) -> bool {
    // SAFETY: read-only queries; a stale handle just isn't top-level.
    unsafe { GetAncestor(hwnd, GA_PARENT) == GetDesktopWindow() }
}

/// Which bits an event sets, if any. `taskbar` says whether the window is a taskbar; it's only
/// asked when the answer matters.
fn classify(event: u32, top: bool, watching: bool, taskbar: impl Fn() -> bool) -> u32 {
    match event {
        EVENT_SYSTEM_FOREGROUND => FOREGROUND,
        // A destroyed window can't be asked whether it was top-level any more.
        EVENT_OBJECT_DESTROY => WINDOWS,
        EVENT_OBJECT_LOCATIONCHANGE if top && watching && taskbar() => MOVED | TASKBAR_SHOWN,
        EVENT_OBJECT_LOCATIONCHANGE if top => MOVED,
        // A new taskbar (a monitor came back, Explorer restarted) is born visible and unveiled.
        EVENT_OBJECT_SHOW | EVENT_OBJECT_CREATE if top && taskbar() => WINDOWS | TASKBAR_SHOWN,
        EVENT_OBJECT_CREATE
        | EVENT_OBJECT_SHOW
        | EVENT_OBJECT_HIDE
        | EVENT_OBJECT_NAMECHANGE
        | EVENT_OBJECT_CLOAKED
        | EVENT_OBJECT_UNCLOAKED
        | EVENT_SYSTEM_MINIMIZESTART
        | EVENT_SYSTEM_MINIMIZEEND
            if top =>
        {
            WINDOWS
        }
        _ => 0,
    }
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // Carets, cursors and other sub-objects fire constantly; only whole windows matter.
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let top = event != EVENT_OBJECT_DESTROY && event != EVENT_SYSTEM_FOREGROUND && top_level(hwnd);
    let taskbar = || mypaper_bars::taskbar::is_taskbar_class(&crate::apps::class_name(hwnd));
    let watching = WATCH_TASKBAR.load(Ordering::Relaxed);
    let bits = classify(event, top, watching, taskbar);
    if bits != 0 {
        PENDING.fetch_or(bits, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_map_to_what_needs_redoing() {
        let (no, yes) = (|| false, || true);
        assert_eq!(
            classify(EVENT_SYSTEM_FOREGROUND, false, false, no),
            FOREGROUND
        );
        assert_eq!(classify(EVENT_OBJECT_DESTROY, false, false, no), WINDOWS);
        assert_eq!(classify(EVENT_OBJECT_NAMECHANGE, true, false, no), WINDOWS);
        assert_eq!(
            classify(EVENT_OBJECT_NAMECHANGE, false, false, no),
            0,
            "a child control renamed"
        );
        assert_eq!(
            classify(EVENT_OBJECT_LOCATIONCHANGE, true, false, no),
            MOVED
        );
        assert_eq!(classify(EVENT_OBJECT_LOCATIONCHANGE, false, false, no), 0);
        assert_eq!(
            classify(EVENT_OBJECT_SHOW, true, false, yes),
            WINDOWS | TASKBAR_SHOWN,
            "a taskbar shown counts even when not watching its moves"
        );
        assert_eq!(classify(EVENT_OBJECT_SHOW, true, false, no), WINDOWS);
        assert_eq!(
            classify(EVENT_OBJECT_CREATE, true, false, yes),
            WINDOWS | TASKBAR_SHOWN,
            "a taskbar built anew must be veiled before it's seen"
        );
        assert_eq!(classify(EVENT_OBJECT_CREATE, true, false, no), WINDOWS);
    }

    #[test]
    fn taskbar_moves_count_only_while_watched() {
        let (no, yes) = (|| false, || true);
        assert_eq!(
            classify(EVENT_OBJECT_LOCATIONCHANGE, true, true, yes),
            MOVED | TASKBAR_SHOWN,
            "an auto-hidden taskbar sliding in"
        );
        assert_eq!(classify(EVENT_OBJECT_LOCATIONCHANGE, true, true, no), MOVED);
        let asked = std::cell::Cell::new(false);
        classify(EVENT_OBJECT_LOCATIONCHANGE, true, false, || {
            asked.set(true);
            true
        });
        assert!(!asked.get(), "no class lookup per move while not watching");
    }
}
