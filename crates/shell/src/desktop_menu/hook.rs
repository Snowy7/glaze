//! The hook thread: a `WH_MOUSE_LL` hook and a message loop, nothing else. Its callback returns
//! in microseconds: for everything but a right-button press or release it returns at once; for
//! those it asks a pure decision ([`Swallower`]) using a window-class check and the cached icon
//! rectangles ([`ICONS`]), and posts to the shell's inbox. A right-click on the bare desktop is
//! swallowed (press and release); everything else, every click on an icon included, passes.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use glaze_desktop::desktop_hit::{self, Click, Edge, IconMap, Swallower, Under, Verdict};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEINPUT, SendInput, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetCursorPos, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN,
};

use super::{WM_ICONS_DIRTY, WM_OUTSIDE, WM_RELEASE, WM_REQUEST};
use crate::inbox;

/// Marks the right-clicks we send ourselves, so our hook lets them through.
const OURS: usize = 0x6d79_7061; // "mypa"

/// To the hook thread: wParam 1 = install the hook, 0 = remove it.
const WM_HOOK_SET: u32 = WM_APP + 45;

/// Where the icons are; replaced whole by the UI thread, read by the hook.
pub static ICONS: RwLock<IconMap> = RwLock::new(IconMap::empty());
/// The menu is on screen: a click anywhere outside it closes it.
pub static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// A refresh of the icon rectangles is already asked for.
pub static ICONS_DIRTY_QUEUED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Only the hook thread runs the callback.
    static SWALLOWER: std::cell::RefCell<Swallower> = std::cell::RefCell::new(Swallower::default());
}

/// The thread that owns the mouse hook; it stops when this is dropped.
pub struct HookThread {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl HookThread {
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("desktop-menu-hook".into())
            .spawn(move || hook_main(tx))
            .context("spawning the mouse hook thread")?;
        let thread_id = rx
            .recv()
            .context("the mouse hook thread died while starting")?;
        Ok(Self {
            thread_id,
            thread: Some(thread),
        })
    }

    /// Installs or removes the hook (asynchronously).
    pub fn set(&self, installed: bool) {
        // SAFETY: posting to a thread id is always safe.
        let _ = unsafe {
            PostThreadMessageW(
                self.thread_id,
                WM_HOOK_SET,
                WPARAM(usize::from(installed)),
                LPARAM(0),
            )
        };
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id is always safe; failure means it is gone.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn hook_main(ready: mpsc::Sender<u32>) {
    // Physical pixels in the hook's coordinates, whatever the process default is.
    // SAFETY: affects this thread only.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mut msg = MSG::default();
    // SAFETY: creates this thread's message queue before its id is published.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
    // SAFETY: no preconditions.
    let _ = ready.send(unsafe { GetCurrentThreadId() });

    let mut hook: Option<HHOOK> = None;
    // SAFETY: standard message loop; the hook procedure runs inside `GetMessageW`.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOOK_SET {
                match (msg.wParam.0 != 0, hook) {
                    (true, None) => hook = install_hook(),
                    (false, Some(installed)) => {
                        let _ = UnhookWindowsHookEx(installed);
                        hook = None;
                        // A swallowed press must not wait for a release that can no longer come.
                        SWALLOWER.with(|s| *s.borrow_mut() = Swallower::default());
                    }
                    _ => {}
                }
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if let Some(installed) = hook {
            let _ = UnhookWindowsHookEx(installed);
        }
    }
}

fn install_hook() -> Option<HHOOK> {
    // SAFETY: a low-level hook with a 'static procedure, installed by the thread that pumps messages.
    let installed = unsafe {
        let module = GetModuleHandleW(None).ok()?;
        SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(HINSTANCE(module.0)), 0)
    };
    installed
        .inspect_err(|e| log::warn!("desktop menu: mouse hook not installed: {e}"))
        .ok()
}

