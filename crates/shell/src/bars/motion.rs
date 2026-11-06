//! Bar motion resolved on configuration changes, sampled only while a value moves.
use mypaper_bars::style::{Curve, Motion, MotionStyle, ReduceMotion};
use mypaper_ui::Spring;
use mypaper_ui::transition::Easing;
pub use mypaper_ui::transition::{Timing as Settings, Transition};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaf {
    Show,
    Hover,
    Press,
    Open,
    Magnify,
    Bounce,
    Attention,
}

fn child(m: &Motion, leaf: Leaf) -> Option<&Motion> {
    match leaf {
        Leaf::Show => None,
        Leaf::Hover => m.hover.as_deref(),
        Leaf::Press => m.press.as_deref(),
        Leaf::Open => m.open.as_deref(),
        Leaf::Magnify => m.magnify.as_deref(),
        Leaf::Bounce => m.bounce.as_deref(),
        Leaf::Attention => m.attention.as_deref(),
    }
}

fn path(root: &Motion, dock: bool, leaf: Leaf) -> [Option<&Motion>; 6] {
    let bar = root.bar.as_deref();
    let kind = bar.and_then(|b| {
        if dock {
            b.dock.as_deref()
        } else {
            b.top.as_deref()
        }
    });
    [
        Some(root),
        bar,
        kind,
        child(root, leaf),
        bar.and_then(|m| child(m, leaf)),
        kind.and_then(|m| child(m, leaf)),
    ]
}

pub fn presentation(root: &Motion, dock: bool, leaf: Leaf) -> MotionStyle {
    path(root, dock, leaf)
        .into_iter()
        .flatten()
        .filter_map(|m| m.style)
        .next_back()
        .unwrap_or(MotionStyle::Slide)
}

/// Structural defaults first, then the same leaf from root to bar to top/dock. A disabled
/// ancestor always wins. `never` bypasses Windows' motion preference, never performance mode.
pub fn resolve(root: &Motion, dock: bool, leaf: Leaf, system: bool, performance: bool) -> Settings {
    let mut enabled = !performance
        && match root.reduce_motion {
            Some(ReduceMotion::Always) => false,
            Some(ReduceMotion::Never) => true,
            _ => system,
        };
    let mut curve = Curve::Named(
        if matches!(leaf, Leaf::Bounce | Leaf::Attention) {
            "linear"
        } else if matches!(leaf, Leaf::Hover | Leaf::Press) {
            "ease-out"
        } else {
            "spring"
        }
        .into(),
    );
    let mut duration = match leaf {
        Leaf::Magnify => 70.0,
        Leaf::Hover => 120.0,
        Leaf::Press => 90.0,
        Leaf::Bounce => 4000.0,
        Leaf::Attention => 1800.0,
        _ => 180.0,
    };
    let mut configured_duration = false;
    for node in path(root, dock, leaf).into_iter().flatten() {
        enabled &= node.enabled != Some(false);
        if let Some(d) = node.duration {
            duration = d;
            configured_duration = true;
        }
        if let Some(c) = &node.curve {
            curve = c.clone();
        }
    }
    let named_spring = |spring: Spring| {
        if configured_duration {
            Easing::TimedSpring {
                spring,
                settle: spring.settle(0.001),
            }
        } else {
            Easing::Spring(spring)
        }
    };
    let easing = match curve {
        Curve::Spring(s) => Easing::Spring(Spring::new(s.mass, s.stiffness, s.damping)),
        Curve::Named(n) if n == "spring" && leaf == Leaf::Magnify => {
            named_spring(Spring::apple(0.22, 0.0))
        }
        Curve::Named(n) if n == "spring" => named_spring(Spring::DEFAULT),
        Curve::Named(n) if n == "spring-soft" => named_spring(Spring::SOFT),
        Curve::Named(n) if n == "spring-bouncy" => named_spring(Spring::BOUNCY),
        Curve::Bezier(b) => Easing::Bezier(b),
        Curve::Named(n) => Easing::Bezier(match n.as_str() {
            "linear" => [0.0, 0.0, 1.0, 1.0],
            "ease-in" => [0.42, 0.0, 1.0, 1.0],
            "ease-out" | "smooth" => [0.0, 0.0, 0.2, 1.0],
            "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
            "snappy" => [0.2, 0.8, 0.2, 1.0],
            "emphasized" => [0.2, 0.0, 0.0, 1.0],
            "overshoot" => [0.2, 1.4, 0.4, 1.0],
            _ => [0.25, 0.1, 0.25, 1.0],
        }),
    };
    if matches!(leaf, Leaf::Show | Leaf::Open)
        && presentation(root, dock, leaf) == MotionStyle::None
    {
        enabled = false;
    }
    Settings::new(enabled, duration, easing)
}

