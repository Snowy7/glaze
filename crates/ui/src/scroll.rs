//! A scroll view's state: a pixel offset into content taller than its viewport.
//!
//! - **Fixed viewport.** The view gives the viewport (`Frame::scroll`); content outside it is
//!   clipped and its hits dropped. Headers and footers live outside the viewport and never
//!   move.
//! - **Precision touchpads** send small wheel deltas: they move the content by exactly that
//!   much, at once. A **wheel notch** (a multiple of 120) glides [`NOTCH`] design px along the
//!   theme's scroll spring.
//! - **The overlay scrollbar** sits inside the viewport's right edge, shows while scrolling and
//!   fades after the theme's idle time. While nothing moves, [`Scroll::tick`] asks for nothing
//!   (Rule 2).
//!
//! The owner keeps a `Scroll`, feeds it [`Scroll::wheel`], and redraws when that or
//! [`Scroll::tick`] says so. `view(&self)` may draw with it: the content's extent is recorded
//! through a `Cell` while drawing, so the next wheel clamps against what is on screen.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::motion::Spring;
use crate::theme::MotionTokens;

/// Design px one wheel notch scrolls (three lines of 16 px, Windows' default).
pub const NOTCH: f32 = 48.0;

/// The frame interval while gliding or fading.
const FRAME: Duration = Duration::from_millis(8);

#[derive(Debug, Clone)]
pub struct Scroll {
    /// Where the content rests (device px from its top); the glide heads here.
    target: f32,
    /// A glide in progress: where it started, and when.
    glide: Option<(f32, Instant)>,
    /// Content and viewport heights as last drawn, device px.
    extent: Cell<(f32, f32)>,
    /// The last scroll, for the scrollbar's fade.
    active: Option<Instant>,
    spring: Spring,
    idle: Duration,
    fade: Duration,
}

impl Default for Scroll {
    fn default() -> Self {
        Self::new(&crate::theme::Theme::new(
            crate::theme::Style::Liquid,
            crate::theme::Scheme::Dark,
            crate::theme::SYSTEM_BLUE,
        )
        .motion)
    }
}

impl Scroll {
    pub fn new(motion: &MotionTokens) -> Self {
        Self {
            target: 0.0,
            glide: None,
            extent: Cell::new((0.0, 0.0)),
            active: None,
            spring: motion.scroll,
            idle: Duration::from_secs_f32(motion.scrollbar_idle),
            fade: Duration::from_secs_f32(motion.scrollbar_fade),
        }
    }

    /// How far the content can move: content less viewport.
    pub fn max(&self) -> f32 {
        let (content, viewport) = self.extent.get();
        (content - viewport).max(0.0)
    }

    /// Records the content's and the viewport's heights (device px) as drawn.
    pub fn set_extent(&self, content: f32, viewport: f32) {
        self.extent.set((content, viewport));
    }

    /// The offset to draw at `now` (whole pixels, so text stays crisp).
    pub fn offset(&self, now: Instant) -> f32 {
        let value = match self.glide {
            Some((from, start)) => {
                let t = now.saturating_duration_since(start).as_secs_f32();
                from + (self.target - from) * self.spring.at(t)
            }
            None => self.target,
        };
        value.clamp(0.0, self.max()).round()
    }

    /// The wheel moved by `delta` (positive is up, 120 a notch) at `s` device px per design
    /// px. Whether the offset changed.
    pub fn wheel(&mut self, delta: i16, s: f32, now: Instant) -> bool {
        let notch = delta % 120 == 0;
        let by = -f32::from(delta) / 120.0 * NOTCH * s;
        self.scroll_by(by, notch, now)
    }

    /// Moves by `by` device px, gliding or at once.
    pub fn scroll_by(&mut self, by: f32, glide: bool, now: Instant) -> bool {
        let from = self.offset(now);
        let target = (self.target + by).clamp(0.0, self.max());
        if target == self.target && (self.glide.is_none() || from == target) {
            self.glide = None;
            return false;
        }
        self.target = target;
        self.glide = glide.then_some((from, now));
        self.active = Some(now);
        true
    }

    /// Scrolls by a viewport less a little overlap (Page Up / Page Down).
    pub fn page(&mut self, down: bool, now: Instant) -> bool {
        let (_, viewport) = self.extent.get();
        let by = (viewport * 0.85).max(1.0);
        self.scroll_by(if down { by } else { -by }, true, now)
    }

