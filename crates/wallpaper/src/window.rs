//! Native windows the wallpaper renders into, and the frame-pacing waiter.

use std::time::Instant;

use anyhow::{Context, Result};
use mypaper_desktop::{DesktopHost, EXIT_RESTART, Rect};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_CONSOLE_DISPLAY_STATE};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, GetCurrentThreadId, INFINITE,
    SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow,
    GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, HWND_MESSAGE, IDC_ARROW, IsIconic,
    LoadCursorW, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PBT_POWERSETTINGCHANGE,
    PostQuitMessage, PostThreadMessageW, QS_ALLINPUT, RegisterClassExW, SetWindowLongPtrW, WM_APP,
    WM_DESTROY, WM_ERASEBKGND, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_CHILD,
    WS_EX_NOACTIVATE, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{GUID, HSTRING, PCWSTR, w};

use crate::hold::Fact;

/// Posted to the main thread when theme files change.
pub const WM_RELOAD: u32 = WM_APP + 1;
/// Posted to the main thread when a system condition changes: `wParam` is a
/// [`Fact`] (see [`Fact::to_wparam`]), `lParam` is 1 or 0.
pub const WM_FACT: u32 = WM_APP + 2;
/// Posted to the main thread when `config.json` changes.
pub const WM_CONFIG: u32 = WM_APP + 3;

const CLASS: PCWSTR = w!("mypaper.wallpaper");
const CONTROL_CLASS: PCWSTR = w!("mypaper.wallpaper.control");
const DEV_WINDOW: isize = 1;

pub fn register_class() -> Result<()> {
    register(CLASS, wndproc)?;
    register(CONTROL_CLASS, control_proc)
}

fn register(
    name: PCWSTR,
    proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static wndproc and class name.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: name,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

/// An owned window, destroyed on drop.
pub struct Window(pub HWND);

impl Window {
    /// A borderless child of the desktop, covering `monitor`, behind the icons.
    pub fn desktop(host: &DesktopHost, monitor: Rect) -> Result<Self> {
        // SAFETY: class registered by `register_class`; parent is a live shell window.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                CLASS,
                w!("mypaper"),
                WS_CHILD,
                0,
                0,
                monitor.width,
                monitor.height,
                Some(host.parent()),
                None,
                None,
                None,
            )?
        };
        let window = Self(hwnd);
        host.place(hwnd, monitor)?;
        Ok(window)
    }

    /// A normal resizable window, for developing without touching the desktop.
    pub fn dev(title: &str) -> Result<Self> {
        // SAFETY: class registered by `register_class`.
        let hwnd = unsafe {
            let hwnd = CreateWindowExW(
                Default::default(),
                CLASS,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                720,
                None,
                None,
                None,
                None,
            )?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, DEV_WINDOW);
            hwnd
        };
        Ok(Self(hwnd))
    }

    pub fn client_size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        // SAFETY: valid window; `rect` outlives the call.
        let _ = unsafe { GetClientRect(self.0, &mut rect) };
        (
            (rect.right - rect.left).max(0) as u32,
            (rect.bottom - rect.top).max(0) as u32,
        )
    }

    pub fn minimized(&self) -> bool {
        // SAFETY: read-only query.
        unsafe { IsIconic(self.0) }.as_bool()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: we own this window and drop it on its creating thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: standard window procedure, called by the system on our thread.
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1), // the GPU paints everything; avoid flicker
            WM_DESTROY => {
                // Closing the dev window quits. A desktop window dying means
                // Explorer took its parent down: ask to be restarted.
                let dev = GetWindowLongPtrW(hwnd, GWLP_USERDATA) == DEV_WINDOW;
                PostQuitMessage(if dev { 0 } else { EXIT_RESTART.into() });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// An invisible message-only window (it never shows or takes focus) that
/// receives power and session notifications and forwards them to the main
/// thread as [`WM_FACT`]. Must be created, and its messages pumped, on the
/// main thread.
pub struct ControlWindow {
    hwnd: HWND,
    power: Vec<HPOWERNOTIFY>,
    session: bool,
}

impl ControlWindow {
    pub fn new() -> Result<Self> {
        // SAFETY: class registered by `register_class`; HWND_MESSAGE makes a message-only window.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                CONTROL_CLASS,
                w!("mypaper control"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?
        };
        // Missing notifications only cost us a pause reason: warn, don't fail.
        let power = [GUID_ACDC_POWER_SOURCE, GUID_CONSOLE_DISPLAY_STATE]
            .iter()
            .filter_map(|guid| {
                // SAFETY: valid window and static GUID; the handle is unregistered on drop.
                unsafe {
                    RegisterPowerSettingNotification(
                        HANDLE(hwnd.0),
                        guid,
                        DEVICE_NOTIFY_WINDOW_HANDLE,
                    )
                }
                .inspect_err(|e| log::warn!("power notifications unavailable: {e}"))
                .ok()
            })
            .collect();
        // SAFETY: valid window of this thread; unregistered on drop.
        let session = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }
            .inspect_err(|e| log::warn!("session lock notifications unavailable: {e}"))
            .is_ok();
        Ok(Self {
            hwnd,
            power,
            session,
        })
    }
}

