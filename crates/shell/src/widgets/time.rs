//! Wall-clock deadlines follow the least significant visible digit, including across suspension.
use glaze_ui::calendar::Date;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::{SystemTimeToFileTime, TzSpecificLocalTimeToSystemTime};

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub ms: i64,
}
impl Stamp {
    pub fn at(ms: i64) -> Self {
        let ticks = ((ms + 11_644_473_600_000) * 10_000) as u64;
        let file = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
        // SAFETY: time conversion only, with valid output structures.
        unsafe {
            let _ = windows::Win32::System::Time::FileTimeToSystemTime(&file, &mut utc);
            let _ = windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime(
                None, &utc, &mut local,
            );
        }
        Self {
            date: Date {
                year: local.wYear.into(),
                month: local.wMonth.into(),
                day: local.wDay.into(),
            },
            hour: local.wHour.into(),
            minute: local.wMinute.into(),
            second: local.wSecond.into(),
            ms,
        }
    }
    pub fn now() -> Self {
        // SAFETY: read-only OS clock query.
        let t = unsafe { GetLocalTime() };
        Self {
            date: Date {
                year: t.wYear.into(),
                month: t.wMonth.into(),
                day: t.wDay.into(),
            },
            hour: t.wHour.into(),
            minute: t.wMinute.into(),
            second: t.wSecond.into(),
            ms: epoch_ms(),
        }
    }
    pub fn zone(self, zone: &str) -> Option<Self> {
        let cal = windows::Globalization::Calendar::new().ok()?;
        cal.ChangeTimeZone(&windows::core::HSTRING::from(zone))
            .ok()?;
        cal.SetDateTime(windows::Foundation::DateTime {
            UniversalTime: (self.ms + 11_644_473_600_000) * 10_000,
        })
        .ok()?;
        // Calendar's default clock can be twelve-hour; force the documented 24-hour clock.
        cal.ChangeClock(windows::core::h!("24HourClock")).ok()?;
        Some(Self {
            date: Date {
                year: cal.Year().ok()?,
                month: cal.Month().ok()? as u32,
                day: cal.Day().ok()? as u32,
            },
            hour: cal.Hour().ok()? as u32,
            minute: cal.Minute().ok()? as u32,
            second: self.second,
            ..self
        })
    }
    pub fn clock(self, format24: bool, seconds: bool) -> String {
        let hour = if format24 {
            self.hour
        } else {
            (self.hour + 11) % 12 + 1
        };
        let mut text = format!("{hour:02}:{:02}", self.minute);
        if seconds {
            text.push_str(&format!(":{:02}", self.second));
        }
        if !format24 {
            let period = locale_time(self, "tt");
            text.push(' ');
            text.push_str(&period);
        }
        text
    }
    pub fn midnight_after(self) -> i64 {
        local_epoch(self.date.add_days(1), 0, 0).unwrap_or(self.ms + 86_400_000)
    }
}

/// Windows' user locale, including localized weekday/month names and day-period words.
pub fn date_label(date: Date, long: bool) -> String {
    let mut buffer = [0u16; 256];
    let stamp = SYSTEMTIME {
        wYear: date.year as u16,
        wMonth: date.month as u16,
        wDay: date.day as u16,
        ..Default::default()
    };
    // SAFETY: NLS copies into the bounded buffer; null locale means user default.
    let count = unsafe {
        windows::Win32::Globalization::GetDateFormatEx(
            windows::core::PCWSTR::null(),
            if long {
                windows::Win32::Globalization::DATE_LONGDATE
            } else {
                windows::Win32::Globalization::DATE_SHORTDATE
            },
            Some(&stamp),
            windows::core::PCWSTR::null(),
            Some(&mut buffer),
            windows::core::PCWSTR::null(),
        )
    };
    if count > 1 {
        let label = String::from_utf16_lossy(&buffer[..count as usize - 1]);
        if long {
            // Some user long-date patterns omit the weekday; the clock still promises it.
            let mut weekday = [0u16; 80];
            // SAFETY: same date, user locale and bounded output as above.
            let n = unsafe {
                windows::Win32::Globalization::GetDateFormatEx(
                    windows::core::PCWSTR::null(),
                    windows::Win32::Globalization::ENUM_DATE_FORMATS_FLAGS(0),
                    Some(&stamp),
                    &windows::core::HSTRING::from("dddd"),
                    Some(&mut weekday),
                    windows::core::PCWSTR::null(),
                )
            };
            if n > 1 {
                let weekday = String::from_utf16_lossy(&weekday[..n as usize - 1]);
                if !label.contains(&weekday) {
                    return format!("{weekday}, {label}");
                }
            }
        }
        label
    } else {
        date.key()
    }
}

