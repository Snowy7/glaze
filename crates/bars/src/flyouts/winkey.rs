//! The Windows key opens the launcher (docs/bars.md, "Launcher"), and Win+N and Win+A open the
//! notifications and quick settings flyouts, through a low-level keyboard hook (`WH_KEYBOARD_LL`)
//! in our own process: Windows calls it on our thread, nothing is loaded into anyone else's. It's
//! installed while the launcher needs it or scrolling tiling is enabled, on a thread of
//! its own (a hook that answers late is dropped by Windows), and it decides with a few
//! comparisons and atomic loads per key.
//!
//! Only a lone press and release replaces Start, and only Win+N and Win+A (with nothing else
//! held) replace the action center. Opt-in Win+E opens Files. Every other key passes through (Win+Shift+S
//! and the rest work as ever).
//! - The release of a lone press is replayed as the unassigned key `vkE8`, then the release, and
//!   swallowed only once the replay is in. Explorer sees a Windows key combination, so Start
//!   stays closed (AutoHotkey's "menu mask key"), and the launcher opens.
//! - For Win+N and Win+A the letter is swallowed (its press, repeats and release) after the mask
//!   key went in, so Explorer sees a combination and opens neither Start nor its own panel. The
//!   flyout opens on Win's release, replayed exactly like a lone press's: our injected input is
//!   then the last input, which is what lets the bar take the foreground (opening at the letter,
//!   with the user's own Win key still down, Windows refused it: measured).
//!
//! Everything goes through and Explorer acts as usual when a fullscreen app is in front, no bar
//! can open the flyout, the main loop is hung, or the replay failed (so a key never stays down).

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;
use std::sync::{Arc, Mutex, OnceLock};
use glaze_tiling::{Binding, Command};

type TilingHandler = Arc<dyn Fn(Command) + Send + Sync>;
type TilingConfig = (Vec<Binding>, TilingHandler);
static TILING_ENABLED: AtomicBool = AtomicBool::new(false);
static OVERVIEW: AtomicBool = AtomicBool::new(false);
pub fn set_overview(on:bool){OVERVIEW.store(on,Ordering::Relaxed);}
static TILING_CONFIG: OnceLock<Mutex<Option<TilingConfig>>> = OnceLock::new();
/// Reuses the shell's keyboard hook; the handler must only enqueue the command.
pub fn configure_tiling(config: Option<TilingConfig>) {
    TILING_ENABLED.store(config.is_some(), Ordering::Release);
    if config.is_some() || TILING_CONFIG.get().is_some() {
        *TILING_CONFIG.get_or_init(|| Mutex::new(None)).lock().unwrap() = config;
    }
    WinKey::reset();
}
pub fn tiling_enabled() -> bool { TILING_ENABLED.load(Ordering::Acquire) }

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
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

/// What a Windows key shortcut opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The Windows key alone.
    Launcher,
    /// Win+N.
    Notifications,
    /// Win+A.
    Quick,
    /// Win+E, only while the Files experiment is enabled.
    Files,
}

impl Target {
    /// The flyout's name in the page's `open` event.
    pub fn flyout(self) -> &'static str {
        match self {
            Self::Launcher => "launcher",
            Self::Notifications => "notifications",
            Self::Quick => "quick",
            Self::Files => "files",
        }
    }

    /// The key after the Windows key.
    fn of_letter(vk: u16) -> Option<Self> {
        match vk {
            0x4E => Some(Self::Notifications), // N
            0x41 => Some(Self::Quick),         // A
            _ => None,
        }
    }
}

/// What the hook does with one key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pass,
    /// The release of the Windows key, alone or after Win+N / Win+A: swallow it, replay it
    /// masked and open the flyout.
    Open(Target),
    /// The letter of Win+N or Win+A: mask the combination and swallow the letter (the flyout
    /// opens on Win's release). If the hook can't, [`Machine::cancel_swallow`] lets the letter
    /// through.
    Panel(Target),
    /// The rest of a swallowed letter (its repeats and its release).
    Swallow,
}

