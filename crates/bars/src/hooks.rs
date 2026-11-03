//! WinEvent hooks on top-level windows: how the bars hear about new, closed, renamed, minimized
//! and focused windows, and about moves (a window going fullscreen). Nothing polls.
//!
//! Events arrive through the installing thread's message loop; the callback only sets bits that
//! the main loop takes with [`Hooks::take`].

use std::sync::atomic::{AtomicU32, Ordering};

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
/// A taskbar window was shown (Explorer re-showing a taskbar we hid).
pub const TASKBAR_SHOWN: u32 = 8;

static PENDING: AtomicU32 = AtomicU32::new(0);

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

/// Which bits an event sets, if any.
fn classify(event: u32, top: bool, taskbar: impl FnOnce() -> bool) -> u32 {
    match event {
        EVENT_SYSTEM_FOREGROUND => FOREGROUND,
        // A destroyed window can't be asked whether it was top-level any more.
        EVENT_OBJECT_DESTROY => WINDOWS,
        EVENT_OBJECT_LOCATIONCHANGE if top => MOVED,
        EVENT_OBJECT_SHOW if top && taskbar() => WINDOWS | TASKBAR_SHOWN,
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
    let bits = classify(event, top, taskbar);
    if bits != 0 {
        PENDING.fetch_or(bits, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_map_to_what_needs_redoing() {
        let no = || false;
        assert_eq!(classify(EVENT_SYSTEM_FOREGROUND, false, no), FOREGROUND);
        assert_eq!(classify(EVENT_OBJECT_DESTROY, false, no), WINDOWS);
        assert_eq!(classify(EVENT_OBJECT_NAMECHANGE, true, no), WINDOWS);
        assert_eq!(
            classify(EVENT_OBJECT_NAMECHANGE, false, no),
            0,
            "a child control renamed"
        );
        assert_eq!(classify(EVENT_OBJECT_LOCATIONCHANGE, true, no), MOVED);
        assert_eq!(classify(EVENT_OBJECT_LOCATIONCHANGE, false, no), 0);
        assert_eq!(
            classify(EVENT_OBJECT_SHOW, true, || true),
            WINDOWS | TASKBAR_SHOWN
        );
        assert_eq!(classify(EVENT_OBJECT_SHOW, true, no), WINDOWS);
    }
}