impl Drop for ControlWindow {
    fn drop(&mut self) {
        // SAFETY: we registered these handles on this window and own it, on its creating thread.
        unsafe {
            for handle in self.power.drain(..) {
                let _ = UnregisterPowerSettingNotification(handle);
            }
            if self.session {
                let _ = WTSUnRegisterSessionNotification(self.hwnd);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// What a power-setting broadcast means for us, given its 32-bit payload.
fn power_fact(setting: &GUID, value: u32) -> Option<(Fact, bool)> {
    if *setting == GUID_ACDC_POWER_SOURCE {
        // 0 = AC, 1 = battery, 2 = short-term (UPS), which still has mains behind it.
        Some((Fact::OnBattery, value == 1))
    } else if *setting == GUID_CONSOLE_DISPLAY_STATE {
        // 0 = off, 1 = on, 2 = dimmed (still visible).
        Some((Fact::DisplayOff, value == 0))
    } else {
        None
    }
}

fn post_fact(fact: Fact, value: bool) {
    // SAFETY: the window procedure runs on the main thread, which pumps these.
    let _ = unsafe {
        PostThreadMessageW(
            GetCurrentThreadId(),
            WM_FACT,
            WPARAM(fact.to_wparam()),
            LPARAM(value.into()),
        )
    };
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_POWERSETTINGCHANGE => {
            // SAFETY: for PBT_POWERSETTINGCHANGE, lparam points at a
            // POWERBROADCAST_SETTING followed by `DataLength` bytes of payload.
            let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
            if setting.DataLength as usize >= size_of::<u32>() {
                // SAFETY: the payload is at least 4 bytes; it may be unaligned.
                let value = unsafe { setting.Data.as_ptr().cast::<u32>().read_unaligned() };
                if let Some((fact, on)) = power_fact(&setting.PowerSetting, value) {
                    post_fact(fact, on);
                }
            }
            LRESULT(1)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 as u32 {
                WTS_SESSION_LOCK => post_fact(Fact::SessionLocked, true),
                WTS_SESSION_UNLOCK => post_fact(Fact::SessionLocked, false),
                _ => {}
            }
            LRESULT(0)
        }
        // SAFETY: default handling of a message we were called with.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Sleeps the main thread until a deadline or a window message, whichever
/// comes first. Uses a high-resolution waitable timer, so frame pacing is
/// precise without spinning (plain `Sleep` rounds to ~15.6ms).
pub struct Waiter {
    timer: HANDLE,
}

impl Waiter {
    pub fn new() -> Result<Self> {
        // SAFETY: creates an unnamed timer we own.
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                PCWSTR::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )?
        };
        Ok(Self { timer })
    }

    /// `None` sleeps until the next message: zero CPU while fully paused.
    pub fn wait(&self, deadline: Option<Instant>) {
        // SAFETY: the timer handle is valid for our lifetime.
        unsafe {
            let handles = match deadline {
                Some(deadline) => {
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return;
                    };
                    let due = -((left.as_nanos() / 100) as i64).max(1); // relative, 100ns units
                    let _ = SetWaitableTimer(self.timer, &due, 0, None, None, false);
                    Some(std::slice::from_ref(&self.timer))
                }
                None => None,
            };
            MsgWaitForMultipleObjectsEx(handles, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        let _ = unsafe { CloseHandle(self.timer) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_source_maps_to_battery() {
        let source = GUID_ACDC_POWER_SOURCE;
        assert_eq!(power_fact(&source, 0), Some((Fact::OnBattery, false)));
        assert_eq!(power_fact(&source, 1), Some((Fact::OnBattery, true)));
        assert_eq!(power_fact(&source, 2), Some((Fact::OnBattery, false)));
    }

    #[test]
    fn only_display_off_counts_as_off() {
        let display = GUID_CONSOLE_DISPLAY_STATE;
        assert_eq!(power_fact(&display, 0), Some((Fact::DisplayOff, true)));
        assert_eq!(power_fact(&display, 1), Some((Fact::DisplayOff, false)));
        assert_eq!(power_fact(&display, 2), Some((Fact::DisplayOff, false)));
        assert_eq!(power_fact(&GUID::zeroed(), 0), None);
    }
}
