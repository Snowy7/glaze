//! Windows' short date/time, including the user's Regional format overrides. Formatting in
//! Windows also preserves localized day periods, calendars and literals the token clock lacks.

use serde::Serialize;
use windows::Win32::Globalization::{
    ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx, GetLocaleInfoEx, GetTimeFormatEx, LOCALE_SSHORTDATE,
    LOCALE_SSHORTTIME, LOCALE_STIMEFORMAT, TIME_FORMAT_FLAGS,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::{HSTRING, PCWSTR};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemClock {
    time: String,
    time_with_seconds: String,
    date: String,
}

fn pattern(kind: u32) -> Option<HSTRING> {
    let mut buffer = [0u16; 256];
    // SAFETY: null selects the user's locale, including overrides; the buffer is writable.
    let len = unsafe { GetLocaleInfoEx(PCWSTR::null(), kind, Some(&mut buffer)) };
    (len > 1).then(|| HSTRING::from_wide(&buffer[..len as usize - 1]))
}

/// Read at each tick so changes in Windows Settings apply without restarting the bars.
pub fn system() -> Option<SystemClock> {
    let (short_time, long_time, short_date) = (
        pattern(LOCALE_SSHORTTIME)?,
        pattern(LOCALE_STIMEFORMAT)?,
        pattern(LOCALE_SSHORTDATE)?,
    );
    // SAFETY: read-only OS queries, with valid patterns and buffers that outlive each call.
    unsafe {
        let local = GetLocalTime();
        let time = |format: &HSTRING| {
            let mut buffer = [0u16; 256];
            let len = GetTimeFormatEx(
                PCWSTR::null(),
                TIME_FORMAT_FLAGS(0),
                Some(&local),
                format,
                Some(&mut buffer),
            );
            (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
        };
        let mut buffer = [0u16; 256];
        let len = GetDateFormatEx(
            PCWSTR::null(),
            ENUM_DATE_FORMATS_FLAGS(0),
            Some(&local),
            &short_date,
            Some(&mut buffer),
            PCWSTR::null(),
        );
        Some(SystemClock {
            time: time(&short_time)?,
            time_with_seconds: time(&long_time)?,
            date: (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))?,
        })
    }
}
