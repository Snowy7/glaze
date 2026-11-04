//! Wall-clock deadlines follow the least significant visible digit, including across suspension.
use mypaper_ui::calendar::Date;
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
            text.push_str(if self.hour < 12 { " AM" } else { " PM" });
        }
        text
    }
    pub fn midnight_after(self) -> i64 {
        local_epoch(self.date.add_days(1), 0, 0).unwrap_or(self.ms + 86_400_000)
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
