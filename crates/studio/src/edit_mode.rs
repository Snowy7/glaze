//! Edit mode: Studio edits the real desktop. Windows' own Minimize All clears the desktop,
//! Studio becomes the docked panel above it, and a keyboard hook keeps Alt+Tab, Alt+Esc,
//! Ctrl+Esc and the Windows key from leaving until Done or Esc. Leaving restores Studio's
//! window where it was and Undo Minimize All brings the other windows back as Windows left
//! them (snapped and maximized windows included).
//!
//! Ctrl+Alt+Del and Ctrl+Shift+Esc are never blocked. The hook belongs to Studio's process, so
//! if Studio ends, Windows drops it; dropping a [`Session`] also unhooks and restores windows.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_RWIN, VK_TAB};
use windows::Win32::UI::Shell::{DefSubclassProc, IShellDispatch, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetWindowPlacement, HC_ACTION, HHOOK, HWND_NOTOPMOST, HWND_TOPMOST,
    KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MINMAXINFO, MSG, PM_REMOVE, PeekMessageW, SW_HIDE, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPlacement, SetWindowPos,
    SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WINDOWPLACEMENT, WM_GETMINMAXINFO,
};

/// While true, the keyboard hook swallows the keys that would leave edit mode.
static LOCKED: AtomicBool = AtomicBool::new(false);
/// The docked panel's minimum width in physical pixels, for the window subclass.
static MIN_WIDTH: AtomicI32 = AtomicI32::new(360);
const SUBCLASS: usize = 0x6D_6564; // "med"
/// Explorer minimizes asynchronously; Studio shows itself only after that has happened.
const SETTLE: Duration = Duration::from_millis(320);

/// An edit-mode session: what to give back when it ends.
pub struct Session {
    window: HWND,
    placement: WINDOWPLACEMENT,
    hook: Option<HHOOK>,
    minimized: bool,
}

impl Session {
    /// Clears the desktop and turns Studio's window into the docked panel at `bounds`.
    pub fn enter(window: HWND, bounds: (i32, i32, i32, i32)) -> Self {
        let mut placement = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        // SAFETY: our own window; `placement` is a correctly sized out-parameter.
        let _ = unsafe { GetWindowPlacement(window, &mut placement) };
        MIN_WIDTH.store(bounds.2.min(360), Ordering::Relaxed);
        // SAFETY: hiding our own window so Minimize All leaves it alone; the subclass only
        // lowers WM_GETMINMAXINFO's minimum and is removed in `Drop`.
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
            let _ = SetWindowSubclass(window, Some(min_size), SUBCLASS, 0);
        }
        let minimized = minimize_all(true);
        if minimized {
            settle(SETTLE);
        }
        // SAFETY: our own window: place it as the panel above everything and show it.
        unsafe {
            let _ = SetWindowPos(window, Some(HWND_TOPMOST), bounds.0, bounds.1, bounds.2, bounds.3, SWP_NOACTIVATE);
            let _ = ShowWindow(window, SW_SHOW);
            let _ = SetForegroundWindow(window);
        }
        crate::panel::round_window(window);
        LOCKED.store(true, Ordering::Relaxed);
        // SAFETY: a low-level hook on this thread, which pumps messages; removed in `Drop`.
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keys), None, 0) }
            .inspect_err(|e| log::warn!("edit mode: keyboard lock unavailable: {e}"))
            .ok();
        Self { window, placement, hook, minimized }
    }

    /// Puts Studio's window back where it was and brings the other windows back.
    pub fn exit(self) {
        // SAFETY: our own window; the placement was read from it when the session began.
        unsafe {
            let _ = SetWindowPos(self.window, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = SetWindowPlacement(self.window, &self.placement);
        }
        crate::panel::round_window(self.window);
        // `Drop` unhooks, removes the subclass and restores the other windows.
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        LOCKED.store(false, Ordering::Relaxed);
        // SAFETY: the hook and subclass this session installed on this thread.
        unsafe {
            if let Some(hook) = self.hook.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = RemoveWindowSubclass(self.window, Some(min_size), SUBCLASS);
        }
        if self.minimized {
            minimize_all(false);
            // SAFETY: our own window back in front of the windows coming back.
            let _ = unsafe { SetForegroundWindow(self.window) };
        }
    }
}

/// Windows' Minimize All (`true`) or Undo Minimize All (`false`), through the Shell object.
fn minimize_all(minimize: bool) -> bool {
    // SAFETY: COM on the UI thread (apartment-threaded; a repeat call is harmless) and an
    // in-process Shell object used and released here.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell: windows::core::Result<IShellDispatch> =
            CoCreateInstance(&windows::Win32::UI::Shell::Shell, None, CLSCTX_INPROC_SERVER);
        let result = shell.and_then(|shell| if minimize { shell.MinimizeAll() } else { shell.UndoMinimizeALL() });
        result.inspect_err(|e| log::warn!("edit mode: minimize all ({minimize}): {e}")).is_ok()
    }
}

/// Keeps this thread's windows answering for `length` (Explorer animates the minimize).
fn settle(length: Duration) {
    let start = Instant::now();
    while start.elapsed() < length {
        let mut msg = MSG::default();
        // SAFETY: the standard pump on the thread that owns these windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Swallows Alt+Tab, Alt+Esc, Ctrl+Esc and the Windows keys while edit mode is locked.
unsafe extern "system" fn keys(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && LOCKED.load(Ordering::Relaxed) {
        // SAFETY: for HC_ACTION, lparam points at this event's KBDLLHOOKSTRUCT.
        let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let vk = key.vkCode as u16;
        let alt = key.flags.contains(LLKHF_ALTDOWN);
        // SAFETY: a plain key-state query.
        let ctrl = unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) } < 0;
        if vk == VK_LWIN.0 || vk == VK_RWIN.0 || (alt && (vk == VK_TAB.0 || vk == VK_ESCAPE.0)) || (ctrl && !alt && vk == VK_ESCAPE.0) {
            return LRESULT(1);
        }
    }
    // SAFETY: passing the event on, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The window's own minimum stays for the full window; the panel may be narrower.
unsafe extern "system" fn min_size(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    // SAFETY: forwarding to the original procedure, then adjusting its MINMAXINFO answer.
    unsafe {
        let result = DefSubclassProc(hwnd, msg, wparam, lparam);
        if msg == WM_GETMINMAXINFO {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            info.ptMinTrackSize.x = MIN_WIDTH.load(Ordering::Relaxed);
            info.ptMinTrackSize.y = 160;
        }
        result
    }
}
