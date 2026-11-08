//! A tray menu borrows the foreground without dismissing the overflow behind it.
//! Hooks only observe clicks/Escape while this interaction exists; they never consume input.

use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{Input::KeyboardAndMouse::VK_ESCAPE, WindowsAndMessaging::*},
};
use windows::core::BOOL;

const POLL: Duration = Duration::from_millis(50);
const STARTUP: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Wait,
    Resume,
    Release,
    Close,
}

#[derive(Default)]
struct Input {
    flyout: isize,
    owner: Option<u32>,
    close: bool,
}
thread_local! { static INPUT: RefCell<Input> = RefCell::new(Input::default()); }

pub(super) struct Hold {
    started: Instant,
    seen: bool,
    visible: Vec<isize>,
    return_to: isize,
    menu_front: Option<isize>,
    resumed: bool,
    mouse: Option<HHOOK>,
    keyboard: Option<HHOOK>,
}

impl Hold {
    pub fn new(flyout: HWND, owner: Option<u32>, return_to: isize) -> Self {
        INPUT.with_borrow_mut(|input| {
            *input = Input {
                flyout: flyout.0 as isize,
                owner,
                close: false,
            }
        });
        let mut visible = Vec::new();
        // SAFETY: enumeration writes only to the local vector; hooks are owned by this UI thread.
        let (mouse, keyboard) = unsafe {
            let _ = EnumWindows(
                Some(visible_window),
                LPARAM((&mut visible as *mut Vec<isize>) as isize),
            );
            (
                SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), None, 0).ok(),
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), None, 0).ok(),
            )
        };
        Self {
            started: Instant::now(),
            seen: false,
            visible,
            return_to,
            menu_front: None,
            resumed: false,
            mouse,
            keyboard,
        }
    }

    pub fn deadline(&self) -> Instant {
        Instant::now() + POLL
    }

    pub fn follow(&mut self) -> Decision {
        let (cancelled, owner, flyout) = INPUT.with_borrow(|i| (i.close, i.owner, i.flyout));
        // SAFETY: read-only window and GUI-thread queries, stale handles simply fail.
        let (front, menu, visible) = unsafe {
            let front = GetForegroundWindow();
            let mut pid = 0;
            let thread = GetWindowThreadProcessId(front, Some(&mut pid));
            let mut gui = GUITHREADINFO {
                cbSize: size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let native = GetGUIThreadInfo(thread, &mut gui).is_ok()
                && (gui.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE)).0 != 0;
            let related = owner.is_none_or(|owner| owner == pid) || pid == std::process::id();
            let custom = pid == std::process::id()
                && front.0 as isize != flyout
                && is_shell_tray_menu(front);
            (
                front,
                related && (native || custom),
                IsWindowVisible(front).as_bool(),
            )
        };
        let front = front.0 as isize;
        let activated = self.seen && self.menu_front != Some(front) && front != self.return_to;
        let newly_shown =
            visible && front != flyout && (activated || !self.visible.contains(&front));
        let mut decision = decide(
            cancelled,
            menu,
            self.seen,
            self.started.elapsed() < STARTUP,
            newly_shown,
        );
        self.seen |= menu;
        if menu {
            self.menu_front = Some(front);
        }
        if decision == Decision::Resume {
            // Windows may deny the foreground handback. Keep observing outside clicks/Escape
            // until focus really returns, rather than dismissing on a delayed deactivation.
            decision = handback(&mut self.resumed, front == flyout);
        }
        decision
    }
}

fn handback(attempted: &mut bool, focused: bool) -> Decision {
    if focused {
        Decision::Release
    } else if *attempted {
        Decision::Wait
    } else {
        *attempted = true;
        Decision::Resume
    }
}

