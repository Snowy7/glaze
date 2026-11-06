//! Desktop lifetime events, including cross-process parents that vanish without
//! delivering `WM_DESTROY` to our child windows. Keep alive even with no visible layers.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;

use anyhow::{Context, Result, ensure};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_CREATE,
    EVENT_OBJECT_DESTROY, EVENT_OBJECT_SHOW, GWLP_USERDATA, GetClassNameW, GetWindowLongPtrW,
    GetWindowThreadProcessId, OBJID_WINDOW, RegisterClassExW, RegisterWindowMessageW, SetWindowLongPtrW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::DesktopHost;

const CLASS: PCWSTR = w!("mypaper.desktop.events");
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

thread_local! {
    // Out-of-context callbacks run on the thread that installed the hook.
    static WATCHES: RefCell<HashMap<isize, Weak<Signals>>> = RefCell::default();
}

struct Signals {
    host: Cell<Option<DesktopHost>>,
    lost: Cell<bool>,
    ready: Cell<bool>,
}

/// Watches desktop destruction and replacement shell creation/broadcasts.
/// Create and pump messages on the owning thread. No polling is used.
///
/// While a desktop is known, the hook takes only its Explorer's events (the window lifecycle of
/// every other process used to wake the owning thread: ~1-3 a second on a quiet desktop, more
/// while apps open and close windows). Once it is lost, the hook takes every process's again,
/// so the replacement shell's creation is seen.
pub struct DesktopEvents {
    window: HWND,
    hook: Cell<HWINEVENTHOOK>,
    /// The process the hook is limited to (0: every process).
    pid: Cell<u32>,
    signals: Rc<Signals>,
}

/// Hooks window creation, destruction and showing in process `pid` (0: every process).
fn hook(pid: u32) -> HWINEVENTHOOK {
    // SAFETY: static callback delivered to this thread; unhooked before its signals drop.
    unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(on_event),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }
}

/// The Explorer process that owns `host` (0, every process, when there is none).
fn explorer_pid(host: Option<DesktopHost>) -> u32 {
    let mut pid = 0;
    if let Some(host) = host {
        // SAFETY: a read-only query; a dead window leaves `pid` at 0.
        unsafe { GetWindowThreadProcessId(host.parent(), Some(&mut pid)) };
    }
    pid
}

impl DesktopEvents {
    pub fn new(host: DesktopHost) -> Result<Self> {
        let signals = Rc::new(Signals {
            host: Cell::new(Some(host)),
            lost: Cell::new(false),
            ready: Cell::new(false),
        });
        // SAFETY: static class/procedure; this top-level window stays hidden and never activates.
        let window = unsafe {
            let message =
                *TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(w!("TaskbarCreated")));
            ensure!(message != 0, "registering TaskbarCreated");
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class); // Another watcher may have registered it already.
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("mypaper desktop events"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .context("creating desktop lifetime watcher")?;
            SetWindowLongPtrW(window, GWLP_USERDATA, Rc::as_ptr(&signals) as isize);
            window
        };
        let pid = explorer_pid(Some(host));
        let hook = hook(pid);
        if hook.is_invalid() {
            // SAFETY: our window; its signals are still alive.
            let _ = unsafe { DestroyWindow(window) };
            anyhow::bail!("watching desktop window destruction");
        }
        WATCHES.with_borrow_mut(|watches| {
            watches.insert(hook.0 as isize, Rc::downgrade(&signals));
        });
        Ok(Self {
            window,
            hook: Cell::new(hook),
            pid: Cell::new(pid),
            signals,
        })
    }

    pub fn take_lost(&self) -> bool {
        let lost = self.signals.lost.replace(false);
        if lost {
            // Its Explorer is gone: watch every process for the next one.
            self.rescope();
        }
        lost
    }
    pub fn take_ready(&self) -> bool {
        self.signals.ready.replace(false)
    }

    /// Watches a resolved desktop, or shell creation while waiting for one.
    pub fn set_host(&self, host: Option<DesktopHost>) {
        self.signals.host.set(host);
        self.rescope();
    }

    /// Limits the hook to the current desktop's Explorer, or every process without one. Called
    /// outside the hook's callback (a hook is not replaced from inside its own callback).
    fn rescope(&self) {
        let pid = explorer_pid(self.signals.host.get());
        if pid == self.pid.get() {
            return;
        }
        let new = hook(pid);
        if new.is_invalid() {
            return; // keep the wider hook
        }
        let old = self.hook.replace(new);
        self.pid.set(pid);
        WATCHES.with_borrow_mut(|watches| {
            if let Some(signals) = watches.remove(&(old.0 as isize)) {
                watches.insert(new.0 as isize, signals);
            }
        });
        // SAFETY: our previous hook, on its creating thread.
        let _ = unsafe { UnhookWinEvent(old) };
    }
}

