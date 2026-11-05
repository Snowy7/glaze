//! Opt-in timed transitions. Resolution (including a spring's settling time) belongs to config
//! changes; sampling does only curve math and returns the exact destination when finished.
use std::time::{Duration, Instant};

use crate::Spring;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Bezier([f32; 4]),
    Spring(Spring),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub enabled: bool,
    pub duration: Duration,
    pub easing: Easing,
}

impl Timing {
    pub fn new(enabled: bool, milliseconds: f32, easing: Easing) -> Self {
        let duration = match easing {
            Easing::Spring(spring) => Duration::from_secs_f32(spring.settle(0.001)),
            _ => Duration::from_secs_f32(milliseconds.max(0.0) / 1000.0),
        };
        Self {
            enabled: enabled && milliseconds > 0.0,
            duration,
            easing,
        }
    }

    pub fn start(self, from: f32, to: f32, now: Instant) -> Transition {
        Transition {
            from,
            to,
            timing: self,
            start: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    from: f32,
    to: f32,
    pub timing: Timing,
    start: Instant,
}

impl Transition {
    pub fn done(self, now: Instant) -> bool {
        !self.timing.enabled || now.saturating_duration_since(self.start) >= self.timing.duration
    }

    pub fn value(self, now: Instant) -> f32 {
        if self.done(now) {
            return self.to;
        }
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let p = match self.timing.easing {
            Easing::Spring(spring) => spring.at(elapsed),
            Easing::Bezier(points) => {
                bezier_at(points, elapsed / self.timing.duration.as_secs_f32())
            }
        };
        self.from + (self.to - self.from) * p
    }
}

pub fn bezier_at([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cubic =
        |a, b, t: f32| 3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..16 {
        let t = (low + high) / 2.0;
        if cubic(x1, x2, t) < x {
            low = t;
        } else {
            high = t;
        }
    }
    cubic(y1, y2, (low + high) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timed_transition_lands_exactly_and_disabled_never_moves() {
        let now = Instant::now();
        let timing = Timing::new(true, 80.0, Easing::Bezier([0.0, 0.0, 1.0, 1.0]));
        let motion = timing.start(2.0, 4.0, now);
        assert!((motion.value(now + Duration::from_millis(40)) - 3.0).abs() < 0.001);
        assert_eq!(motion.value(now + Duration::from_millis(80)), 4.0);
        assert!(motion.done(now + Duration::from_millis(80)));
        assert_eq!(
            Timing {
                enabled: false,
                ..timing
            }
            .start(2.0, 4.0, now)
            .value(now),
            4.0
        );
    }
}