pub fn greeting(hour: u32) -> &'static str {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SISO639LANGNAME};
    let mut language = [0u16; 16];
    // SAFETY: the user's language code only; no global locale is changed.
    let n = unsafe {
        GetLocaleInfoEx(
            windows::core::PCWSTR::null(),
            LOCALE_SISO639LANGNAME,
            Some(&mut language),
        )
    };
    let language = String::from_utf16_lossy(&language[..n.saturating_sub(1) as usize]);
    let period = match hour {
        5..=11 => 0,
        12..=16 => 1,
        17..=20 => 2,
        _ => 3,
    };
    match language.as_str() {
        "ar" => ["صباح الخير", "مساء الخير", "مساء الخير", "ما زلت مستيقظًا"][period],
        "ja" => [
            "おはようございます",
            "こんにちは",
            "こんばんは",
            "まだ起きていますか",
        ][period],
        "zh" => ["早上好", "下午好", "晚上好", "夜深了"][period],
        "fr" => ["Bonjour", "Bon après-midi", "Bonsoir", "Encore debout"][period],
        "de" => ["Guten Morgen", "Guten Tag", "Guten Abend", "Noch wach"][period],
        "es" => [
            "Buenos días",
            "Buenas tardes",
            "Buenas noches",
            "Aún despierto",
        ][period],
        _ => ["Good morning", "Good afternoon", "Good evening", "Still up"][period],
    }
}
fn locale_time(stamp: Stamp, format: &str) -> String {
    let mut buffer = [0u16; 64];
    let time = SYSTEMTIME {
        wHour: stamp.hour as u16,
        wMinute: stamp.minute as u16,
        wSecond: stamp.second as u16,
        ..Default::default()
    };
    // SAFETY: NLS formatting into our local buffer.
    let count = unsafe {
        windows::Win32::Globalization::GetTimeFormatEx(
            windows::core::PCWSTR::null(),
            windows::Win32::Globalization::TIME_FORMAT_FLAGS(0),
            Some(&time),
            &windows::core::HSTRING::from(format),
            Some(&mut buffer),
        )
    };
    if count > 1 {
        String::from_utf16_lossy(&buffer[..count as usize - 1])
    } else if stamp.hour < 12 {
        "AM".into()
    } else {
        "PM".into()
    }
}
pub fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn local_epoch(date: Date, hour: u32, minute: u32) -> Option<i64> {
    let local = SYSTEMTIME {
        wYear: date.year as u16,
        wMonth: date.month as u16,
        wDay: date.day as u16,
        wHour: hour as u16,
        wMinute: minute as u16,
        ..Default::default()
    };
    let mut utc = SYSTEMTIME::default();
    let mut file = FILETIME::default();
    // SAFETY: all input/out structs are valid across these synchronous conversion calls.
    unsafe {
        TzSpecificLocalTimeToSystemTime(None, &local, &mut utc).ok()?;
        SystemTimeToFileTime(&utc, &mut file).ok()?;
    }
    Some(
        (((u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime)) / 10_000) as i64
            - 11_644_473_600_000,
    )
}
pub fn boundary(now: i64, quantum: i64) -> Duration {
    Duration::from_millis((quantum - now.rem_euclid(quantum)).max(1) as u64)
}
/// How long after a second or minute boundary clocks redraw: a wait can end a little early, and
/// the bars' clocks and the widgets' (and media progress) use the same, so one wake does both.
pub const CLOCK_SLACK: Duration = Duration::from_millis(5);
/// The wait until a clock showing `quantum`-ms steps next changes, plus [`CLOCK_SLACK`].
pub fn clock_tick(now: i64, quantum: i64) -> Duration {
    boundary(now, quantum) + CLOCK_SLACK
}
pub fn remaining_boundary(remaining: i64, quantum: i64) -> Duration {
    Duration::from_millis(((remaining.max(1) - 1).rem_euclid(quantum) + 1) as u64)
}

/// Windows' locale uses Monday=0; our calendar uses Sunday=0.
pub fn week_start() -> u32 {
    use std::sync::OnceLock;
    static START: OnceLock<u32> = OnceLock::new();
    *START.get_or_init(|| {
        let mut value = [0u16; 4];
        // SAFETY: user-default locale and an owned UTF-16 output buffer.
        let n = unsafe {
            windows::Win32::Globalization::GetLocaleInfoEx(
                windows::core::PCWSTR::null(),
                windows::Win32::Globalization::LOCALE_IFIRSTDAYOFWEEK,
                Some(&mut value),
            )
        };
        if n > 1 {
            (u32::from(value[0].saturating_sub(b'0' as u16)) + 1) % 7
        } else {
            1
        }
    })
}

/// Round waits upward. Flooring the final fraction of a millisecond makes the UI busy-spin.
pub fn wait_ms(duration: Duration) -> u32 {
    duration
        .as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadlines_match_the_displayed_digit() {
        assert_eq!(wait_ms(Duration::from_nanos(1)), 1);
        assert_eq!(wait_ms(Duration::from_micros(1001)), 2);
        assert_eq!(wait_ms(Duration::ZERO), 0);
        assert_eq!(boundary(12_345, 60_000), Duration::from_millis(47_655));
        assert_eq!(remaining_boundary(60_000, 1000), Duration::from_secs(1));
        assert_eq!(
            remaining_boundary(59_999, 60_000),
            Duration::from_millis(59_999)
        );
    }
}
