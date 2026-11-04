//! WinEvent hooks on top-level windows: how the bars hear about new, closed, renamed, minimized
//! and focused windows, and about moves (a window going fullscreen). Nothing polls.
//!
//! Events arrive through the installing thread's message loop; the callback only sets bits that
//! the main loop takes with [`Hooks::take`].

use std::{
    cell::RefCell,
    collections::HashSet,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW,
    EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GA_PARENT, GetAncestor, GetDesktopWindow, GetForegroundWindow,
    GetShellWindow, GetWindowThreadProcessId, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
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

thread_local! {
    // WinEvent callbacks run on the installing thread. A dead child/control window must not
    // trigger an enumeration of every application; only known app windows affect that list.
    static WINDOWS_SEEN: RefCell<HashSet<isize>> = RefCell::default();
}
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
    location: Option<HWINEVENTHOOK>,
    foreground_pid: u32,
    explorer_location: Option<HWINEVENTHOOK>,
    explorer_pid: u32,
}

impl Hooks {
    pub fn install() -> Self {
        let ranges = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ];
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let hooks: Vec<_> = ranges
            .into_iter()
            // SAFETY: out-of-context hooks with a 'static callback, removed on drop.
            .map(|(min, max)| unsafe {
                SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags)
            })
            .filter(|hook| !hook.is_invalid())
            .collect();
        // Fullscreen detection only follows the foreground app. Other applications' animation
        // must not deliver location callbacks to this UI thread. Explorer still owns taskbar moves.
        PENDING.fetch_or(WINDOWS | FOREGROUND | MOVED, Ordering::Relaxed);
        let mut hooks = Self {
            hooks,
            location: None,
            foreground_pid: 0,
            explorer_location: None,
            explorer_pid: 0,
        };
        hooks.follow_foreground();
        hooks
    }

    /// The app snapshot identifies destroyed handles after Windows can no longer classify them.
    pub fn track(&mut self, windows: impl IntoIterator<Item = isize>) {
        WINDOWS_SEEN.with(|seen| *seen.borrow_mut() = windows.into_iter().collect());
        let mut explorer = 0;
        // SAFETY: read-only query; follows a new Explorer PID after its restart.
        unsafe { GetWindowThreadProcessId(GetShellWindow(), Some(&mut explorer)) };
        if explorer != self.explorer_pid {
            if let Some(hook) = self.explorer_location.take() {
                // SAFETY: this object owns the hook on this thread.
                let _ = unsafe { UnhookWinEvent(hook) };
            }
            self.explorer_location = location_hook(explorer);
            self.explorer_pid = explorer;
        }
    }

    /// What happened since the last call (a set of the constants above).
    pub fn take(&mut self) -> u32 {
        let bits = PENDING.swap(0, Ordering::Relaxed);
        if bits & FOREGROUND != 0 {
            self.follow_foreground();
        }
        bits
    }

    fn follow_foreground(&mut self) {
        let mut pid = 0;
        // SAFETY: read-only process-id query; a vanished window leaves zero.
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        if pid == self.foreground_pid {
            return;
        }
        if let Some(hook) = self.location.take() {
            // SAFETY: this object owns the hook, on its installing thread.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        self.location = location_hook(pid);
        self.foreground_pid = pid;
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        for hook in self
            .hooks
            .drain(..)
            .chain(self.location.take())
            .chain(self.explorer_location.take())
        {
            // SAFETY: a hook from SetWinEventHook.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

fn location_hook(pid: u32) -> Option<HWINEVENTHOOK> {
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    // SAFETY: out-of-context callback scoped to this process, unhooked by Hooks.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(on_event),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    (!hook.is_invalid()).then_some(hook)
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
        // For a dead window, `top` means it belonged to the last app snapshot.
        EVENT_OBJECT_DESTROY if top => WINDOWS,
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
    let top = if event == EVENT_OBJECT_DESTROY {
        WINDOWS_SEEN.with(|seen| seen.borrow_mut().remove(&(hwnd.0 as isize)))
    } else {
        event != EVENT_SYSTEM_FOREGROUND && top_level(hwnd)
    };
    let taskbar = || crate::taskbar::is_taskbar_class(&crate::apps::class_name(hwnd));
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
        assert_eq!(classify(EVENT_OBJECT_DESTROY, true, false, no), WINDOWS);
        assert_eq!(
            classify(EVENT_OBJECT_DESTROY, false, false, no),
            0,
            "destroyed controls/tooltips outside the app snapshot don't rebuild the window list"
        );
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
