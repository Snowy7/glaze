//! Opt-in hover/press samples for kit controls and menus. Entries exist only for entered
//! controls; moving back to rest removes them, and a settled hover never requests a frame.
use crate::transition::{Timing, Transition};
use crate::{HitId, Interaction, Rgba};
use std::{collections::HashMap, time::Instant};

#[derive(Default)]
struct Value {
    value: f32,
    target: f32,
    motion: Option<Transition>,
}
impl Value {
    fn step(&mut self, target: f32, timing: Timing, now: Instant) -> bool {
        let before = self.value;
        if target != self.target {
            self.target = target;
            self.motion = timing
                .enabled
                .then(|| timing.start(self.value, target, now));
            if !timing.enabled {
                self.value = target;
            }
        }
        if let Some(motion) = self.motion {
            self.value = motion.value(now);
            if motion.done(now) {
                self.motion = None;
            }
        }
        before != self.value
    }
}

pub struct InteractionMotion {
    pub hover: Timing,
    pub press: Timing,
    values: HashMap<HitId, (Value, Value)>,
    moving: bool,
}

impl InteractionMotion {
    pub fn new(hover: Timing, press: Timing) -> Self {
        Self {
            hover,
            press,
            values: HashMap::new(),
            moving: false,
        }
    }
    pub fn moving(&self) -> bool {
        self.moving
    }
    pub fn step(&mut self, input: &Interaction, now: Instant) -> bool {
        for id in [input.hover, input.pressed].into_iter().flatten() {
            self.values.entry(id).or_default();
        }
        let mut changed = false;
        self.moving = false;
        self.values.retain(|&id, (hover, press)| {
            let hot = input.hover == Some(id);
            changed |= hover.step(f32::from(hot && input.pressed.is_none()), self.hover, now);
            changed |= press.step(f32::from(hot && input.pressed == Some(id)), self.press, now);
            self.moving |= hover.motion.is_some() || press.motion.is_some();
            hot || input.pressed == Some(id) || hover.value != 0.0 || press.value != 0.0
        });
        changed
    }
    pub fn wash(&self, id: HitId, hover: Rgba, press: Rgba, fallback: Rgba) -> Rgba {
        let Some((h, p)) = self.values.get(&id) else {
            return fallback;
        };
        let h = h.value.clamp(0.0, 1.0);
        let p = p.value.clamp(0.0, 1.0);
        let ha = hover.a * h * (1.0 - p);
        let pa = press.a * p;
        let alpha = ha + pa;
        if alpha == 0.0 {
            return Rgba::TRANSPARENT;
        }
        Rgba::new(
            (hover.r * ha + press.r * pa) / alpha,
            (hover.g * ha + press.g * pa) / alpha,
            (hover.b * ha + press.b * pa) / alpha,
            alpha,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transition::Easing;
    use std::time::Duration;
    #[test]
    fn hover_and_press_have_separate_timing_and_rest_stops_frames() {
        let hover = Timing::new(true, 100.0, Easing::Bezier([0.0, 0.0, 1.0, 1.0]));
        let press = Timing::new(false, 50.0, hover.easing);
        let mut motion = InteractionMotion::new(hover, press);
        let now = Instant::now();
        let mut input = Interaction {
            hover: Some(7),
            ..Default::default()
        };
        motion.step(&input, now);
        assert!(motion.moving());
        motion.step(&input, now + Duration::from_millis(50));
        let white = Rgba::new(1.0, 1.0, 1.0, 0.2);
        assert!((motion.wash(7, white, white, Rgba::TRANSPARENT).a - 0.1).abs() < 0.001);
        input.pressed = Some(7);
        motion.step(&input, now + Duration::from_millis(60));
        assert_eq!(motion.wash(7, white, white, Rgba::TRANSPARENT), white);
        input.reset();
        motion.step(&input, now + Duration::from_millis(70));
        motion.step(&input, now + Duration::from_millis(200));
        assert!(!motion.moving());
        assert!(motion.values.is_empty());
    }
}
