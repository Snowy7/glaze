//! What the bars' clicks do, and the small system readings (battery, CPU, memory).
//!
//! Window activation relies on the foreground rights the user's click on a bar gives our process
//! (the bar windows are activatable, not `WS_EX_NOACTIVATE`), so `SetForegroundWindow` may pass the
//! foreground on to the app.

use std::time::Instant;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LUID, WPARAM};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS, SetSuspendState};
use windows::Win32::System::Shutdown::{
    EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, EXIT_WINDOWS_FLAGS, ExitWindowsEx, LockWorkStation,
    SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetSystemTimes, OpenProcessToken};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LEFT, VK_LWIN, VK_RIGHT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, IsWindow, PostMessageW, SC_MINIMIZE, SC_RESTORE, SC_TASKLIST,
    SW_SHOWNORMAL, SetForegroundWindow, WM_CLOSE, WM_SYSCOMMAND,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::bridge::{Direction, Panel, PowerAction};

pub fn hwnd(raw: isize) -> HWND {
    HWND(raw as _)
}

/// Brings a window to the front, restoring it if it's minimized.
pub fn activate(window: HWND) -> bool {
    // SAFETY: plain calls on a window handle; a stale one just fails.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return false;
        }
        let done = SetForegroundWindow(window).as_bool();
        if IsIconic(window).as_bool() {
            // What the taskbar sends. Chromium-based windows ignore a ShowWindow(SW_RESTORE)
            // from another process, but every app handles its own restore command (posted, so
            // a hung app can't block the bars).
            let _ = PostMessageW(
                Some(window),
                WM_SYSCOMMAND,
                WPARAM(SC_RESTORE as usize),
                LPARAM(0),
            );
        }
        if !done {
            log::info!("Windows refused to bring {window:?} to the front");
        }
        done
    }
}

/// Minimizes a window the way its title bar button does.
pub fn minimize(window: HWND) {
    // SAFETY: posting to a window; a stale handle just fails.
    let _ = unsafe {
        PostMessageW(
            Some(window),
            WM_SYSCOMMAND,
            WPARAM(SC_MINIMIZE as usize),
            LPARAM(0),
        )
    };
}

/// Asks a window to close, as its title bar button does (it may ask to save first).
pub fn close(window: HWND) {
    // SAFETY: posting to a window; a stale handle just fails.
    let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
}

/// Opens a path, `shell:` path or URI with its default handler, off the main thread (shell
/// execution can block on slow shares and shell extensions).
pub fn open(target: String) {
    let spawned = std::thread::Builder::new()
        .name("open".into())
        .spawn(move || {
            // SAFETY: COM for ShellExecute on this short-lived thread; valid strings.
            let opened = unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                let run = |file: &str, args: Option<&str>| {
                    let args = args.map(HSTRING::from);
                    let result = ShellExecuteW(
                        None,
                        w!("open"),
                        &HSTRING::from(file),
                        args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
                        PCWSTR::null(),
                        SW_SHOWNORMAL,
                    );
                    result.0 as isize > 32
                };
                // `shell:AppsFolder\…` items open through Explorer when the direct way fails.
                run(&target, None)
                    || (target.starts_with("shell:") && run("explorer.exe", Some(&target)))
            };
            if !opened {
                log::warn!("couldn't open {target:?}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("open: {e}");
    }
}

/// Opens the Start menu.
pub fn start_menu() {
    // SAFETY: a lookup and a post to the taskbar; without one, the Windows key does it.
    unsafe {
        match FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            Ok(tray) => {
                let _ = PostMessageW(
                    Some(tray),
                    WM_SYSCOMMAND,
                    WPARAM(SC_TASKLIST as usize),
                    LPARAM(0),
                );
            }
            Err(_) => keys(&[VK_LWIN]),
        }
    }
}

pub fn action_center(panel: Panel) {
    open(
        match panel {
            Panel::Notifications => "ms-actioncenter:",
            Panel::Quick => "ms-actioncenter:controlcenter/",
        }
        .into(),
    );
}

/// Switches to the next virtual desktop on the left or right (Win+Ctrl+arrow), as if the user
/// pressed it; only ever sent on the user's click.
pub fn switch_desktop(direction: Direction) {
    let arrow = match direction {
        Direction::Left => VK_LEFT,
        Direction::Right => VK_RIGHT,
    };
    keys(&[VK_LWIN, VK_CONTROL, arrow]);
}

/// Opens Windows Search (Win+S), on the user's click.
pub fn search() {
    keys(&[VK_LWIN, VIRTUAL_KEY(u16::from(b'S'))]);
}

/// Minimizes every window, or brings them back (Win+D), on the user's click.
pub fn show_desktop() {
    keys(&[VK_LWIN, VIRTUAL_KEY(u16::from(b'D'))]);
}

/// Presses `chord` (in order) and releases it (in reverse).
fn keys(chord: &[VIRTUAL_KEY]) {
    let key = |vk: VIRTUAL_KEY, up: bool| {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if vk == VK_LEFT || vk == VK_RIGHT || vk == VK_LWIN {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        }
    };
    let inputs: Vec<INPUT> = chord
        .iter()
        .map(|&vk| key(vk, false))
        .chain(chord.iter().rev().map(|&vk| key(vk, true)))
        .collect();
    // SAFETY: a valid input array and its element size.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Lock, sleep, sign out, restart or shut down. The page asks before restart and shut down.
pub fn power(action: PowerAction) {
    log::info!("power: {action:?}");
    let exit = |flags: EXIT_WINDOWS_FLAGS| {
        enable_shutdown_privilege();
        let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
        // SAFETY: a documented call; the system takes it from here.
        if let Err(e) = unsafe { ExitWindowsEx(flags, reason) } {
            log::warn!("{action:?} failed: {e}");
        }
    };
    match action {
        PowerAction::Lock => {
            // SAFETY: no preconditions.
            let _ = unsafe { LockWorkStation() };
        }
        PowerAction::Sleep => {
            // Returns only after the machine wakes up, so not on the main thread.
            let _ = std::thread::Builder::new().name("sleep".into()).spawn(|| {
                // SAFETY: no preconditions.
                unsafe { SetSuspendState(false, false, false) };
            });
        }
        PowerAction::Signout => exit(EWX_LOGOFF),
        PowerAction::Restart => exit(EWX_REBOOT),
        PowerAction::Shutdown => exit(EWX_POWEROFF),
    }
}

/// Restart and shut down need `SeShutdownPrivilege` enabled on our token.
fn enable_shutdown_privilege() {
    // SAFETY: our own process token, closed below; the struct is fully initialized.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return;
        }
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid).is_ok() {
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let _ = AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None);
        }
        let _ = CloseHandle(token);
    }
}