/// Follows the Windows key: pressed, then whether anything else happened before its release.
#[derive(Debug, Default)]
pub struct Machine {
    /// The Windows key being held (`VK_LWIN` or `VK_RWIN`).
    held: Option<u16>,
    /// Something else was pressed while it was held.
    combo: bool,
    /// The letter of Win+N / Win+A being swallowed, until its release.
    swallowing: Option<u16>,
    /// The flyout that letter asked for, opened when Win comes up.
    panel: Option<Target>,
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
        self.on_key_with_files(vk, down, ours, false)
    }

    /// The optional Files route shares the installed hook and its cancellation semantics.
    pub fn on_key_with_files(&mut self, vk: u16, down: bool, ours: bool, files: bool) -> Action {
        if ours {
            return Action::Pass;
        }
        if self.swallowing == Some(vk) {
            if !down {
                self.swallowing = None;
            }
            return Action::Swallow;
        }
        match (self.held, down) {
            (Some(held), true) if vk == held => Action::Pass, // auto-repeat
            (Some(_), true) => {
                // The first key after the Windows key, if it's N or A: the flyout, not the panel.
                let panel = if self.combo {
                    None
                } else {
                    Target::of_letter(vk).or_else(|| (files && vk == 0x45).then_some(Target::Files))
                };
                self.combo = true;
                match panel {
                    Some(target) => {
                        self.swallowing = Some(vk);
                        self.panel = Some(target);
                        Action::Panel(target)
                    }
                    None => Action::Pass,
                }
            }
            (Some(held), false) if vk == held => {
                let lone = !self.combo;
                let panel = self.panel.take();
                self.held = None;
                self.combo = false;
                match (lone, panel) {
                    (true, _) => Action::Open(Target::Launcher),
                    (false, Some(target)) => Action::Open(target),
                    (false, None) => Action::Pass,
                }
            }
            (None, true) if is_win(vk) => {
                self.held = Some(vk);
                self.combo = false;
                Action::Pass
            }
            _ => Action::Pass,
        }
    }

    /// The hook couldn't open the flyout for the [`Action::Panel`] it was just given: the letter
    /// goes through to Explorer after all, with its repeats and release.
    pub fn cancel_swallow(&mut self) {
        self.swallowing = None;
        self.panel = None;
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
static FILES: AtomicBool = AtomicBool::new(false);
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

    /// Updated by config reload, never by doing I/O inside the keyboard callback.
    pub fn set_files_enabled(on: bool) {
        if FILES.swap(on, Ordering::Relaxed) != on {
            Self::reset();
        }
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
        let tiling = tiling_enabled();
        if tiling && !ours {
            HOOK.with(|hook| {
                if let Ok(mut hook) = hook.try_borrow_mut() {
                    hook.0.sync(EPOCH.load(Ordering::Relaxed));
                }
            });
            // Core owns panic recovery, including while the overview has keyboard capture.
            if key.vkCode == 0x08
                && [VK_CONTROL, VK_SHIFT, VK_MENU].iter().all(|vk| {
                    // SAFETY: read-only modifier state; this never generates input.
                    unsafe { GetAsyncKeyState(i32::from(vk.0)) < 0 }
                })
            {
                // SAFETY: forwarding the unchanged hook event.
                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }
            if OVERVIEW.load(Ordering::Relaxed) && launcher_can_open() && !is_win(key.vkCode as u16) {
                if key.vkCode == 0x1B && down {
                    if let Some(config) = TILING_CONFIG.get().and_then(|c| c.try_lock().ok())
                        && let Some((_, handler)) = &*config
                    {
                        handler(Command::Overview);
                    }
                }
                let win = HOOK.with(|h| h.try_borrow().is_ok_and(|h| h.0.held.is_some()));
                if !win || key.vkCode == 0x1B { return LRESULT(1); }
            }
            // Classify and enqueue only. A hung shell must leave Windows' keys available.
            if down && launcher_can_open() {
                let win = HOOK.with(|hook| hook.try_borrow().is_ok_and(|h| h.0.held.is_some()));
                if win && !is_win(key.vkCode as u16) {
                    // SAFETY: cached keyboard state, without messaging another process.
                    let held = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(vk.0)) < 0 };
                    let command = TILING_CONFIG.get().and_then(|c| c.try_lock().ok()).and_then(|c| {
                        c.as_ref().and_then(|(bindings, handler)| {
                            bindings.iter().find(|b| b.key == key.vkCode as u16
                                && b.ctrl == held(VK_CONTROL) && b.shift == held(VK_SHIFT)
                                && b.alt == held(VK_MENU)).map(|b| (b.command, handler.clone()))
                        })
                    });
                    // Win+arrow remains ours after remapping. Failed masking falls through
                    // the normal state machine, so neither a command nor a stuck key results.
                    if (command.is_some() || (0x25..=0x28).contains(&key.vkCode)) && send_mask() {
                        HOOK.with(|hook| {
                            if let Ok(mut h) = hook.try_borrow_mut() {
                                h.0.combo = true;
                                h.0.swallowing = Some(key.vkCode as u16);
                            }
                        });
                        if let Some((command, handler)) = command { handler(command); }
                        return LRESULT(1);
                    }
                }
            }
        }
        let action = HOOK.with(|hook| {
            let Ok(mut hook) = hook.try_borrow_mut() else {
                return Action::Pass;
            };
            hook.0.sync(EPOCH.load(Ordering::Relaxed));
            if !ENABLED.load(Ordering::Relaxed) && !tiling {
                return Action::Pass;
            }
            let action = hook.0.on_key_with_files(key.vkCode as u16, down, ours, FILES.load(Ordering::Relaxed));
            if !ENABLED.load(Ordering::Relaxed) && action != Action::Swallow { Action::Pass } else { action }
        });
        if action != Action::Pass || is_win(key.vkCode as u16) {
            log::debug!(
                "Windows key hook: vk {:#x} down={down} ours={ours}: {action:?}",
                key.vkCode
            );
        }
        match action {
            // Swallowed; the replay carries the release.
            Action::Open(target) if launcher_can_open() && replay_masked(key) => {
                open(target);
                return LRESULT(1);
            }
            Action::Panel(_) => {
                // Shift+Win+N and the like are Explorer's, even though Win went down first.
                if launcher_can_open() && !modifiers_held() && send_mask() {
                    return LRESULT(1);
                }
                HOOK.with(|hook| {
                    if let Ok(mut hook) = hook.try_borrow_mut() {
                        hook.0.cancel_swallow();
                    }
                });
            }
            Action::Swallow => return LRESULT(1),
            Action::Open(_) | Action::Pass => {}
        }
    }
    // SAFETY: passes the event on, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Tells the main loop to open `target`'s flyout.
