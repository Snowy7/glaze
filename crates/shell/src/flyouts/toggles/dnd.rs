//! Do not disturb through `CLSID_QuietHoursSettings`, the quiet hours service (twinui) behind
//! Settings' and the notification center's switch.
//!
//! **Undocumented**: neither the class nor `IQuietHoursSettings` is in the SDK. Its first two
//! slots get and put the user-selected profile; Windows 11's "do not disturb" is the
//! priority-only profile, off is unrestricted (the profile ids are the CloudStore keys'
//! `microsoft.quiethoursprofile.*`).
#![allow(non_snake_case)]

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::core::{GUID, HRESULT, HSTRING, PCWSTR, PWSTR};

#[windows::core::interface("6bff4732-81ec-4ffb-ae67-b6c1bc29631f")]
unsafe trait IQuietHoursSettings: windows::core::IUnknown {
    unsafe fn get_UserSelectedProfile(&self, profile: *mut PWSTR) -> HRESULT;
    unsafe fn put_UserSelectedProfile(&self, profile: PCWSTR) -> HRESULT;
}

const QUIET_HOURS_SETTINGS: GUID = GUID::from_u128(0xf53321fa_34f8_4b7f_b9a3_361877cb94cf);
const UNRESTRICTED: &str = "Microsoft.QuietHoursProfile.Unrestricted";
const PRIORITY_ONLY: &str = "Microsoft.QuietHoursProfile.PriorityOnly";

fn settings() -> windows::core::Result<IQuietHoursSettings> {
    // SAFETY: plain COM activation of an out-of-process server; the quick lane runs COM.
    unsafe { CoCreateInstance(&QUIET_HOURS_SETTINGS, None, CLSCTX_ALL) }
}

/// Whether do not disturb is on (any profile but unrestricted), or `None` when the service
/// doesn't answer.
pub fn read() -> Option<bool> {
    let settings = settings().ok()?;
    let mut profile = PWSTR::null();
    // SAFETY: the server allocates the string with CoTaskMemAlloc; it is freed once read.
    unsafe {
        settings.get_UserSelectedProfile(&mut profile).ok().ok()?;
        let id = profile.to_string();
        CoTaskMemFree(Some(profile.0 as *const _));
        Some(!id.ok()?.eq_ignore_ascii_case(UNRESTRICTED))
    }
}

/// Turns do not disturb on (priority only) or off, and reads it back.
pub fn set(on: bool) -> Result<(), String> {
    let settings = settings().map_err(|e| format!("Do not disturb: {e}"))?;
    let id = HSTRING::from(if on { PRIORITY_ONLY } else { UNRESTRICTED });
    // SAFETY: `id` outlives the call.
    unsafe { settings.put_UserSelectedProfile(PCWSTR(id.as_ptr())) }
        .ok()
        .map_err(|e| format!("Do not disturb: {e}"))?;
    super::confirm("do not disturb", on, read())
}