fn decide(cancelled: bool, menu: bool, seen: bool, starting: bool, newly_shown: bool) -> Decision {
    if cancelled {
        Decision::Close
    } else if menu {
        Decision::Wait
    } else if newly_shown {
        Decision::Close
    } else if seen || !starting {
        Decision::Resume
    } else {
        Decision::Wait
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        for hook in [self.mouse.take(), self.keyboard.take()]
            .into_iter()
            .flatten()
        {
            // SAFETY: each hook belongs to this guard and its installing thread.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
        INPUT.with_borrow_mut(|i| *i = Input::default());
    }
}

unsafe extern "system" fn visible_window(window: HWND, data: LPARAM) -> BOOL {
    // SAFETY: caller passes a live Vec for this synchronous enumeration; no windows are changed.
    if unsafe { IsWindowVisible(window) }.as_bool() && !unsafe { IsIconic(window) }.as_bool() {
        unsafe { &mut *(data.0 as *mut Vec<isize>) }.push(window.0 as isize);
    }
    BOOL(1)
}

fn class_is(window: HWND, name: &str) -> bool {
    let mut class = [0u16; 128];
    // SAFETY: read-only class lookup into a local buffer.
    let len = unsafe { GetClassNameW(window, &mut class) }.max(0) as usize;
    class[..len].iter().copied().eq(name.encode_utf16())
}

fn is_shell_tray_menu(window: HWND) -> bool {
    if !class_is(window, "glaze.ui") {
        return false;
    }
    let mut title = [0u16; 64];
    // SAFETY: own-process popup's title, copied into a local buffer without sending messages.
    let len = unsafe { GetWindowTextW(window, &mut title) }.max(0) as usize;
    title[..len]
        .iter()
        .copied()
        .eq("Glaze commands".encode_utf16())
}

unsafe extern "system" fn mouse(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            wp.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: a nonnegative low-level mouse callback carries a live MSLLHOOKSTRUCT.
        let point = unsafe { (*(lp.0 as *const MSLLHOOKSTRUCT)).pt };
        // SAFETY: read-only lookup at the event's point; no input is generated.
        let window = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
        let mut pid = 0;
        // SAFETY: read-only PID query; a vanished window leaves pid zero.
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        INPUT.with_borrow_mut(|input| {
            let ours = window.0 as isize == input.flyout;
            let native = class_is(window, "#32768") && input.owner.is_none_or(|owner| owner == pid);
            let custom = pid == std::process::id() && is_shell_tray_menu(window);
            input.close |= !ours && !native && !custom;
        });
    }
    // SAFETY: observation only; preserve the original hook chain and every input event.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

unsafe extern "system" fn keyboard(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: a nonnegative low-level keyboard callback carries a live KBDLLHOOKSTRUCT.
        let key = unsafe { (*(lp.0 as *const KBDLLHOOKSTRUCT)).vkCode };
        if key == u32::from(VK_ESCAPE.0) {
            INPUT.with_borrow_mut(|i| i.close = true);
        }
    }
    // SAFETY: observation only; Escape still reaches the app's menu.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;

    thread_local! { static PROBE: RefCell<Option<Hold>> = const { RefCell::new(None) }; }
    thread_local! { static OBSERVED: RefCell<Vec<Decision>> = const { RefCell::new(Vec::new()) }; }

    unsafe extern "system" fn probe(window: HWND, _: u32, timer: usize, _: u32) {
        PROBE.with_borrow_mut(|hold| {
            OBSERVED.with_borrow_mut(|seen| seen.push(hold.as_mut().unwrap().follow()))
        });
        // SAFETY: this timer/menu belongs to the test thread; it never generates input.
        unsafe {
            let _ = KillTimer(Some(window), timer);
            let _ = EndMenu();
        }
    }

    /// Own test windows only. Run under the desktop lock; no mouse or keyboard is synthesized.
    #[test]
    #[ignore = "live native menu fixture; requires the desktop lock"]
    fn live_native_menu_with_a_visible_owner_stays_held_then_resumes() {
        // SAFETY: the fixture creates, shows and closes only its own window and native menu.
        unsafe {
            let owner = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Glaze menu test"),
                WS_OVERLAPPEDWINDOW,
                -30000,
                -30000,
                160,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let _ = ShowWindow(owner, SW_SHOWNOACTIVATE);
            let _ = SetForegroundWindow(owner);
            let hold = Hold::new(HWND::default(), Some(std::process::id()), 0);
            assert!(
                hold.mouse.is_some() && hold.keyboard.is_some(),
                "temporary observers must install"
            );
            PROBE.with_borrow_mut(|probe| *probe = Some(hold));
            OBSERVED.with_borrow_mut(Vec::clear);
            let menu = CreatePopupMenu().unwrap();
            AppendMenuW(menu, MF_STRING, 1, w!("Fixture command")).unwrap();
            let timer = SetTimer(Some(owner), 1, 100, Some(probe));
            assert_ne!(timer, 0);
            let _ = TrackPopupMenu(menu, TPM_RETURNCMD, -30000, -30000, None, owner, None);
            let _ = KillTimer(Some(owner), timer);
            OBSERVED.with_borrow(|seen| assert_eq!(seen.as_slice(), &[Decision::Wait]));
            PROBE.with_borrow_mut(|probe| {
                assert_eq!(probe.as_mut().unwrap().follow(), Decision::Resume)
            });
            PROBE.with_borrow_mut(|probe| *probe = None);
            let _ = DestroyMenu(menu);
            let _ = DestroyWindow(owner);
        }
    }

    #[test]
    fn asynchronous_callback_keeps_the_hold_until_menu_start() {
        assert_eq!(decide(false, false, false, true, false), Decision::Wait);
        assert_eq!(decide(false, true, false, true, false), Decision::Wait);
        assert_eq!(decide(false, true, true, false, true), Decision::Wait);
    }

    #[test]
    fn denied_foreground_handback_keeps_observers_until_focus_really_returns() {
        let mut attempted = false;
        assert_eq!(handback(&mut attempted, false), Decision::Resume);
        assert_eq!(handback(&mut attempted, false), Decision::Wait);
        assert_eq!(handback(&mut attempted, true), Decision::Release);
    }

    #[test]
    fn menu_dismissal_resumes_but_an_opened_window_outside_click_or_escape_closes() {
        assert_eq!(decide(false, false, true, false, false), Decision::Resume);
        assert_eq!(decide(false, false, true, false, true), Decision::Close);
        assert_eq!(decide(true, true, true, false, false), Decision::Close);
        assert_eq!(decide(false, false, false, false, false), Decision::Resume);
    }
}
