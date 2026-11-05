//! Coverage follows app lifecycle and foreground/Explorer movement, rather than every animated
//! child HWND on the desktop. Destroyed handles are classified from the last live top-level set.
use std::{
    cell::RefCell,
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::*,
    },
};

static CHANGED: AtomicBool = AtomicBool::new(true);
thread_local! {static TOP:RefCell<HashSet<isize>>=RefCell::new(HashSet::new());}

pub struct WindowEvents {
    hooks: Vec<HWINEVENTHOOK>,
    locations: Vec<HWINEVENTHOOK>,
    processes: Vec<u32>,
}
impl WindowEvents {
    pub fn install() -> Self {
        TOP.with_borrow_mut(HashSet::clear);
        // SAFETY: synchronous enumeration records only HWND identities; no window is changed.
        unsafe {
            let _ = EnumWindows(Some(remember), LPARAM(0));
        }
        let hooks = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ]
        .into_iter()
        .filter_map(|(first, last)| hook(first, last, 0))
        .collect();
        CHANGED.store(true, Ordering::Relaxed);
        Self {
            hooks,
            locations: Vec::new(),
            processes: Vec::new(),
        }
    }
    pub fn take_changed(&mut self) -> bool {
        let (mut foreground, mut explorer) = (0, 0);
        // SAFETY: read-only process IDs, used to scope callbacks on this UI thread.
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut foreground));
            GetWindowThreadProcessId(GetShellWindow(), Some(&mut explorer));
        }
        let mut processes: Vec<_> = [foreground, explorer]
            .into_iter()
            .filter(|&pid| pid != 0 && pid != std::process::id())
            .collect();
        processes.sort_unstable();
        processes.dedup();
        if processes != self.processes {
            unhook(&mut self.locations);
            self.locations = processes
                .iter()
                .filter_map(|&pid| {
                    hook(
                        EVENT_OBJECT_LOCATIONCHANGE,
                        EVENT_OBJECT_LOCATIONCHANGE,
                        pid,
                    )
                })
                .collect();
            self.processes = processes;
            CHANGED.store(true, Ordering::Relaxed);
        }
        CHANGED.swap(false, Ordering::Relaxed)
    }
}
impl Drop for WindowEvents {
    fn drop(&mut self) {
        unhook(&mut self.hooks);
        unhook(&mut self.locations);
    }
}
fn hook(first: u32, last: u32, pid: u32) -> Option<HWINEVENTHOOK> {
    // SAFETY: static out-of-context callback, owned and removed on its installing thread.
    let hook = unsafe {
        SetWinEventHook(
            first,
            last,
            None,
            Some(changed),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    (!hook.is_invalid()).then_some(hook)
}
fn unhook(hooks: &mut Vec<HWINEVENTHOOK>) {
    for hook in hooks.drain(..) {
        // SAFETY: this object owns each successfully registered hook.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}
fn eligible(hwnd: HWND) -> bool {
    // SAFETY: read-only queries; a destroyed or child handle is not a covering app window.
    unsafe {
        GetAncestor(hwnd, GA_PARENT) == GetDesktopWindow()
            && GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32
                & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0)
                == 0
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
    let relevant = if event == EVENT_OBJECT_DESTROY {
        TOP.with_borrow_mut(|top| top.remove(&(hwnd.0 as isize)))
    } else if eligible(hwnd) {
        TOP.with_borrow_mut(|top| top.insert(hwnd.0 as isize));
        true
    } else {
        false
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
            changed(
                HWINEVENTHOOK::default(),
                EVENT_OBJECT_CREATE,
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
