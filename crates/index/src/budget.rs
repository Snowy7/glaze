//! Cooperative thread CPU budget for offline catch-up, including generation replacement.
//! Thread-local state keeps live notification delivery and queries outside the budget.
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

thread_local! {
    static PACER: RefCell<Option<Pacer>> = const { RefCell::new(None) };
}

struct Pacer {
    wall: Instant,
    checked: Instant,
    cpu: Duration,
}

fn thread_cpu() -> Option<Duration> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: pseudo-handle for this thread; all four output structures are writable.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

/// Sleep debt required to keep one worker near 25% of one logical CPU.
fn debt(cpu: Duration, wall: Duration) -> Duration {
    cpu.saturating_mul(4).saturating_sub(wall)
}

pub(crate) struct Budget;
impl Budget {
    pub(crate) fn enter() -> Self {
        PACER.with_borrow_mut(|slot| {
            assert!(slot.is_none(), "nested catch-up budget");
            *slot = Some(Pacer {
                wall: Instant::now(),
                checked: Instant::now(),
                cpu: thread_cpu().unwrap_or_default(),
            });
        });
        Self
    }
    pub(crate) fn active() -> bool {
        PACER.with_borrow(|slot| slot.is_some())
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        checkpoint();
        PACER.with_borrow_mut(|slot| *slot = None);
    }
}

pub(crate) fn checkpoint() {
    PACER.with_borrow_mut(|slot| {
        let Some(pacer) = slot else { return };
        // Avoid a kernel accounting call at every record/window boundary.
        if pacer.checked.elapsed() < Duration::from_millis(10) {
            return;
        }
        let current = thread_cpu();
        let cpu = current.map_or(pacer.wall.elapsed(), |at| at.saturating_sub(pacer.cpu));
        let sleep = debt(cpu, pacer.wall.elapsed());
        if !sleep.is_zero() {
            std::thread::sleep(sleep)
        }
        // Do not accumulate idle/game time as credit for a later CPU burst.
        if pacer.wall.elapsed() >= Duration::from_millis(250) {
            pacer.wall = Instant::now();
            pacer.cpu = current.unwrap_or_default();
        }
        pacer.checked = Instant::now();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quarter_core_budget_counts_waiting_and_cannot_borrow_future_cpu() {
        assert_eq!(
            debt(Duration::from_millis(20), Duration::from_millis(20)),
            Duration::from_millis(60)
        );
        assert_eq!(
            debt(Duration::from_millis(20), Duration::from_millis(100)),
            Duration::ZERO
        );
        assert_eq!(debt(Duration::ZERO, Duration::ZERO), Duration::ZERO);
    }
}
