//! The Windows key opens the launcher (docs/bars.md, "Launcher"), through a low-level keyboard
//! hook (`WH_KEYBOARD_LL`) in our own process: Windows calls it on our thread, nothing is loaded
//! into anyone else's. It's only installed while `launcher.winKey` is on and the native taskbar is
//! hidden, on a thread of its own (a hook that answers late is dropped by Windows), and it decides
//! with a few comparisons per key.
//!
//! Only a lone press and release replaces Start. Every key passes through untouched (Win+E,
//! Win+Shift+S and the rest work as ever), except the release of a lone press: that one is
//! swallowed and replayed as the unassigned key `vkE8`, then the release. Explorer sees a Windows
//! key combination, so Start stays closed (AutoHotkey's "menu mask key"), and the launcher opens.
//! With a fullscreen app in front, or no launcher to open, the release goes through and Start
//! opens as usual.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_QUIT, WM_SYSKEYDOWN,
};

use crate::flyouts::Done;
use crate::wake::{Msg, Waker};

/// Unassigned virtual key: pressed between the Windows key's press and release, it makes them a
/// combination, so Start doesn't open.
const MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// `dwExtraInfo` of the keys the bars inject themselves ("mypa"), so the hook lets them by.
pub const OURS: usize = 0x6D79_7061;

/// What the hook does with one key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pass,
    /// The release of a lone Windows key press: swallow it, mask it and open the launcher.
    Open,
}

/// Follows the Windows key: pressed, then whether anything else happened before its release.
#[derive(Debug, Default)]
pub struct Machine {
    /// The Windows key being held (`VK_LWIN` or `VK_RWIN`).
    held: Option<u16>,
    /// Something else was pressed while it was held.
    combo: bool,
}

fn is_win(vk: u16) -> bool {
    vk == VK_LWIN.0 || vk == VK_RWIN.0
}

impl Machine {
    /// One key event. `ours` is input the bars injected themselves (the replay below, the native
    /// Start fallback, desktop switching): it passes and changes nothing, so the fallback can't
    /// loop back here. Everyone else's input counts, injected or not (on-screen keyboards,
    /// remote desktop, key remappers).
    pub fn on_key(&mut self, vk: u16, down: bool, ours: bool) -> Action {
        if ours {
            return Action::Pass;
        }
        match (self.held, down) {
            (Some(held), true) if vk == held => Action::Pass, // auto-repeat
            (Some(_), true) => {
                self.combo = true;
                Action::Pass
            }
            (Some(held), false) if vk == held => {
                let lone = !self.combo;
                self.held = None;
                self.combo = false;
                if lone { Action::Open } else { Action::Pass }
            }
            (None, true) if is_win(vk) => {
                self.held = Some(vk);
                self.combo = false;
                Action::Pass
            }
            _ => Action::Pass,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Whether a lone press opens the launcher right now (the main loop keeps it current: false
/// while no bar can show it).
static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static HOOK: RefCell<(Machine, Option<Waker>)> = RefCell::new((Machine::default(), None));
}

/// The hook's thread, while installed.
pub struct WinKey {
    thread: u32,
    handle: Option<JoinHandle<()>>,
}

impl WinKey {
    pub fn start(waker: Waker) -> Option<Self> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("winkey".into())
            .spawn(move || {
                HOOK.with(|hook| hook.borrow_mut().1 = Some(waker));
                // SAFETY: a low-level hook with a 'static procedure, on this thread, removed below.
                let hook = unsafe {
                    let module = GetModuleHandleW(None).ok();
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module.map(Into::into), 0)
                };
                // SAFETY: no preconditions.
                let _ = ready_tx.send((unsafe { GetCurrentThreadId() }, hook.is_ok()));
                let Ok(hook) = hook else { return };
                let mut msg = MSG::default();
                // SAFETY: this thread's own message loop; the hook is called from it.
                unsafe {
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(hook);
                }
            })
            .ok()?;
        let (thread, installed) = ready_rx.recv().ok()?;
        if !installed {
            log::warn!("launcher: couldn't install the Windows key hook");
            let _ = handle.join();
            return None;
        }
        log::info!("launcher: the Windows key opens the launcher");
        Some(Self {
            thread,
            handle: Some(handle),
        })
    }

    pub fn set_enabled(on: bool) {
        ENABLED.store(on, Ordering::Relaxed);
    }
}

impl Drop for WinKey {
    fn drop(&mut self) {
        // SAFETY: posting to the thread we started; it unhooks and exits.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        log::info!("launcher: Windows key hook removed");
    }
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lParam is a KBDLLHOOKSTRUCT.
        let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let ours = key.dwExtraInfo == OURS;
        let vk = key.vkCode as u16;
        let action = HOOK.with(|hook| {
            let Ok(mut hook) = hook.try_borrow_mut() else {
                return Action::Pass;
            };
            if !ENABLED.load(Ordering::Relaxed) {
                hook.0.reset();
                return Action::Pass;
            }
            hook.0.on_key(vk, down, ours)
        });
        if action == Action::Open && !mypaper_desktop::fullscreen_app_in_front() {
            replay_masked(VIRTUAL_KEY(vk));
            HOOK.with(|hook| {
                if let Ok(hook) = hook.try_borrow()
                    && let Some(waker) = &hook.1
                {
                    waker.send(Msg::Flyout(Done::WinKey));
                }
            });
            return LRESULT(1); // swallowed; the replay carries the release
        }
    }
    // SAFETY: passes the event on, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The mask key's press and release, then the Windows key's release.
fn replay_masked(win: VIRTUAL_KEY) {
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                dwExtraInfo: OURS,
                ..Default::default()
            },
        },
    };
    let inputs = [
        key(MASK, KEYBD_EVENT_FLAGS(0)),
        key(MASK, KEYEVENTF_KEYUP),
        key(win, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY),
    ];
    // SAFETY: a valid input array and its element size.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