impl Drop for DesktopEvents {
    fn drop(&mut self) {
        let hook = self.hook.get();
        WATCHES.with_borrow_mut(|watches| {
            watches.remove(&(hook.0 as isize));
        });
        // SAFETY: our hook and window, released on their creating thread, before signals.
        unsafe {
            let _ = UnhookWinEvent(hook);
            let _ = DestroyWindow(self.window);
        }
    }
}

unsafe extern "system" fn window_proc(window: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if TASKBAR_CREATED.get() == Some(&msg) {
        // SAFETY: the watcher keeps Signals alive until after DestroyWindow.
        let signals = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Signals;
        // SAFETY: null during creation; otherwise the Rc allocation above.
        if let Some(signals) = unsafe { signals.as_ref() } {
            signals.ready.set(true);
        }
    }
    // SAFETY: default handling of the system-provided message.
    unsafe { DefWindowProcW(window, msg, w, l) }
}

unsafe extern "system" fn on_event(
    hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let signals =
        WATCHES.with_borrow(|watches| watches.get(&(hook.0 as isize)).and_then(Weak::upgrade));
    let Some(signals) = signals else { return };
    if event == EVENT_OBJECT_DESTROY {
        if signals
            .host
            .get()
            .is_some_and(|host| host.contains_window(window))
        {
            signals.host.set(None);
            signals.lost.set(true);
            signals.ready.set(true); // One debounced attempt, then wait for shell creation.
        }
    } else if signals.host.get().is_none() {
        // Creation/show also wakes recovery if TaskbarCreated never arrives.
        let mut class = [0u16; 32];
        // SAFETY: read-only query; a vanished window returns an empty class.
        let len = unsafe { GetClassNameW(window, &mut class) }.max(0) as usize;
        if matches!(
            String::from_utf16_lossy(&class[..len]).as_str(),
            "Progman" | "SHELLDLL_DefView" | "WorkerW"
        ) {
            signals.ready.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_destruction_is_coalesced_and_rearms_for_both_desktop_layouts() {
        let hwnd = |value| HWND(value as *mut std::ffi::c_void);
        let hook = HWINEVENTHOOK(123 as _);
        let first = DesktopHost::Layered {
            progman: hwnd(1),
            defview: hwnd(2),
        };
        let next = DesktopHost::Legacy {
            workerw: hwnd(3),
            icons: hwnd(4),
        };
        let signals = Rc::new(Signals {
            host: Cell::new(Some(first)),
            lost: Cell::new(false),
            ready: Cell::new(false),
        });
        WATCHES.with_borrow_mut(|watches| {
            watches.insert(hook.0 as isize, Rc::downgrade(&signals));
        });
        let destroy = |window, object, child| {
            // SAFETY: destroy handling only compares opaque HWND values and updates our signals.
            unsafe { on_event(hook, EVENT_OBJECT_DESTROY, window, object, child, 0, 0) };
        };
        destroy(hwnd(99), OBJID_WINDOW.0, 0);
        destroy(hwnd(1), OBJID_WINDOW.0, 1); // accessibility child, not the desktop HWND
        destroy(hwnd(1), -4, 0); // client object
        assert!(!signals.lost.get());
        destroy(hwnd(2), OBJID_WINDOW.0, 0); // DefView alone can be rebuilt
        assert!(signals.lost.replace(false));
        assert!(signals.ready.replace(false));
        assert_eq!(signals.host.get(), None);
        destroy(hwnd(1), OBJID_WINDOW.0, 0); // another event from the same dying shell
        assert!(!signals.lost.get());
        signals.host.set(Some(next));
        destroy(hwnd(2), OBJID_WINDOW.0, 0); // delayed event from the old shell
        assert!(!signals.lost.get());
        destroy(hwnd(4), OBJID_WINDOW.0, 0); // legacy icon host
        assert!(signals.lost.replace(false));
        signals.host.set(Some(next));
        destroy(hwnd(3), OBJID_WINDOW.0, 0); // legacy wallpaper host
        assert!(signals.lost.get());
        WATCHES.with_borrow_mut(|watches| {
            watches.remove(&(hook.0 as isize));
        });
    }
}