fn open(target: Target) {
    HOOK.with(|hook| {
        if let Ok(hook) = hook.try_borrow()
            && let Some(waker) = &hook.1
        {
            waker.send(Msg::Flyout(Done::WinKey(Instant::now(), target)));
        }
    });
}

/// Is Shift, Ctrl or Alt down?
fn modifiers_held() -> bool {
    [VK_SHIFT, VK_CONTROL, VK_MENU].iter().any(|vk| {
        // SAFETY: no preconditions.
        unsafe { GetAsyncKeyState(i32::from(vk.0)) < 0 }
    })
}

/// A key press or release of ours (`OURS` in its extra info).
fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
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
    }
}

/// Whether all of `inputs` went in.
fn send(inputs: &[INPUT]) -> bool {
    // SAFETY: a valid input array and its element size.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

/// The mask key's press and release: Explorer sees the Windows key held with another key.
fn send_mask() -> bool {
    send(&[
        key_input(MASK, 0, KEYBD_EVENT_FLAGS(0)),
        key_input(MASK, 0, KEYEVENTF_KEYUP),
    ])
}

/// The mask key's press and release, then the Windows key's release as it came (its scan code
/// and extended flag). Whether all three went in: if not, the real release must pass.
fn replay_masked(release: &KBDLLHOOKSTRUCT) -> bool {
    let extended = if release.flags.0 & LLKHF_EXTENDED.0 != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    send(&[
        key_input(MASK, 0, KEYBD_EVENT_FLAGS(0)),
        key_input(MASK, 0, KEYEVENTF_KEYUP),
        key_input(
            VIRTUAL_KEY(release.vkCode as u16),
            release.scanCode as u16,
            KEYEVENTF_KEYUP | extended,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const LWIN: u16 = VK_LWIN.0;
    const RWIN: u16 = VK_RWIN.0;
    const E: u16 = 0x45;
    const SHIFT: u16 = 0x10;
    const OPEN: Action = Action::Open(Target::Launcher);

    #[test]
    fn files_is_opt_in_and_cancellable() {
        let mut machine = Machine::default();
        assert_eq!(machine.on_key_with_files(LWIN, true, false, true), Action::Pass);
        assert_eq!(machine.on_key_with_files(E, true, false, true), Action::Panel(Target::Files));
        assert_eq!(machine.on_key_with_files(E, true, false, true), Action::Swallow);
        assert_eq!(machine.on_key_with_files(E, false, false, true), Action::Swallow);
        assert_eq!(machine.on_key_with_files(LWIN, false, false, true), Action::Open(Target::Files));
        machine.on_key_with_files(LWIN, true, false, true);
        machine.on_key_with_files(E, true, false, true);
        machine.cancel_swallow();
        assert_eq!(machine.on_key_with_files(E, false, false, true), Action::Pass);
        assert_eq!(machine.on_key_with_files(LWIN, false, false, true), Action::Pass);
        for (vk, down) in [(LWIN, true), (E, true), (E, false), (LWIN, false)] {
            assert_eq!(machine.on_key_with_files(vk, down, false, false), Action::Pass);
        }
    }

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
            [Action::Pass, OPEN]
        );
        assert_eq!(
            run(&[
                (RWIN, true, false),
                (RWIN, true, false),
                (RWIN, false, false)
            ]),
            [Action::Pass, Action::Pass, OPEN],
            "auto-repeat is still a lone press"
        );
    }

    const N: u16 = 0x4E;
    const A: u16 = 0x41;

    #[test]
    fn win_n_and_win_a_swallow_the_letter_and_open_their_flyouts_on_the_release() {
        use Action::{Open, Panel, Pass, Swallow};
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (N, true, false),
                (N, true, false), // auto-repeat
                (N, false, false),
                (LWIN, false, false)
            ]),
            [
                Pass,
                Panel(Target::Notifications),
                Swallow,
                Swallow,
                Open(Target::Notifications)
            ],
            "the letter goes; Win's release opens the flyout, not the launcher"
        );
        assert_eq!(
            run(&[
                (RWIN, true, false),
                (A, true, false),
                (A, false, false),
                (RWIN, false, false)
            ]),
            [Pass, Panel(Target::Quick), Swallow, Open(Target::Quick)]
        );
        // Win let go before the letter: the letter's release is still swallowed.
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (A, true, false),
                (LWIN, false, false),
                (A, false, false)
            ]),
            [Pass, Panel(Target::Quick), Open(Target::Quick), Swallow]
        );
        // The next lone press is the launcher again.
        assert_eq!(
            run(&[
                (LWIN, true, false),
                (N, true, false),
                (N, false, false),
                (LWIN, false, false),
                (LWIN, true, false),
                (LWIN, false, false)
            ])[5],
            OPEN
        );
    }

    #[test]
    fn only_the_first_key_after_the_windows_key_can_be_a_panel() {
        // Another key first (Win+E+N), or Win+Shift+N: Explorer's.
        assert_eq!(
            run(&[(LWIN, true, false), (E, true, false), (N, true, false)])[2],
            Action::Pass
        );
        assert_eq!(
            run(&[(LWIN, true, false), (SHIFT, true, false), (N, true, false)])[2],
            Action::Pass
        );
        // N alone, or our own N, is nobody's business.
        assert_eq!(
            run(&[(N, true, false), (N, false, false)]),
            [Action::Pass; 2]
        );
        assert_eq!(
            run(&[(LWIN, true, false), (N, true, true), (N, false, true)])[1..],
            [Action::Pass; 2]
        );
    }

    #[test]
    fn a_panel_that_cannot_open_lets_the_letter_through() {
        let mut machine = Machine::default();
        machine.on_key(LWIN, true, false);
        assert_eq!(machine.on_key(A, true, false), Action::Panel(Target::Quick));
        machine.cancel_swallow();
        assert_eq!(machine.on_key(A, false, false), Action::Pass);
        assert_eq!(
            machine.on_key(LWIN, false, false),
            Action::Pass,
            "no flyout was asked for, and it isn't a lone press"
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
            OPEN
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
            OPEN,
            "E came before this press, so it isn't a combination"
        );
        // The same epoch again changes nothing.
        machine.on_key(LWIN, true, false);
        machine.sync(1);
        assert_eq!(machine.on_key(LWIN, false, false), OPEN);
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
            [Action::Pass, Action::Pass, OPEN]
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
            OPEN
        );
    }
}
