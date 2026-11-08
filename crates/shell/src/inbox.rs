//! The shell's inbox: a message-only window that other threads (the mouse hook, file watchers)
//! post to, and that only queues what it receives for the UI loop.
//!
//! A window, not the thread: the UI thread makes COM calls to Explorer, and COM's wait loop
//! dispatches (and so discards) thread messages that have no window, which would lose a
//! swallowed right-click's release. A window message always reaches [`inbox_proc`]. Core finds
//! the window by its class to ask the shell to quit (`WM_CLOSE`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, PostMessageW, PostQuitMessage,
    RegisterClassExW, WM_APP, WM_CLOSE, WM_TIMER, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

pub const CLASS: PCWSTR = w!("glaze.shell.inbox");
/// `config.json` changed. (Clear of `glaze_desktop::WM_FACT`, `WM_APP + 2`, which the control
/// window posts to the thread.)
pub const WM_CONFIG: u32 = WM_APP + 10;
/// The wallpaper wrote a new `palette.json`.
pub const WM_PALETTE: u32 = WM_APP + 11;

/// A message waiting for the UI loop.
#[derive(Debug, Clone, Copy)]
pub struct Msg {
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
}

/// The inbox window's raw handle; 0 while there is none.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static QUEUE: RefCell<VecDeque<Msg>> = const { RefCell::new(VecDeque::new()) };
}

/// The inbox window; destroyed when dropped.
pub struct Inbox {
    hwnd: HWND,
}

impl Inbox {
    pub fn new() -> Result<Self> {
        // SAFETY: plain Win32 registration and message-only window creation with a 'static
        // procedure.
        let hwnd = unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(inbox_proc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
            }
            CreateWindowExW(
                Default::default(),
                CLASS,
                w!("Glaze shell"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .context("creating the shell's inbox window")?
        };
        WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
        Ok(Self { hwnd })
    }

    /// For timers that should tick into the queue.
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        WINDOW.store(0, Ordering::Relaxed);
        // SAFETY: our own window, on its creating thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Posts to the inbox from any thread (microseconds; never blocks).
pub fn post(message: u32, lparam: isize) {
    let window = WINDOW.load(Ordering::Relaxed);
    if window == 0 {
        return;
    }
    // SAFETY: posting to a window handle is always safe; failure means it is gone.
    let _ = unsafe {
        PostMessageW(
            Some(HWND(window as *mut _)),
            message,
            WPARAM(0),
            LPARAM(lparam),
        )
    };
}

/// The oldest queued message.
pub fn next() -> Option<Msg> {
    QUEUE.with(|queue| queue.borrow_mut().pop_front())
}

/// Queues `WM_APP` messages and timer ticks, whatever the thread is doing when they arrive, so
/// they are handled in order and never re-entrantly. `WM_CLOSE` ends the shell.
unsafe extern "system" fn inbox_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if (WM_APP..0xC000).contains(&message) || message == WM_TIMER {
        QUEUE.with(|queue| {
            queue.borrow_mut().push_back(Msg {
                message,
                wparam: wparam.0,
                lparam: lparam.0,
            });
        });
        return LRESULT(0);
    }
    if message == WM_CLOSE {
        // SAFETY: posts WM_QUIT to this thread's queue; the loop cleans up.
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
