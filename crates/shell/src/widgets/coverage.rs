//! Coverage follows app lifecycle and foreground/Explorer movement, rather than every animated
//! child HWND on the desktop. Destroyed handles are classified from the last live top-level set.
use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{Accessibility::HWINEVENTHOOK, WindowsAndMessaging::*},
};

use mypaper_desktop::win_events::{self, Subscription};

static CHANGED: AtomicBool = AtomicBool::new(true);
/// The visible app windows (filled on install, kept by the events on the WinEvent thread).
static TOP: Mutex<Option<HashSet<isize>>> = Mutex::new(None);

/// Runs `f` on the known top-level windows.
fn with_top<R>(f: impl FnOnce(&mut HashSet<isize>) -> R) -> R {
    f(TOP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(HashSet::new))
}

/// Records a change and wakes the shell loop after the event batch.
fn mark_changed() {
    CHANGED.store(true, Ordering::Relaxed);
    win_events::notify();
}

pub struct WindowEvents {
    _subscription: Subscription,
}
impl WindowEvents {
    pub fn install() -> Self {
        with_top(HashSet::clear);
        // SAFETY: synchronous enumeration records only HWND identities; no window is changed.
        unsafe {
            let _ = EnumWindows(Some(remember), LPARAM(0));
        }
        CHANGED.store(true, Ordering::Relaxed);
        Self {
            _subscription: Subscription::new(|event| {
                if event.event == EVENT_OBJECT_NAMECHANGE {
                    return;
                }
                // SAFETY: the shared source calls us with whole windows; `changed` only reads them.
                unsafe {
                    changed(
                        HWINEVENTHOOK::default(),
                        event.event,
                        HWND(event.window as _),
                        OBJID_WINDOW.0,
                        CHILDID_SELF as i32,
                        0,
                        0,
                    )
                };
            }),
        }
    }
    pub fn take_changed(&mut self) -> bool {
        CHANGED.swap(false, Ordering::Relaxed)
    }
}
fn eligible(hwnd: HWND) -> bool {
    // SAFETY: read-only queries; a destroyed or child handle is not a covering app window.
    unsafe {
        GetAncestor(hwnd, GA_PARENT) == GetDesktopWindow()
            && GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32
                & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0)
                == 0
            && IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
    }
}
unsafe extern "system" fn remember(hwnd: HWND, _: LPARAM) -> windows::core::BOOL {
    if eligible(hwnd) {
        with_top(|top| top.insert(hwnd.0 as isize));
    }
    true.into()
}
unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let relevant = if event == EVENT_OBJECT_DESTROY {
        with_top(|top| top.remove(&(hwnd.0 as isize)))
    } else {
        let visible = eligible(hwnd);
        with_top(|top| {
            let known = top.remove(&(hwnd.0 as isize));
            if visible {
                top.insert(hwnd.0 as isize);
            }
            visible || known
        })
    };
    if relevant || event == EVENT_SYSTEM_FOREGROUND {
        mark_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    #[test]
    fn child_destruction_does_not_invalidate_coverage_but_a_known_app_does() {
        // SAFETY: hidden test-owned windows; each is destroyed on this thread.
        unsafe {
            let parent = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Coverage probe"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let child = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Child"),
                WS_CHILD,
                0,
                0,
                10,
                10,
                Some(parent),
                None,
                None,
                None,
            )
            .unwrap();
            with_top(HashSet::clear);
            CHANGED.store(false, Ordering::Relaxed);
            changed(
                HWINEVENTHOOK::default(),
                EVENT_OBJECT_CREATE,
                parent,
                OBJID_WINDOW.0,
                0,
                0,
                0,
            );
            assert!(
                !CHANGED.load(Ordering::Relaxed),
                "hidden app/console creation cannot cover the desktop"
            );
            let _ = ShowWindow(parent, SW_SHOWNOACTIVATE);
            changed(
                HWINEVENTHOOK::default(),
                EVENT_OBJECT_SHOW,
                parent,
                OBJID_WINDOW.0,
                0,
                0,
                0,
            );
            CHANGED.store(false, Ordering::Relaxed);
            DestroyWindow(child).unwrap();
            changed(
                HWINEVENTHOOK::default(),
                EVENT_OBJECT_DESTROY,
                child,
                OBJID_WINDOW.0,
                0,
                0,
                0,
            );
            assert!(!CHANGED.load(Ordering::Relaxed));
            DestroyWindow(parent).unwrap();
            changed(
                HWINEVENTHOOK::default(),
                EVENT_OBJECT_DESTROY,
                parent,
                OBJID_WINDOW.0,
                0,
                0,
                0,
            );
            assert!(CHANGED.load(Ordering::Relaxed));
        }
    }
}