// --- Readings --------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// The battery, or `None` on a desktop (or when Windows doesn't know).
pub fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` outlives the call.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    battery_of(&status)
}

fn battery_of(status: &SYSTEM_POWER_STATUS) -> Option<Battery> {
    const NO_BATTERY: u8 = 128;
    const UNKNOWN: u8 = 255;
    if status.BatteryFlag == UNKNOWN
        || status.BatteryFlag & NO_BATTERY != 0
        || status.BatteryLifePercent > 100
    {
        return None;
    }
    Some(Battery {
        percent: status.BatteryLifePercent,
        charging: status.ACLineStatus == 1,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stats {
    /// Percent of the whole machine.
    pub cpu: f64,
    pub memory: Memory,
}

/// Bytes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
}

/// CPU usage between calls, and memory now. Exists only while a stats module is on screen.
pub struct Sampler {
    last: (u64, u64),
    pub at: Instant,
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// `(idle, total)` CPU time so far.
fn cpu_times() -> (u64, u64) {
    let (mut idle, mut kernel, mut user) = Default::default();
    // SAFETY: the out-pointers are valid FILETIMEs for the call.
    let _ = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
    (ticks(idle), ticks(kernel) + ticks(user))
}

/// The busy share of the CPU time between two `(idle, total)` readings.
fn cpu_percent((idle0, total0): (u64, u64), (idle1, total1): (u64, u64)) -> f64 {
    let total = total1.saturating_sub(total0);
    if total == 0 {
        return 0.0;
    }
    let idle = idle1.saturating_sub(idle0);
    (100.0 * (1.0 - idle as f64 / total as f64)).clamp(0.0, 100.0)
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            last: cpu_times(),
            at: Instant::now(),
        }
    }

    pub fn sample(&mut self) -> Stats {
        let now = cpu_times();
        let cpu = cpu_percent(self.last, now);
        self.last = now;
        self.at = Instant::now();
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `status` has dwLength set and outlives the call.
        let _ = unsafe { GlobalMemoryStatusEx(&mut status) };
        Stats {
            cpu,
            memory: Memory {
                used: status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
                total: status.ullTotalPhys,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batteries_are_hidden_on_desktops() {
        let status = |flag: u8, percent: u8, ac: u8| SYSTEM_POWER_STATUS {
            ACLineStatus: ac,
            BatteryFlag: flag,
            BatteryLifePercent: percent,
            ..Default::default()
        };
        assert_eq!(battery_of(&status(128, 255, 1)), None, "no battery");
        assert_eq!(battery_of(&status(255, 50, 1)), None, "unknown");
        assert_eq!(
            battery_of(&status(8, 84, 1)),
            Some(Battery {
                percent: 84,
                charging: true
            })
        );
        assert_eq!(
            battery_of(&status(1, 90, 0)),
            Some(Battery {
                percent: 90,
                charging: false
            })
        );
    }

    #[test]
    fn cpu_is_the_busy_share() {
        assert_eq!(cpu_percent((1_000, 2_000), (1_750, 3_000)), 25.0);
        assert_eq!(cpu_percent((5, 5), (5, 5)), 0.0);
    }
}
