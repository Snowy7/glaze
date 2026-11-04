//! Keyboard focus over a surface's hit items.

use crate::scene::{Hit, HitId};

/// The next enabled hit item after `current` going `delta` (+1 next, -1 previous), wrapping
/// around; with no `current`, the first (or last). `None` when nothing is enabled.
pub fn step(hits: &[Hit], current: Option<HitId>, delta: i32) -> Option<HitId> {
    let count = hits.len() as i32;
    if count == 0 {
        return None;
    }
    let position = current.and_then(|id| hits.iter().position(|h| h.id == id));
    let start = position.map_or(if delta > 0 { -1 } else { count }, |i| i as i32);
    (1..=count)
        .map(|n| &hits[(start + delta * n).rem_euclid(count) as usize])
        .find(|h| h.enabled)
        .map(|h| h.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    fn hits(enabled: &[bool]) -> Vec<Hit> {
        enabled
            .iter()
            .enumerate()
            .map(|(i, &enabled)| Hit {
                id: i as HitId * 10,
                rect: Rect::default(),
                role: accesskit::Role::MenuItem,
                label: "".into(),
                enabled,
                checked: None,
            })
            .collect()
    }

    #[test]
    fn steps_skip_disabled_items_and_wrap() {
        let hits = hits(&[true, false, true, true]);
        assert_eq!(step(&hits, None, 1), Some(0), "first");
        assert_eq!(step(&hits, None, -1), Some(30), "last");
        assert_eq!(step(&hits, Some(0), 1), Some(20), "over the disabled one");
        assert_eq!(step(&hits, Some(20), -1), Some(0));
        assert_eq!(step(&hits, Some(30), 1), Some(0), "wraps down");
        assert_eq!(step(&hits, Some(0), -1), Some(30), "wraps up");
        assert_eq!(step(&[], None, 1), None);
        assert_eq!(step(&self::hits(&[false]), None, 1), None);
    }
}
