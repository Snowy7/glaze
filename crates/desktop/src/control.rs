//! The invisible window that hears about power and session changes.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_CONSOLE_DISPLAY_STATE};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow, HWND_MESSAGE,
    PBT_POWERSETTINGCHANGE, PostThreadMessageW, RegisterClassExW, WM_APP, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSEXW, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{GUID, PCWSTR, w};

use crate::hold::Fact;

/// Posted to the thread that created a [`ControlWindow`] when a system
/// condition changes: `wParam` is a [`Fact`] (see [`Fact::to_wparam`]),
/// `lParam` is 1 or 0.
pub const WM_FACT: u32 = WM_APP + 2;

/// An invisible message-only window (it never shows or takes focus) that
/// receives power and session notifications and forwards them to its thread
/// as [`WM_FACT`]. Create it, and pump its messages, on the main thread.
pub struct ControlWindow {
    hwnd: HWND,
    power: Vec<HPOWERNOTIFY>,
    session: bool,
}

impl ControlWindow {
    /// Creates the window with window class `class`, registered on first use.
    /// The class name is how other processes find the window
    /// (`FindWindowExW(HWND_MESSAGE, …)`).
    pub fn new(class: PCWSTR) -> Result<Self> {
        register(class)?;
        // SAFETY: class registered above; HWND_MESSAGE makes a message-only window.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                class,
                w!("Glaze control"),
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

    pub fn hwnd(&self) -> HWND {
        self.hwnd
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

fn register(class: PCWSTR) -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let info = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(control_proc),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassExW(&info) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
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
    // SAFETY: the window procedure runs on the creating thread, which pumps these.
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
