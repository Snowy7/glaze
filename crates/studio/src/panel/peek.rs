//! Hold-to-peek uses release hooks because a hidden window no longer receives key/mouse up.
use crate::app::App;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

static THREAD: AtomicU32 = AtomicU32::new(0);
static KEY: AtomicU32 = AtomicU32::new(0);
thread_local! {
    static HOOKS: std::cell::RefCell<Vec<HHOOK>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn start(app: &mut App, key: Option<u32>) {
    let Some(window) = app.window else { return };
    if app.peeking {
        return;
    }
    // SAFETY: read-only key state; hooks run on our message-pumping thread.
    let mouse = key.is_none() && unsafe { GetAsyncKeyState(1) } < 0;
    let hook = unsafe {
        if mouse {
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0)
        } else {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0)
        }
    };
    let Ok(hook) = hook else {
        app.alert("Couldn't start Peek. Try again.");
        return;
    };
    HOOKS.with(|hooks| hooks.borrow_mut().push(hook));
    KEY.store(key.unwrap_or(0x0D), Ordering::Relaxed);
    THREAD.store(app.thread, Ordering::Relaxed);
    app.input.pressed = None;
    app.peeking = true;
    unsafe {
        let _ = ShowWindow(window, SW_HIDE);
    }
}

pub fn end() {
    THREAD.store(0, Ordering::Relaxed);
    HOOKS.with(|hooks| {
        for hook in hooks.borrow_mut().drain(..) {
            // SAFETY: each hook belongs to this module and is removed exactly once.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
    });
}

fn finish() {
    let thread = THREAD.swap(0, Ordering::Relaxed);
    if thread != 0 {
        let _ =
            unsafe { PostThreadMessageW(thread, crate::peek::WM_PEEK_END, WPARAM(0), LPARAM(0)) };
    }
}

unsafe extern "system" fn key_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && THREAD.load(Ordering::Relaxed) != 0 {
        // SAFETY: Windows supplies a KBDLLHOOKSTRUCT for a nonnegative hook code.
        let key = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) }.vkCode;
        if matches!(w.0 as u32, WM_KEYUP | WM_SYSKEYUP)
            && (key == KEY.load(Ordering::Relaxed) || matches!(key, 0xA2 | 0xA3))
        {
            finish();
        }
        if key == KEY.load(Ordering::Relaxed) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && THREAD.load(Ordering::Relaxed) != 0 && w.0 as u32 == WM_LBUTTONUP {
        finish();
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
