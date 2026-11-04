//! Startup markers for the perf harness (`tools/perf`, docs/performance.md).
//!
//! Each process logs one line per milestone, measured from its own creation time
//! (`GetProcessTimes`), not from `main`, so loader and runtime start-up count too:
//!
//! ```text
//! READY component=wallpaper first_frame_ms=84.2 epoch_us=1793612345678901
//! ```
//!
//! `epoch_us` is the wall clock (Unix microseconds) when the milestone happened, which lets the
//! harness place every component's milestone on core's timeline.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// 100 ns ticks between 1601-01-01 (FILETIME) and 1970-01-01 (Unix).
const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}

/// Now, in FILETIME ticks (100 ns since 1601).
fn now_ticks() -> u64 {
    // SAFETY: no preconditions.
    ticks(unsafe { GetSystemTimePreciseAsFileTime() })
}

/// When this process was created, in FILETIME ticks.
fn created_ticks() -> Option<u64> {
    let mut created = FILETIME::default();
    let mut unused = [FILETIME::default(); 3];
    let [exited, kernel, user] = &mut unused;
    // SAFETY: the pseudo handle of our own process; every out-pointer outlives the call.
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, exited, kernel, user) }.ok()?;
    Some(ticks(created))
}

/// How long ago this process was created.
pub fn process_age() -> Duration {
    let age = created_ticks().map_or(0, |created| now_ticks().saturating_sub(created));
    Duration::from_nanos(age * 100)
}

/// Logs `READY component=<component> <metric>=<ms since process creation>` the first time it's
/// called for this pair; later calls only take a lock, so call it from places that run a
/// handful of times, not per frame.
pub fn ready(component: &'static str, metric: &'static str) {
    static SEEN: Mutex<BTreeSet<(&str, &str)>> = Mutex::new(BTreeSet::new());
    let first = SEEN
        .lock()
        .map(|mut seen| seen.insert((component, metric)))
        .unwrap_or(false);
    if first {
        let now = now_ticks();
        let age_ms = process_age().as_secs_f64() * 1000.0;
        let epoch_us = now.saturating_sub(UNIX_EPOCH_TICKS) / 10;
        log::info!("READY component={component} {metric}={age_ms:.1} epoch_us={epoch_us}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_process_is_younger_than_a_minute_and_older_than_nothing() {
        let age = process_age();
        assert!(
            age > Duration::ZERO && age < Duration::from_secs(60),
            "{age:?}"
        );
    }

    #[test]
    fn unix_epoch_offset_matches_std() {
        let unix_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        let ours = (now_ticks() - UNIX_EPOCH_TICKS) / 10;
        assert!(ours.abs_diff(unix_now) < 1_000_000, "{ours} vs {unix_now}");
    }
}
