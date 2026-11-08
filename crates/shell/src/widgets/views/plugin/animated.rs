//! The `animated` view kind (docs/plugins.md, "Views"): when the child's values change (a ring's
//! fraction, a limit bar's level, a segmented bar's segments, a sparkline's points), the shell
//! moves them to the new values with the kit's spring instead of jumping. A spring in flight asks
//! its widget for another frame ([`moving`]); at rest nothing ticks.
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use glaze_plugin_sdk::View;
use glaze_ui::kit::Frame;
use glaze_ui::{HitId, Rect, Spring, Tween};

type NodeSprings = HashMap<(i32, i32, i32), Vec<(f32, Tween)>>;

thread_local! {
    /// Springs by where their node sits (a face's layout keeps a node in place between views).
    static SPRINGS: RefCell<NodeSprings> = RefCell::new(HashMap::new());
    static MOVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a spring was still moving at the last draw; read once per widget schedule.
pub fn moving() -> bool {
    MOVING.with(|m| m.replace(false))
}

/// The child's animatable values, in order.
fn values(view: &View) -> Vec<f32> {
    match view {
        View::Ring { fraction, .. } => vec![*fraction],
        View::LimitBar { left, .. } => vec![*left],
        View::SegmentedBar { segments } => segments.iter().map(|s| s.fraction).collect(),
        View::Sparkline { values, .. } => values.clone(),
        _ => Vec::new(),
    }
}

/// `view` showing `shown` in place of its own values (same order as [`values`]).
fn with_values(view: &View, shown: &[f32]) -> View {
    let mut view = view.clone();
    match &mut view {
        View::Ring { fraction, .. } => *fraction = shown[0],
        View::LimitBar { left, .. } => *left = shown[0],
        View::SegmentedBar { segments } => segments.iter_mut().zip(shown).for_each(|(s, v)| s.fraction = *v),
        View::Sparkline { values, .. } => values.copy_from_slice(shown),
        _ => {}
    }
    view
}

/// Draws `child` in `rect` with its values on springs of `(stiffness, damping)`.
pub fn draw(f: &mut Frame<'_>, rect: Rect, child: &View, (stiffness, damping): (f32, f32), next: &mut HitId) {
    let targets = values(child);
    if targets.is_empty() {
        return super::node(f, rect, child, next);
    }
    let spring = Spring::new(1.0, stiffness.clamp(10.0, 1000.0), damping.clamp(1.0, 100.0));
    let key = (rect.left as i32, rect.top as i32, rect.width() as i32);
    let now = Instant::now();
    let (shown, busy) = SPRINGS.with(|springs| {
        let mut springs = springs.borrow_mut();
        let slots = springs.entry(key).or_default();
        if slots.len() != targets.len() {
            // A new node, or its shape changed: start at rest on the new values.
            *slots = targets.iter().map(|t| (*t, Tween::new(*t, *t, spring, now))).collect();
        }
        for ((target, tween), new) in slots.iter_mut().zip(&targets) {
            if target != new {
                *tween = Tween::new(tween.value(now), *new, spring, now);
                *target = *new;
            }
        }
        let shown: Vec<f32> = slots.iter().map(|(_, t)| t.value(now)).collect();
        let busy = slots.iter().any(|(_, t)| !t.done(now));
        (shown, busy)
    });
    if busy {
        MOVING.with(|m| m.set(true));
    }
    super::node(f, rect, &with_values(child, &shown), next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use glaze_plugin_sdk::Tone;

    #[test]
    fn values_round_trip_through_the_child() {
        let ring = View::Ring { fraction: 0.25, label: None, color: Some(Tone::Accent) };
        assert_eq!(values(&ring), [0.25]);
        assert_eq!(with_values(&ring, &[0.75]), View::Ring { fraction: 0.75, label: None, color: Some(Tone::Accent) });
        assert!(values(&View::Spacer).is_empty());
    }
}
