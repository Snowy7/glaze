//! CPU sampling math, kept pure so it's tested.

use std::collections::HashMap;
use std::time::Duration;

/// A process as one CPU sample saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Times {
    /// Creation time (FILETIME ticks): with the PID, what identifies the process, since PIDs
    /// are reused.
    pub created: u64,
    /// Kernel plus user time so far, in 100 ns ticks.
    pub busy: u64,
}

/// CPU use over a window.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
pub struct CpuUse {
    /// Percent of the whole machine (every logical CPU), what Task Manager shows.
    pub machine: f64,
    /// Percent of one logical CPU.
    pub one_core: f64,
}

/// `busy` CPU time spent during `wall` on a machine with `logical_cpus`.
pub fn cpu_use(busy: Duration, wall: Duration, logical_cpus: u32) -> CpuUse {
    if wall.is_zero() {
        return CpuUse::default();
    }
    let one_core = busy.as_secs_f64() / wall.as_secs_f64() * 100.0;
    CpuUse {
        machine: one_core / f64::from(logical_cpus.max(1)),
        one_core,
    }
}

/// CPU time the processes in `after` spent since `before` was taken. A process in both counts
/// its difference; one that started in between counts everything it used. One that exited in
/// between is lost (a gone process's times can't be read), which the harness avoids by
/// sampling a settled tree.
pub fn busy_between(before: &HashMap<u32, Times>, after: &HashMap<u32, Times>) -> Duration {
    let ticks: u64 = after
        .iter()
        .map(|(pid, now)| match before.get(pid) {
            Some(then) if then.created == now.created => now.busy.saturating_sub(then.busy),
            _ => now.busy,
        })
        .sum();
    Duration::from_nanos(ticks * 100)
}

/// The median, or `None` for no values.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[mid]),
        _ => Some((sorted[mid - 1] + sorted[mid]) / 2.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(created: u64, busy_ms: u64) -> Times {
        Times {
            created,
            busy: busy_ms * 10_000,
        }
    }

    #[test]
    fn a_tenth_of_a_core_on_sixteen_cpus() {
        let used = cpu_use(Duration::from_millis(1000), Duration::from_secs(10), 16);
        assert!((used.one_core - 10.0).abs() < 1e-9);
        assert!((used.machine - 0.625).abs() < 1e-9);
        assert_eq!(
            cpu_use(Duration::from_secs(1), Duration::ZERO, 16),
            CpuUse::default()
        );
    }

    #[test]
    fn busy_time_counts_deltas_newcomers_and_not_reused_pids() {
        let before = HashMap::from([(1, times(100, 50)), (2, times(100, 10)), (3, times(5, 7))]);
        let after = HashMap::from([
            (1, times(100, 80)), // +30
            (2, times(200, 4)),  // pid reused by a new process: all of its 4
            (4, times(300, 6)),  // started during the window: 6
        ]); // 3 exited: lost
        assert_eq!(busy_between(&before, &after), Duration::from_millis(40));
    }

    #[test]
    fn medians_of_odd_even_and_empty() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&[]), None);
    }
}
