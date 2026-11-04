//! Motion: springs (ported from `bars/shell/src/style/motion.ts`), values moving along them, and
//! the frame ticker, a high-resolution waitable timer armed only while something moves.
//!
//! Slides and fades change a surface's position and alpha only ([`crate::Surface::move_to`],
//! [`crate::Surface::set_alpha`]); nothing is redrawn for them.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmGetCompositionTimingInfo};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateWaitableTimerExW,
    SetWaitableTimerEx, TIMER_ALL_ACCESS,
};

/// A damped spring (m·x'' + c·x' + k·(x − 1) = 0), released from 0 at rest toward 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    /// motion.ts `spring`.
    pub const DEFAULT: Self = Self::new(1.0, 220.0, 22.0);
    /// motion.ts `spring-soft`.
    pub const SOFT: Self = Self::new(1.0, 120.0, 17.0);
    /// motion.ts `spring-bouncy`.
    pub const BOUNCY: Self = Self::new(1.0, 260.0, 13.0);

    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// The position `t` seconds after release.
    pub fn at(&self, t: f32) -> f32 {
        let (m, k, c, t) = (
            f64::from(self.mass),
            f64::from(self.stiffness),
            f64::from(self.damping),
            f64::from(t),
        );
        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (k * m).sqrt());
        let x = if zeta < 1.0 {
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            1.0 - (-zeta * w0 * t).exp() * ((wd * t).cos() + (zeta * w0 / wd) * (wd * t).sin())
        } else if zeta == 1.0 {
            1.0 - (-w0 * t).exp() * (1.0 + w0 * t)
        } else {
            let root = (zeta * zeta - 1.0).sqrt();
            let r1 = -w0 * (zeta - root);
            let r2 = -w0 * (zeta + root);
            1.0 - (r2 * (r1 * t).exp() - r1 * (r2 * t).exp()) / (r2 - r1)
        };
        x as f32
    }

    /// Seconds until the spring stays within `epsilon` of rest (sampled every millisecond,
    /// at most 3 s).
    pub fn settle(&self, epsilon: f32) -> f32 {
        let last = (0..=3000)
            .rev()
            .map(|ms| ms as f32 / 1000.0)
            .find(|&t| (self.at(t) - 1.0).abs() > epsilon)
            .unwrap_or(0.0);
        (last + 0.001).min(3.0)
    }
}

/// A value moving from `from` to `to` along a spring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    spring: Spring,
    start: Instant,
    duration: Duration,
}