    /// Scrolls just enough to show `top..bottom` (content px from its top), for keyboard
    /// focus.
    pub fn reveal(&mut self, top: f32, bottom: f32, now: Instant) -> bool {
        let (_, viewport) = self.extent.get();
        let at = self.target;
        if top < at {
            self.scroll_by(top - at, true, now)
        } else if bottom > at + viewport {
            self.scroll_by(bottom - at - viewport, true, now)
        } else {
            false
        }
    }

    /// Back to the top, at once (a reopened list).
    pub fn reset(&mut self) {
        self.target = 0.0;
        self.glide = None;
        self.active = None;
    }

    /// The scrollbar's opacity at `now`, 0..=1: shown while scrolling, then fading.
    pub fn bar(&self, now: Instant) -> f32 {
        if self.max() <= 0.0 {
            return 0.0;
        }
        let Some(active) = self.active else {
            return 0.0;
        };
        let since = now.saturating_duration_since(active);
        if since <= self.idle {
            1.0
        } else {
            let t = (since - self.idle).as_secs_f32() / self.fade.as_secs_f32().max(0.001);
            (1.0 - t).clamp(0.0, 1.0)
        }
    }

    /// Advances the glide and the fade: whether the view changed since `last` was drawn, and
    /// when to tick again (`None` once everything rests).
    pub fn tick(&mut self, now: Instant) -> (bool, Option<Instant>) {
        let mut moving = false;
        if let Some((_, start)) = self.glide {
            let t = now.saturating_duration_since(start).as_secs_f32();
            if t >= self.spring.settle(0.002) {
                self.glide = None;
            }
            moving = true;
        }
        let next = match self.active {
            _ if moving => Some(now + FRAME),
            Some(active) => {
                let since = now.saturating_duration_since(active);
                if since < self.idle {
                    // Wake when the fade starts.
                    Some(active + self.idle)
                } else if since < self.idle + self.fade {
                    moving = true;
                    Some(now + FRAME)
                } else {
                    // The last frame of the fade (alpha 0) is drawn by this tick.
                    self.active = None;
                    moving = true;
                    None
                }
            }
            None => None,
        };
        (moving, next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touchpad_deltas_move_at_once_and_notches_glide_within_the_content() {
        let now = Instant::now();
        let mut scroll = Scroll::default();
        scroll.set_extent(1000.0, 300.0);
        assert!(scroll.wheel(-30, 1.0, now), "a touchpad delta");
        assert_eq!(scroll.offset(now), 12.0, "30/120 of a notch, at once");
        assert!(scroll.wheel(-120, 1.0, now));
        assert_eq!(scroll.offset(now), 12.0, "a notch starts where it was");
        let later = now + Duration::from_millis(600);
        assert_eq!(scroll.offset(later), 60.0, "and glides a notch down");
        for _ in 0..40 {
            scroll.wheel(-120, 1.0, later);
        }
        assert_eq!(scroll.offset(later + Duration::from_secs(2)), 700.0, "clamped");
        assert!(!scroll.wheel(-120, 1.0, later + Duration::from_secs(2)));
    }

    #[test]
    fn the_scrollbar_shows_while_scrolling_then_fades_and_ticks_stop() {
        let now = Instant::now();
        let mut scroll = Scroll::default();
        scroll.set_extent(1000.0, 300.0);
        assert_eq!(scroll.bar(now), 0.0, "hidden until scrolled");
        assert_eq!(scroll.tick(now), (false, None), "nothing to do at rest");
        scroll.wheel(-40, 1.0, now);
        assert_eq!(scroll.bar(now), 1.0);
        let (_, next) = scroll.tick(now);
        assert_eq!(next, Some(now + scroll.idle), "sleeps until the fade");
        let fading = now + scroll.idle + scroll.fade / 2;
        assert!(scroll.bar(fading) > 0.0 && scroll.bar(fading) < 1.0);
        assert!(scroll.tick(fading).0);
        let done = now + scroll.idle + scroll.fade + Duration::from_millis(1);
        assert_eq!(scroll.tick(done), (true, None), "draws the faded frame, then rests");
        assert_eq!(scroll.tick(done), (false, None));
        let mut short = Scroll::default();
        short.set_extent(200.0, 300.0);
        assert!(!short.wheel(-120, 1.0, now), "content that fits never scrolls");
        assert_eq!(short.bar(now), 0.0);
    }
}