/// The `WH_MOUSE_LL` callback. Microseconds: mouse moves return at once; only the right button
/// runs the (pure) decision, and the rest is a posted message.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let message = wparam.0 as u32;
    let mut swallow = false;
    if code >= 0 {
        match message {
            WM_RBUTTONDOWN | WM_RBUTTONUP => {
                swallow = on_right_button(message == WM_RBUTTONDOWN, lparam);
            }
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                if MENU_OPEN.load(Ordering::Relaxed) =>
            {
                // SAFETY: for a low-level mouse hook, lparam points at an MSLLHOOKSTRUCT.
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                inbox::post(WM_OUTSIDE, pack(info.pt.x, info.pt.y));
            }
            WM_LBUTTONUP => {
                // A drop on the desktop may have moved an icon: refresh the rectangles soon.
                // SAFETY: as above.
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                if desktop_hit::under(info.pt) == Under::Desktop
                    && !ICONS_DIRTY_QUEUED.swap(true, Ordering::Relaxed)
                {
                    inbox::post(WM_ICONS_DIRTY, 0);
                }
            }
            _ => {}
        }
    }
    if swallow {
        LRESULT(1)
    } else {
        // SAFETY: passing the event on, with the arguments we were given.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// The decision for a right-button event; posts the message that follows from it.
fn on_right_button(down: bool, lparam: LPARAM) -> bool {
    // SAFETY: for a low-level mouse hook, lparam points at an MSLLHOOKSTRUCT.
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    let click = Click {
        edge: if down { Edge::Down } else { Edge::Up },
        at: (info.pt.x, info.pt.y),
        // SAFETY: no preconditions.
        now_ms: unsafe { GetTickCount64() },
        ours: info.flags & LLMHF_INJECTED != 0 && info.dwExtraInfo == OURS,
        // SAFETY: no preconditions.
        shift: down && unsafe { GetAsyncKeyState(i32::from(VK_SHIFT.0)) } < 0,
    };
    let verdict = SWALLOWER.with(|swallower| {
        // A poisoned lock still holds a usable map: the writer only ever swaps it whole.
        let icons = ICONS.read().unwrap_or_else(|e| e.into_inner());
        swallower
            .borrow_mut()
            .decide(click, &icons, || desktop_hit::under(info.pt))
    });
    match verdict {
        Verdict::Pass => false,
        Verdict::SwallowDown => {
            inbox::post(WM_REQUEST, pack(info.pt.x, info.pt.y));
            true
        }
        Verdict::SwallowUp => {
            inbox::post(WM_RELEASE, 0);
            true
        }
    }
}

/// Sends a right click (press and release) at `at`, marked as ours so the hook passes it. The
/// cursor goes back to where the user clicked, so the native menu opens there.
pub fn replay_right_click(at: (i32, i32)) {
    let mouse = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                dwExtraInfo: OURS,
                ..Default::default()
            },
        },
    };
    // SAFETY: plain input injection with a fully initialized array.
    unsafe {
        let _ = SetCursorPos(at.0, at.1);
        SendInput(
            &[mouse(MOUSEEVENTF_RIGHTDOWN), mouse(MOUSEEVENTF_RIGHTUP)],
            size_of::<INPUT>() as i32,
        );
    }
}

/// Screen coordinates as an `lParam` (signed 16-bit halves hold any virtual screen).
pub fn pack(x: i32, y: i32) -> isize {
    (i32::from(y as i16) << 16 | i32::from(x as i16) & 0xFFFF) as isize
}

pub fn unpack(lparam: isize) -> (i32, i32) {
    (i32::from(lparam as i16), i32::from((lparam >> 16) as i16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_points_survive_the_message_round_trip_including_negative_monitors() {
        for point in [
            (0, 0),
            (1280, 700),
            (-1280, 700),
            (-2560, -10),
            (4479, 1439),
            (-1, -1),
        ] {
            assert_eq!(unpack(pack(point.0, point.1)), point);
        }
    }
}
