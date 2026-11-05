//! Peek (Ctrl D): Studio hides so the real desktop shows, until any key or click. While hidden,
//! a low-level keyboard and mouse hook (installed only for the peek) catches that first input,
//! swallows it, and brings Studio back.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, PostThreadMessageW, SW_HIDE, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN,
};

use crate::app::App;

static THREAD: AtomicU32 = AtomicU32::new(0);
/// Posted to the UI thread when the peek should end.
pub const WM_PEEK_END: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 42;

thread_local! {
    static HOOKS: std::cell::RefCell<Vec<HHOOK>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn start(app: &mut App) {
    let Some(hwnd) = app.window else { return };
    if app.peeking {
        return;
    }
    THREAD.store(app.thread, Ordering::Relaxed);
    // SAFETY: hooks with 'static procedures on this (message-pumping) thread, removed in `end`.
    unsafe {
        let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0);
        HOOKS.with(|h| h.borrow_mut().extend(keys.ok().into_iter().chain(mouse.ok())));
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    app.peeking = true;
}

pub fn end(app: &mut App) {
    HOOKS.with(|h| {
        for hook in h.borrow_mut().drain(..) {
            // SAFETY: our own hooks, removed once.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
    });
    if app.peeking {
        app.peeking = false;
        if let Some(hwnd) = app.window {
            // SAFETY: our own window.
            unsafe {
                let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
            }
        }
        app.dirty = true;
    }
}

fn finish() -> LRESULT {
    let thread = THREAD.swap(0, Ordering::Relaxed);
    if thread != 0 {
        // SAFETY: posting to our UI thread.
        let _ = unsafe { PostThreadMessageW(thread, WM_PEEK_END, WPARAM(0), LPARAM(0)) };
    }
    // Swallowed: the input that ends a peek does nothing else.
    LRESULT(1)
}

unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) && THREAD.load(Ordering::Relaxed) != 0 {
        return finish();
    }
    // SAFETY: passing the hook chain on.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN)
        && THREAD.load(Ordering::Relaxed) != 0
    {
        return finish();
    }
    // SAFETY: passing the hook chain on.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
