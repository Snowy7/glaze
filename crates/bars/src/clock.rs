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
    pub time: String,
    pub time_with_seconds: String,
    pub date: String,
}

/// Whether the clock `config` draws changes from one second to the next: its format shows the
/// seconds (`seconds` alone only picks Windows' long time when there is no format). A clock that
/// doesn't ticks once a minute.
pub fn shows_seconds(config: &crate::config::ClockConfig) -> bool {
    if config.format.is_none() {
        return config.seconds;
    }
    let at = windows::Win32::Foundation::SYSTEMTIME {
        wYear: 2026,
        wMonth: 1,
        wDayOfWeek: 4,
        wDay: 1,
        wHour: 10,
        wMinute: 10,
        wSecond: 10,
        wMilliseconds: 0,
    };
    let next = windows::Win32::Foundation::SYSTEMTIME { wSecond: 11, ..at };
    format(config, &at) != format(config, &next)
}

/// The former page's token clock, localized with Windows NLS. Quoted text stays literal.
pub fn format(
    config: &crate::config::ClockConfig,
    local: &windows::Win32::Foundation::SYSTEMTIME,
) -> String {
    let Some(pattern) = &config.format else {
        return system().map_or_else(String::new, |s| {
            format!(
                "{}\n{}",
                if config.seconds {
                    s.time_with_seconds
                } else {
                    s.time
                },
                s.date
            )
        });
    };
    const TOKENS: &[&str] = &[
        "dddd", "MMMM", "yyyy", "ddd", "MMM", "dd", "MM", "yy", "HH", "hh", "mm", "ss", "tt", "d",
        "M", "H", "h",
    ];
    let mut rest = pattern.as_str();
    let mut result = String::new();
    let mut quoted = false;
    while !rest.is_empty() {
        if rest.starts_with("''") {
            result.push('\'');
            rest = &rest[2..];
            continue;
        }
        if rest.starts_with('\'') {
            quoted = !quoted;
            rest = &rest[1..];
            continue;
        }
        if !quoted && let Some(&token) = TOKENS.iter().find(|&&t| rest.starts_with(t)) {
            let mut buffer = [0u16; 128];
            let pattern = HSTRING::from(token);
            // SAFETY: local time, pattern and output buffer remain valid throughout the call.
            let len = unsafe {
                if token.starts_with(['d', 'M', 'y']) {
                    GetDateFormatEx(
                        PCWSTR::null(),
                        ENUM_DATE_FORMATS_FLAGS(0),
                        Some(local),
                        &pattern,
                        Some(&mut buffer),
                        PCWSTR::null(),
                    )
                } else {
                    GetTimeFormatEx(
                        PCWSTR::null(),
                        TIME_FORMAT_FLAGS(0),
                        Some(local),
                        &pattern,
                        Some(&mut buffer),
                    )
                }
            };
            if len > 1 {
                result.push_str(&String::from_utf16_lossy(&buffer[..len as usize - 1]));
            }
            rest = &rest[token.len()..];
        } else {
            let c = rest.chars().next().unwrap();
            result.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ClockConfig;

    #[test]
    fn only_a_format_with_seconds_ticks_every_second() {
        let clock = |seconds, format: Option<&str>| ClockConfig {
            seconds,
            format: format.map(str::to_owned),
        };
        assert!(shows_seconds(&clock(false, Some("HH:mm:ss"))));
        assert!(
            !shows_seconds(&clock(true, Some("HH:mm"))),
            "a preset's minute format wins over `seconds`"
        );
        assert!(!shows_seconds(&clock(true, Some("'ss' HH:mm"))), "quoted text is literal");
        assert!(shows_seconds(&clock(true, None)));
        assert!(!shows_seconds(&clock(false, None)));
    }
}