pub fn reveal(style: MotionStyle) -> mypaper_ui::reveal::Reveal {
    use mypaper_ui::reveal::Reveal;
    match style {
        MotionStyle::Slide => Reveal::Slide,
        MotionStyle::Fade => Reveal::Fade,
        MotionStyle::Pop => Reveal::Pop,
        MotionStyle::Scale => Reveal::Scale,
        MotionStyle::Morph => Reveal::Morph,
        MotionStyle::None => Reveal::None,
    }
}

#[cfg(test)]
pub fn settings(root: &Motion, dock: bool, magnify: bool, system: bool) -> Settings {
    resolve(
        root,
        dock,
        if magnify { Leaf::Magnify } else { Leaf::Show },
        system,
        false,
    )
}

pub struct Value {
    pub value: f32,
    target: f32,
    animation: Option<Transition>,
    pub settings: Settings,
}
impl Value {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            animation: None,
            settings: Settings {
                enabled: false,
                duration: std::time::Duration::ZERO,
                easing: Easing::Bezier([0.0, 0.0, 1.0, 1.0]),
            },
        }
    }
    pub fn configure(&mut self, settings: Settings) {
        self.settings = settings;
        if !settings.enabled {
            self.value = self.target;
            self.animation = None;
        }
    }
    pub fn target(&mut self, to: f32, animate: bool, now: Instant) {
        if self.target == to {
            return;
        }
        self.target = to;
        if animate && self.settings.enabled {
            self.animation = Some(self.settings.start(self.value, to, now));
        } else {
            self.animation = None;
            self.value = to;
        }
    }
    pub fn step(&mut self, now: Instant) -> bool {
        if let Some(animation) = self.animation {
            self.value = animation.value(now);
            if animation.done(now) {
                self.animation = None;
            }
        }
        self.animation.is_some()
    }
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
    fn leaf_values_override_structural_defaults_and_disabled_ancestors_win() {
        let config: Motion = serde_json::from_str(r#"{"duration":400,"hover":{"duration":90,"curve":"linear"},"bar":{"duration":300,"dock":{"duration":200,"hover":{"duration":50}}}}"#).unwrap();
        let hover = resolve(&config, true, Leaf::Hover, true, false);
        assert_eq!(hover.duration, std::time::Duration::from_millis(50));
        assert_eq!(hover.easing, Easing::Bezier([0.0, 0.0, 1.0, 1.0]));
        let mut disabled = config.clone();
        disabled.enabled = Some(false);
        assert!(!resolve(&disabled, true, Leaf::Hover, true, false).enabled);
    }

    #[test]
    fn never_can_override_the_os_but_cannot_override_performance_or_always() {
        let mut config = Motion {
            reduce_motion: Some(ReduceMotion::Never),
            ..Default::default()
        };
        assert!(resolve(&config, true, Leaf::Magnify, false, false).enabled);
        assert!(!resolve(&config, true, Leaf::Magnify, false, true).enabled);
        config.reduce_motion = Some(ReduceMotion::Always);
        assert!(!resolve(&config, true, Leaf::Magnify, true, false).enabled);
    }

    #[test]
    fn show_and_open_have_separate_leaves() {
        let config: Motion = serde_json::from_str(
            r#"{"curve":"linear","duration":100,"open":{"duration":700,"style":"none"}}"#,
        )
        .unwrap();
        assert_eq!(
            resolve(&config, false, Leaf::Show, true, false).duration,
            std::time::Duration::from_millis(100)
        );
        assert!(!resolve(&config, false, Leaf::Open, true, false).enabled);
        assert!(resolve(&config, false, Leaf::Hover, true, false).enabled);
    }
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
