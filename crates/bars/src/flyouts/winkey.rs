//! The Windows key opens the launcher (docs/bars.md, "Launcher"), through a low-level keyboard
//! hook (`WH_KEYBOARD_LL`) in our own process: Windows calls it on our thread, nothing is loaded
//! into anyone else's. It's only installed while `launcher.winKey` is on and the native taskbar is
//! hidden, on a thread of its own (a hook that answers late is dropped by Windows), and it decides
//! with a few comparisons and atomic loads per key.
//!
//! Only a lone press and release replaces Start. Every key passes through untouched (Win+E,
//! Win+Shift+S and the rest work as ever), except the release of a lone press: that one is
//! replayed as the unassigned key `vkE8`, then the release, and swallowed only once the replay is
//! in. Explorer sees a Windows key combination, so Start stays closed (AutoHotkey's "menu mask
//! key"), and the launcher opens. The release goes through and Start opens as usual when a
//! fullscreen app is in front, no launcher can open, the main loop is hung, or the replay failed
//! (so the key never stays down).

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, IsHungAppWindow, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, MSG, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
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
    /// The epoch this state belongs to (see [`Machine::sync`]).
    epoch: u32,
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

    /// Forgets what was seen in an earlier epoch: a press that went down before the session
    /// locked may never come up here, and would make the next lone press look like a repeat.
    pub fn sync(&mut self, epoch: u32) {
        if epoch != self.epoch {
            *self = Self {
                epoch,
                ..Self::default()
            };
        }
    }
}

// What the hook reads per key, kept current by the main loop (the hook must never wait on it).
/// A lone press opens the launcher right now: false while no bar can show it.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// A fullscreen app is in front: Start keeps the key.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Bumped when the session locks or unlocks, and while the hook is disabled.
static EPOCH: AtomicU32 = AtomicU32::new(0);
/// A window of the main loop: while it's hung, Start keeps the key (no launcher would open).
static MAIN: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static HOOK: RefCell<(Machine, Option<Waker>)> = RefCell::new((Machine::default(), None));
}

/// The hook's thread, while installed.
pub struct WinKey {
    thread: u32,
    handle: Option<JoinHandle<()>>,
}

impl WinKey {
    /// Installs the hook. `main` is a window of the main loop, watched for hangs.
    pub fn start(waker: Waker, main: HWND) -> Option<Self> {
        MAIN.store(main.0 as isize, Ordering::Relaxed);
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
                // Before saying it's ready: a quit posted from now on is kept.
                crate::wake::make_queue();
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
        if ENABLED.swap(on, Ordering::Relaxed) && !on {
            Self::reset();
        }
    }

    pub fn set_fullscreen(on: bool) {
        FULLSCREEN.store(on, Ordering::Relaxed);
    }

    /// The hook starts over with the next key (the session locked or unlocked).
    pub fn reset() {
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for WinKey {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // The thread unhooks and exits on WM_QUIT.
            crate::wake::quit_thread(self.thread, handle);
        }
        log::info!("launcher: Windows key hook removed");
    }
}

/// Whether the launcher can take this press: no fullscreen app in front, and a main loop that
/// answers (it opens the launcher).
fn launcher_can_open() -> bool {
    let main = HWND(MAIN.load(Ordering::Relaxed) as *mut c_void);
    // SAFETY: a read-only query; a stale handle reads as not hung.
    !FULLSCREEN.load(Ordering::Relaxed) && !unsafe { IsHungAppWindow(main) }.as_bool()
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lParam is a KBDLLHOOKSTRUCT.
        let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let ours = key.dwExtraInfo == OURS;
        let action = HOOK.with(|hook| {
            let Ok(mut hook) = hook.try_borrow_mut() else {
                return Action::Pass;
            };
            hook.0.sync(EPOCH.load(Ordering::Relaxed));
            if !ENABLED.load(Ordering::Relaxed) {
                return Action::Pass;
            }
            hook.0.on_key(key.vkCode as u16, down, ours)
        });
        if action == Action::Open && launcher_can_open() && replay_masked(key) {
            HOOK.with(|hook| {
                if let Ok(hook) = hook.try_borrow()
                    && let Some(waker) = &hook.1
                {
                    waker.send(Msg::Flyout(Done::WinKey(Instant::now())));
                }
            });
            return LRESULT(1); // swallowed; the replay carries the release
        }
    }
    // SAFETY: passes the event on, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The mask key's press and release, then the Windows key's release as it came (its scan code
/// and extended flag). Whether all three went in: if not, the real release must pass.
fn replay_masked(release: &KBDLLHOOKSTRUCT) -> bool {
    let key = |vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                dwExtraInfo: OURS,
                ..Default::default()
            },
        },
    };
    let extended = if release.flags.0 & LLKHF_EXTENDED.0 != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let inputs = [
        key(MASK, 0, KEYBD_EVENT_FLAGS(0)),
        key(MASK, 0, KEYEVENTF_KEYUP),
        key(
            VIRTUAL_KEY(release.vkCode as u16),
            release.scanCode as u16,
            KEYEVENTF_KEYUP | extended,
        ),
    ];
    // SAFETY: a valid input array and its element size.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
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
    fn a_new_epoch_forgets_a_press_that_never_came_up() {
        let mut machine = Machine::default();
        // Win went down, then the session locked (its release went to the secure desktop).
        assert_eq!(machine.on_key(LWIN, true, false), Action::Pass);
        machine.sync(1);
        assert_eq!(machine.on_key(E, true, false), Action::Pass);
        assert_eq!(machine.on_key(LWIN, true, false), Action::Pass);
        assert_eq!(
            machine.on_key(LWIN, false, false),
            Action::Open,
            "E came before this press, so it isn't a combination"
        );
        // The same epoch again changes nothing.
        machine.on_key(LWIN, true, false);
        machine.sync(1);
        assert_eq!(machine.on_key(LWIN, false, false), Action::Open);
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
