//! Coverage follows app lifecycle and foreground/Explorer movement, rather than every animated
//! child HWND on the desktop. Destroyed handles are classified from the last live top-level set.
use std::{
    cell::RefCell,
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{Accessibility::HWINEVENTHOOK, WindowsAndMessaging::*},
};

use mypaper_desktop::win_events::Subscription;

static CHANGED: AtomicBool = AtomicBool::new(true);
thread_local! {static TOP:RefCell<HashSet<isize>>=RefCell::new(HashSet::new());}

pub struct WindowEvents {
    _subscription: Subscription,
}
impl WindowEvents {
    pub fn install() -> Self {
        TOP.with_borrow_mut(HashSet::clear);
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
                // SAFETY: the shared source calls us on the shell UI thread with whole windows.
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
        TOP.with_borrow_mut(|top| top.insert(hwnd.0 as isize));
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
    if event == EVENT_SYSTEM_FOREGROUND {
        CHANGED.store(true, Ordering::Relaxed);
    }
    let relevant = if event == EVENT_OBJECT_DESTROY {
        TOP.with_borrow_mut(|top| top.remove(&(hwnd.0 as isize)))
    } else {
        let visible = eligible(hwnd);
        TOP.with_borrow_mut(|top| {
            let known = top.remove(&(hwnd.0 as isize));
            if visible {
                top.insert(hwnd.0 as isize);
            }
            visible || known
        })
    };
    if relevant {
        CHANGED.store(true, Ordering::Relaxed);
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
            TOP.with_borrow_mut(|top| {
                top.clear();
            });
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
