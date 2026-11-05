//! The dock's cosine magnification and spring values; no work after settlement.
use mypaper_bars::style::{Curve, Motion, ReduceMotion};
use mypaper_ui::{Spring, Tween};
use std::time::Instant;

#[derive(Clone)]
pub struct Settings {
    pub enabled: bool,
    spring: Option<Spring>,
    bezier: [f32; 4],
    duration: std::time::Duration,
}
/// Resolve the existing tree and each leaf's duration/curve from least to most specific.
pub fn settings(root: &Motion, dock: bool, magnify: bool, system_animations: bool) -> Settings {
    let mut enabled = match root.reduce_motion {
        Some(ReduceMotion::Always) => false,
        Some(ReduceMotion::Never) => true,
        _ => system_animations,
    };
    let leaf = |m: &Motion| {
        if magnify {
            m.magnify.as_deref().cloned()
        } else {
            m.open.as_deref().cloned()
        }
    };
    let bar = root.bar.as_deref();
    let kind = bar.and_then(|b| {
        if dock {
            b.dock.as_deref()
        } else {
            b.top.as_deref()
        }
    });
    let mut curve = Curve::Named("spring".into());
    let mut duration = if magnify { 70.0 } else { 180.0 };
    for node in [
        Some(root.clone()),
        leaf(root),
        bar.cloned(),
        bar.and_then(leaf),
        kind.cloned(),
        kind.and_then(leaf),
    ]
    .into_iter()
    .flatten()
    {
        enabled &= node.enabled != Some(false);
        if let Some(d) = node.duration {
            duration = d;
        }
        if let Some(c) = node.curve {
            curve = c;
        }
    }
    let spring = match &curve {
        Curve::Spring(s) => Some(Spring::new(s.mass, s.stiffness, s.damping)),
        // The magnification springs only on enter and leave (it follows the pointer between):
        // quick and without bounce, as the Dock's.
        Curve::Named(n) if n == "spring" && magnify => Some(Spring::apple(0.22, 0.0)),
        Curve::Named(n) if n == "spring" => Some(Spring::DEFAULT),
        Curve::Named(n) if n == "spring-soft" => Some(Spring::SOFT),
        Curve::Named(n) if n == "spring-bouncy" => Some(Spring::BOUNCY),
        _ => None,
    };
    let bezier = match curve {
        Curve::Bezier(b) => b,
        Curve::Named(n) => match n.as_str() {
            "linear" => [0.0, 0.0, 1.0, 1.0],
            "ease-in" => [0.42, 0.0, 1.0, 1.0],
            "ease-out" | "smooth" => [0.0, 0.0, 0.2, 1.0],
            "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
            "snappy" => [0.2, 0.8, 0.2, 1.0],
            "emphasized" => [0.2, 0.0, 0.0, 1.0],
            "overshoot" => [0.2, 1.4, 0.4, 1.0],
            _ => [0.25, 0.1, 0.25, 1.0],
        },
        _ => [0.0, 0.0, 1.0, 1.0],
    };
    Settings {
        enabled: enabled && duration > 0.0,
        spring,
        bezier,
        duration: std::time::Duration::from_secs_f32(duration / 1000.0),
    }
}
enum Animation {
    Spring(Tween),
    Timed {
        from: f32,
        to: f32,
        start: Instant,
        settings: Settings,
    },
}
pub struct Value {
    pub value: f32,
    target: f32,
    animation: Option<Animation>,
    pub settings: Settings,
}
impl Value {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            animation: None,
            settings: settings(&Motion::default(), true, false, true),
        }
    }
    pub fn target(&mut self, to: f32, animate: bool, now: Instant) {
        if self.target == to {
            return;
        }
        self.target = to;
        let animate = animate && self.settings.enabled;
        self.animation = animate.then(|| match self.settings.spring {
            Some(spring) => Animation::Spring(Tween::new(self.value, to, spring, now)),
            None => Animation::Timed {
                from: self.value,
                to,
                start: now,
                settings: self.settings.clone(),
            },
        });
        if !animate {
            self.value = to;
        }
    }
    pub fn step(&mut self, now: Instant) -> bool {
        let mut done = false;
        match &self.animation {
            Some(Animation::Spring(tween)) => {
                self.value = tween.value(now);
                done = tween.done(now);
            }
            Some(Animation::Timed {
                from,
                to,
                start,
                settings,
            }) => {
                let t = now.saturating_duration_since(*start).as_secs_f32()
                    / settings.duration.as_secs_f32();
                done = t >= 1.0;
                self.value = if done {
                    *to
                } else {
                    from + (to - from) * bezier_at(settings.bezier, t)
                };
            }
            None => {}
        }
        if done {
            self.animation = None;
        }
        self.animation.is_some()
    }
}
fn bezier_at([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
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

/// Extra icon width pushes neighbors apart, anchored continuously under the pointer.
pub fn magnify(centers: &[f32], pointer: f32, size: f32, max: f32, radius: f32) -> Vec<(f32, f32)> {
    let mut before = 0.0;
    let mut values: Vec<_> = centers
        .iter()
        .map(|&center| {
            let d = ((center - pointer).abs() / radius.max(1.0)).min(1.0);
            let scale = 1.0 + (max - 1.0) * (1.0 + (std::f32::consts::PI * d).cos()) / 2.0;
            let extra = (scale - 1.0) * size;
            let shift = before + extra / 2.0;
            before += extra;
            (scale, shift)
        })
        .collect();
    let anchor = centers
        .windows(2)
        .position(|c| pointer >= c[0] && pointer < c[1])
        .map(|i| {
            let t = (pointer - centers[i]) / (centers[i + 1] - centers[i]);
            values[i].1 + (values[i + 1].1 - values[i].1) * t
        })
        .unwrap_or_else(|| {
            if pointer < centers.first().copied().unwrap_or(0.0) {
                values.first()
            } else {
                values.last()
            }
            .map_or(0.0, |v| v.1)
        });
    for (_, shift) in &mut values {
        *shift -= anchor;
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_configured_linear_motion_lands_exactly_and_stops_ticking() {
        let config: Motion = serde_json::from_str(
            r#"{"bar":{"dock":{"magnify":{"curve":"linear","duration":70}}}}"#,
        )
        .unwrap();
        let mut value = Value::new(1.0);
        value.settings = settings(&config, true, true, true);
        let now = Instant::now();
        value.target(2.0, true, now);
        assert!(value.step(now + std::time::Duration::from_millis(35)));
        assert!((value.value - 1.5).abs() < 0.001);
        assert!(!value.step(now + std::time::Duration::from_millis(70)));
        assert_eq!(value.value, 2.0);
        assert!(!value.step(now + std::time::Duration::from_secs(60)));
    }
    #[test]
    fn zoom_pushes_neighbors_and_keeps_pointer_anchor() {
        let values = magnify(&[24.0, 78.0, 132.0], 78.0, 48.0, 1.5, 144.0);
        assert!((values[1].0 - 1.5).abs() < 0.001);
        assert!(values[0].1 < 0.0 && values[2].1 > 0.0);
        assert!(values[1].1.abs() < 0.001);
        assert_eq!(magnify(&[], 0.0, 48.0, 1.5, 144.0), []);
    }
}