#[cfg(test)]
mod tests {
    use super::*;

    const LWIN: u16 = VK_LWIN.0;
    const RWIN: u16 = VK_RWIN.0;
    const E: u16 = 0x45;
    const SHIFT: u16 = 0x10;

    fn run(events: &[(u16, bool, bool)]) -> Vec<Action> {
        let mut machine = Machine::default();
        events
            .iter()
            .map(|&(vk, down, ours)| machine.on_key(vk, down, ours))
            .collect()
    }

    #[test]
    fn a_lone_press_opens_the_launcher() {
        assert_eq!(
            run(&[(LWIN, true, false), (LWIN, false, false)]),
            [Action::Pass, Action::Open]
        );
        assert_eq!(
            run(&[
                (RWIN, true, false),
                (RWIN, true, false),
                (RWIN, false, false)
            ]),
            [Action::Pass, Action::Pass, Action::Open],
            "auto-repeat is still a lone press"
        );
    }

    #[test]
    fn combinations_pass_through() {
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (E, true, false),
                (E, false, false),
                (LWIN, false, false)
            ]),
            [Action::Pass; 4]
        );
        // Shift pressed and released inside the press: Win+Shift is a combination too.
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (SHIFT, true, false),
                (SHIFT, false, false),
                (LWIN, false, false)
            ])[3],
            Action::Pass
        );
        // Both Windows keys.
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (RWIN, true, false),
                (LWIN, false, false)
            ])[2],
            Action::Pass
        );
        // Our own keys during the press (desktop switching) don't count either way.
        assert_eq!(
            run(&[(LWIN, true, false), (E, true, true), (LWIN, false, false)])[2],
            Action::Open
        );
    }

    #[test]
    fn our_own_input_and_strays_never_open_it() {
        assert_eq!(
            run(&[(LWIN, true, true), (LWIN, false, true)]),
            [Action::Pass; 2]
        );
        // A release without its press (the hook came in the middle), then a real lone press.
        assert_eq!(
            run(&[
                (LWIN, false, false),
                (LWIN, true, false),
                (LWIN, false, false)
            ]),
            [Action::Pass, Action::Pass, Action::Open]
        );
        // After a combination, the next lone press counts again.
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (E, true, false),
                (LWIN, false, false),
                (LWIN, true, false),
                (LWIN, false, false)
            ])[4],
            Action::Open
        );
    }
}
