use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EnumWindows, GA_PARENT, GWL_EXSTYLE, GetAncestor, GetClassNameW,
    GetDesktopWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, IsZoomed, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    OBJID_WINDOW, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_NULL, WM_QUIT, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use crate::Rect;

static CHANGED: AtomicBool = AtomicBool::new(true);
/// The thread [`WindowEvents::install`] ran on, woken when `CHANGED` turns true (0: none).
static CONSUMER: AtomicU32 = AtomicU32::new(0);

/// How long the hook thread lets events gather after taking a batch. Every cursor move anywhere
/// is a LOCATIONCHANGE, about 900 a second from a fast mouse; taken one by one on the engine's
/// frame loop they cost it 1.3% of a core while the mouse moved (2026-11-06).
const BATCH: Duration = Duration::from_millis(50);

/// Subscribes to top-level window changes (focus, move, min/max, show/hide, cloak) on a thread
/// of its own, which takes them in batches (at most one wake per [`BATCH`]). When one could
/// change what covers a monitor it posts `WM_NULL` to the thread that installed it, so that
/// thread's message wait returns and [`take_changed`](Self::take_changed) has the news. Unhooks
/// on drop.
pub struct WindowEvents {
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: u32,
}

impl WindowEvents {
    pub fn install() -> Self {
        // SAFETY: a read of the calling thread's id.
        CONSUMER.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
        let (started, thread_id) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("window-events".into())
            .spawn(move || hook_thread(started))
            .inspect_err(|e| log::warn!("window events unavailable: {e}"))
            .ok();
        Self {
            thread,
            thread_id: thread_id.recv().unwrap_or(0),
        }
    }

    /// True if any top-level window changed since the last call.
    pub fn take_changed(&self) -> bool {
        CHANGED.swap(false, Ordering::Relaxed)
    }

    /// Coverage of individual desktop surfaces, without the monitor-only maximized shortcut.
    pub fn covered_regions(regions: &[Rect]) -> Vec<bool> {
        covered(regions, false)
    }
}

impl Drop for WindowEvents {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // SAFETY: the hook thread's queue exists (made before it sent its id).
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = thread.join();
        }
        CONSUMER.store(0, Ordering::Relaxed);
    }
}

/// Hooks the window events, sends its thread id once its queue exists, then takes events in
/// batches until `WM_QUIT`, and unhooks.
fn hook_thread(started: std::sync::mpsc::Sender<u32>) {
    let ranges = [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
    ];
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // SAFETY: out-of-context hooks with a 'static callback, delivered to and unhooked on this
    // thread; the message calls only touch this thread's queue.
    unsafe {
        let hooks: Vec<HWINEVENTHOOK> = ranges
            .into_iter()
            .map(|(min, max)| SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags))
            .filter(|h| !h.is_invalid())
            .collect();
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        let _ = started.send(GetCurrentThreadId());
        'run: loop {
            MsgWaitForMultipleObjectsEx(None, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                DispatchMessageW(&msg);
            }
            std::thread::sleep(BATCH);
        }
        for hook in hooks {
            let _ = UnhookWinEvent(hook);
        }
    }
}

/// Records a coverage-relevant change and wakes the consumer once per change.
fn changed() {
    if !CHANGED.swap(true, Ordering::Relaxed) {
        let consumer = CONSUMER.load(Ordering::Relaxed);
        if consumer != 0 {
            // SAFETY: a no-op thread message; a gone thread just fails the post.
            let _ = unsafe { PostThreadMessageW(consumer, WM_NULL, WPARAM(0), LPARAM(0)) };
        }
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
    // Ignore carets, cursors and other sub-objects; they fire constantly.
    if object == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 {
        // Child HWND animations also use OBJID_WINDOW. They cannot cover a monitor, and must
        // not make the widget host enumerate/restack every desktop surface. A destroyed HWND
        // cannot be classified anymore, so keep its wake (a maximized app may have closed).
        // SAFETY: read-only hierarchy queries; a stale HWND simply has no desktop parent.
        let top = unsafe { GetAncestor(hwnd, GA_PARENT) == GetDesktopWindow() };
        // Use the same overlay exclusion as opaque_frame: wallpaper/glass/HUD windows cannot
        // cover widgets, so their location updates must not repeatedly scan/restack the desktop.
        // SAFETY: read-only style query; a stale handle returns zero and its destroy wake is kept.
        let overlay = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
            & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0)
            != 0;
        if (top && !overlay) || event == EVENT_OBJECT_DESTROY || event == EVENT_SYSTEM_FOREGROUND {
            changed();
        }
    }
}

/// For each monitor: is it fully hidden by a maximized or fullscreen window?
/// One cheap pass over top-level windows; call it after [`WindowEvents`] fires.
pub fn covered_monitors(monitors: &[Rect]) -> Vec<bool> {
    covered(monitors, true)
}

fn covered(monitors: &[Rect], maximized: bool) -> Vec<bool> {
    let mut scan = Scan {
        monitors,
        covered: vec![false; monitors.len()],
        pid: std::process::id(),
        maximized,
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
    maximized: bool,
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
        if (scan.maximized && zoomed && monitor.contains_point(cx, cy)) || frame.contains(monitor) {
            *covered = true;
        }
    }
    true.into()
}

/// The visible frame of a window, or `None` if it cannot hide the desktop
/// (invisible, minimized, cloaked, overlay-style, shell, or ours).
pub(crate) unsafe fn opaque_frame(hwnd: HWND, our_pid: u32) -> Option<Rect> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_CHILD, WS_POPUP,
    };
    use windows::core::w;

    #[test]
    fn child_moves_do_not_invalidate_coverage_but_root_lifecycle_does() {
        // SAFETY: hidden test windows on this thread, destroyed before returning; STATIC is a
        // system class and neither creation nor the callback borrows application state.
        unsafe {
            let root = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("coverage root"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let child = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("coverage child"),
                WS_CHILD,
                0,
                0,
                1,
                1,
                Some(root),
                None,
                None,
                None,
            )
            .unwrap();
            CHANGED.store(false, Ordering::Relaxed);
            on_event(
                Default::default(),
                EVENT_OBJECT_LOCATIONCHANGE,
                child,
                OBJID_WINDOW.0,
                CHILDID_SELF as i32,
                0,
                0,
            );
            assert!(!CHANGED.load(Ordering::Relaxed));
            on_event(
                Default::default(),
                EVENT_OBJECT_LOCATIONCHANGE,
                root,
                OBJID_WINDOW.0,
                CHILDID_SELF as i32,
                0,
                0,
            );
            assert!(CHANGED.swap(false, Ordering::Relaxed));
            let overlay = CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("coverage overlay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            on_event(
                Default::default(),
                EVENT_OBJECT_LOCATIONCHANGE,
                overlay,
                OBJID_WINDOW.0,
                CHILDID_SELF as i32,
                0,
                0,
            );
            assert!(!CHANGED.load(Ordering::Relaxed));
            DestroyWindow(overlay).unwrap();
            DestroyWindow(child).unwrap();
            DestroyWindow(root).unwrap();
            on_event(
                Default::default(),
                EVENT_OBJECT_DESTROY,
                root,
                OBJID_WINDOW.0,
                CHILDID_SELF as i32,
                0,
                0,
            );
            assert!(
                CHANGED.swap(false, Ordering::Relaxed),
                "closing a covered app still resumes its monitor"
            );
        }
    }
}
