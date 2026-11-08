//! Restart policy for supervised processes, kept pure so it is testable.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use glaze_desktop::{EXIT_CONFIG, EXIT_RESTART};

/// Crashes counted within this window decide the backoff.
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// This many crashes inside the window and we stop trying.
const MAX_CRASHES: usize = 5;

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Stop,
    /// Bad configuration: restarting would fail the same way.
    Broken,
    RestartAfter(Duration),
    GiveUp,
}

#[derive(Default)]
pub struct Restarts {
    crashes: VecDeque<Instant>,
}

impl Restarts {
    /// `code` is the exit code, or `None` if the process was killed.
    pub fn on_exit(&mut self, code: Option<i32>, now: Instant) -> Decision {
        match code {
            Some(0) => Decision::Stop,
            Some(c) if c == i32::from(EXIT_CONFIG) => Decision::Broken,
            // The desktop changed (Explorer restart, monitors): give Explorer
            // a moment to rebuild its windows, then reattach.
            Some(c) if c == i32::from(EXIT_RESTART) => {
                Decision::RestartAfter(Duration::from_millis(750))
            }
            _ => {
                self.crashes
                    .retain(|t| now.duration_since(*t) < CRASH_WINDOW);
                self.crashes.push_back(now);
                if self.crashes.len() >= MAX_CRASHES {
                    return Decision::GiveUp;
                }
                Decision::RestartAfter(Duration::from_secs(1 << (self.crashes.len() - 1)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_and_bad_config_stop_while_desktop_change_restarts() {
        let mut r = Restarts::default();
        let now = Instant::now();
        assert_eq!(r.on_exit(Some(0), now), Decision::Stop);
        assert_eq!(r.on_exit(Some(2), now), Decision::Broken);
        assert_eq!(
            r.on_exit(Some(3), now),
            Decision::RestartAfter(Duration::from_millis(750))
        );
    }

    #[test]
    fn crashes_back_off_then_give_up() {
        let mut r = Restarts::default();
        let now = Instant::now();
        let delays: Vec<_> = (0..4).map(|_| r.on_exit(Some(101), now)).collect();
        assert_eq!(
            delays,
            [1, 2, 4, 8].map(|s| Decision::RestartAfter(Duration::from_secs(s)))
        );
        assert_eq!(r.on_exit(None, now), Decision::GiveUp);
    }

    #[test]
    fn old_crashes_are_forgiven() {
        let mut r = Restarts::default();
        let start = Instant::now();
        for _ in 0..4 {
            r.on_exit(Some(1), start);
        }
        let later = start + CRASH_WINDOW + Duration::from_secs(1);
        assert_eq!(
            r.on_exit(Some(1), later),
            Decision::RestartAfter(Duration::from_secs(1))
        );
    }
}