impl Tween {
    /// Starts at `now`. It ends when the spring settles within 1/1000 of the distance.
    pub fn new(from: f32, to: f32, spring: Spring, now: Instant) -> Self {
        Self {
            from,
            to,
            spring,
            start: now,
            duration: Duration::from_secs_f32(spring.settle(0.001)),
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        if self.done(now) {
            return self.to;
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        self.from + (self.to - self.from) * self.spring.at(t)
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration
    }
}

/// Wakes the UI loop once per display frame while something moves, and never otherwise. Wait
/// on [`handle`](Self::handle) with the window messages; call [`schedule`](Self::schedule)
/// after each pass of the loop.
pub struct Ticker {
    timer: HANDLE,
    frame: Duration,
    /// When the armed tick is due; a due time in the past has fired.
    due: Option<Instant>,
}

impl Ticker {
    pub fn new() -> Result<Self> {
        // SAFETY: creates an unnamed timer owned by this struct.
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
        }
        .context("high-resolution waitable timer")?;
        Ok(Self {
            timer,
            frame: display_frame(),
            due: None,
        })
    }

    /// Signalled when a frame is due (an auto-reset timer: a satisfied wait resets it).
    pub fn handle(&self) -> HANDLE {
        self.timer
    }

    /// While `moving`, makes sure a tick is pending (one frame from now, unless one already
    /// is: extra loop passes never push it back); otherwise disarms the timer.
    pub fn schedule(&mut self, moving: bool) {
        let now = Instant::now();
        // SAFETY: our own timer; the due time is a local.
        unsafe {
            if moving {
                if self.due.is_some_and(|due| due > now) {
                    return;
                }
                // Negative: relative, in 100 ns units.
                let relative = -(self.frame.as_nanos() as i64 / 100);
                let armed = SetWaitableTimerEx(self.timer, &relative, 0, None, None, None, 0);
                self.due = armed.is_ok().then_some(now + self.frame);
            } else if self.due.take().is_some() {
                let _ = CancelWaitableTimer(self.timer);
            }
        }
    }

    /// Is a tick pending (or just fired and not yet followed by `schedule`)?
    pub fn armed(&self) -> bool {
        self.due.is_some()
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        // SAFETY: our own handle, closed once.
        let _ = unsafe { CloseHandle(self.timer) };
    }
}

/// One refresh of the display, from DWM (60 Hz when it can't say).
fn display_frame() -> Duration {
    let mut timing = DWM_TIMING_INFO {
        cbSize: size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: the out struct is a valid local with its size set.
    let ok = unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing) }.is_ok();
    let rate = timing.rateRefresh;
    if ok && rate.uiNumerator > 0 && rate.uiDenominator > 0 {
        Duration::from_secs_f64(f64::from(rate.uiDenominator) / f64::from(rate.uiNumerator))
    } else {
        Duration::from_secs_f64(1.0 / 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::WaitForSingleObject;

    #[test]
    fn springs_start_at_rest_and_settle_on_one_in_every_damping_regime() {
        for spring in [
            Spring::DEFAULT,
            Spring::BOUNCY,
            Spring::new(1.0, 100.0, 20.0), // critically damped
            Spring::new(1.0, 100.0, 40.0), // overdamped
        ] {
            assert!(spring.at(0.0).abs() < 1e-6, "{spring:?}");
            let settle = spring.settle(0.001);
            assert!(settle > 0.05 && settle < 3.0, "{spring:?}: {settle}");
            assert!((spring.at(settle) - 1.0).abs() <= 0.001, "{spring:?}");
        }
        // Underdamped springs overshoot; the critically damped one never does.
        assert!((0..1000).any(|ms| Spring::BOUNCY.at(ms as f32 / 1000.0) > 1.05));
        let critical = Spring::new(1.0, 100.0, 20.0);
        assert!((0..1000).all(|ms| critical.at(ms as f32 / 1000.0) <= 1.0));
        // motion.ts: the default spring settles in about two thirds of a second.
        let settle = Spring::DEFAULT.settle(0.001);
        assert!((0.5..0.8).contains(&settle), "{settle}");
    }

    #[test]
    fn a_tween_ends_exactly_on_its_target() {
        let start = Instant::now();
        let tween = Tween::new(-36.0, 0.0, Spring::DEFAULT, start);
        assert_eq!(tween.value(start), -36.0);
        let later = start + Duration::from_secs(1);
        assert!(tween.done(later));
        assert_eq!(tween.value(later), 0.0);
    }

    #[test]
    fn the_ticker_fires_only_while_armed() {
        let mut ticker = Ticker::new().unwrap();
        ticker.schedule(true);
        assert!(ticker.armed());
        // SAFETY: waits on our own timer.
        assert_eq!(
            // SAFETY: the ticker owns this valid timer handle throughout the bounded wait.
            unsafe { WaitForSingleObject(ticker.handle(), 200) },
            WAIT_OBJECT_0
        );
        ticker.schedule(true);
        ticker.schedule(false);
        assert!(!ticker.armed());
        // SAFETY: as above.
        assert_eq!(
            // SAFETY: the disarmed ticker still owns this valid handle throughout the wait.
            unsafe { WaitForSingleObject(ticker.handle(), 100) },
            WAIT_TIMEOUT
        );
    }
}
