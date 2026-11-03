use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use crate::Rect;

static CHANGED: AtomicBool = AtomicBool::new(true);

/// Subscribes to top-level window changes (focus, move, min/max, show/hide,
/// cloak). Events arrive through the message loop of the installing thread,
/// so that thread must pump messages. Unhooks on drop.
pub struct WindowEvents {
    hooks: Vec<HWINEVENTHOOK>,
}

impl WindowEvents {
    pub fn install() -> Self {
        let ranges = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND),
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ];
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let hooks = ranges
            .into_iter()
            // SAFETY: out-of-context hook with a 'static callback.
            .map(|(min, max)| unsafe {
                SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags)
            })
            .filter(|h| !h.is_invalid())
            .collect();
        Self { hooks }
    }

    /// True if any top-level window changed since the last call.
    pub fn take_changed(&self) -> bool {
        CHANGED.swap(false, Ordering::Relaxed)
    }
}

impl Drop for WindowEvents {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: hooks came from SetWinEventHook.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // Ignore carets, cursors and other sub-objects; they fire constantly.
    if object == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 {
        CHANGED.store(true, Ordering::Relaxed);
    }
}

/// For each monitor: is it fully hidden by a maximized or fullscreen window?
/// One cheap pass over top-level windows; call it after [`WindowEvents`] fires.
pub fn covered_monitors(monitors: &[Rect]) -> Vec<bool> {
    let mut scan = Scan {
        monitors,
        covered: vec![false; monitors.len()],
        pid: std::process::id(),
    };
    // SAFETY: the callback only runs during this call, while `scan` is alive.
    unsafe {
        let _ = EnumWindows(Some(scan_window), LPARAM(&raw mut scan as isize));
    }
    scan.covered
}

struct Scan<'a> {
    monitors: &'a [Rect],
    covered: Vec<bool>,
    pid: u32,
}

const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

unsafe extern "system" fn scan_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the Scan owned by `covered_monitors`.
    let scan = unsafe { &mut *(data.0 as *mut Scan) };
    // SAFETY: read-only queries on a window handle from EnumWindows.
    let Some(frame) = (unsafe { opaque_frame(hwnd, scan.pid) }) else {
        return true.into();
    };
    // SAFETY: as above.
    let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
    let (cx, cy) = frame.center();
    for (monitor, covered) in scan.monitors.iter().zip(scan.covered.iter_mut()) {
        // Maximized windows stop at the taskbar, so match them by center.
        if (zoomed && monitor.contains_point(cx, cy)) || frame.contains(monitor) {
            *covered = true;
        }
    }
    true.into()
}

/// The visible frame of a window, or `None` if it cannot hide the desktop
/// (invisible, minimized, cloaked, overlay-style, shell, or ours).
unsafe fn opaque_frame(hwnd: HWND, our_pid: u32) -> Option<Rect> {
    // SAFETY: read-only queries on a valid top-level window.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) != 0 {
            return None; // overlays (e.g. GPU driver HUDs) span the screen but hide nothing
        }
        let mut cloaked = 0u32;
        let size = size_of::<u32>() as u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), size).is_ok()
            && cloaked != 0
        {
            return None; // suspended UWP apps, windows on other virtual desktops
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == our_pid {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class) as usize;
        if SHELL_CLASSES.contains(&String::from_utf16_lossy(&class[..len]).as_str()) {
            return None;
        }
        let mut rect = RECT::default();
        let size = size_of::<RECT>() as u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size,
        )
        .is_err()
        {
            GetWindowRect(hwnd, &mut rect).ok()?;
        }
        Some(rect.into())
    }
}
