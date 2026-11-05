//! What a view returns: a display list of paint operations plus the hit items (for input and
//! accessibility). A surface compares each new list with the one on screen and redraws only the
//! bounds of the operations that changed; an identical list costs nothing.

use std::sync::Arc;

use accesskit::Role;

use crate::geom::{Point, Rect, Rgba};
use crate::text::TextStyle;

/// Identifies a hit item within its surface; the view chooses them.
pub type HitId = u32;

/// One paint operation, in the surface's device pixels, drawn in list order.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// A clipped group, used by windowless scroll views. Bounds never escape the viewport.
    Clipped { rect: Rect, ops: Arc<[Op]> },
    /// Markdown emphasis without changing the shared text style contract.
    Emphasis {
        rect: Rect,
        text: Arc<str>,
        style: TextStyle,
        color: Rgba,
        italic: bool,
        strike: bool,
    },
    /// A clockwise circular arc from twelve o'clock, in radians. Full turns draw an ellipse.
    Arc {
        center: Point,
        radius: f32,
        sweep: f32,
        width: f32,
        color: Rgba,
    },
    /// An immutable premultiplied bitmap (photos, album artwork and app icons).
    Image {
        rect: Rect,
        image: Arc<crate::Image>,
        opacity: f32,
        source: Option<Rect>,
    },
    /// A snapshot of a field's revision, so edits invalidate its bounds without an idle tick.
    TextField {
        rect: Rect,
        field: crate::TextField,
        revision: u64,
    },
    /// A filled rectangle, rounded when `radius > 0`.
    Fill {
        rect: Rect,
        radius: f32,
        color: Rgba,
    },
    /// A rectangle's outline, centred on its edge.
    Stroke {
        rect: Rect,
        radius: f32,
        width: f32,
        color: Rgba,
    },
    Line {
        from: Point,
        to: Point,
        width: f32,
        color: Rgba,
    },
    /// A string (or an icon glyph in an icon font) with its top-left at `rect`'s; `rect` is its
    /// measured size ([`crate::Text::measure`]).
    Text {
        rect: Rect,
        text: Arc<str>,
        style: TextStyle,
        color: Rgba,
    },
    /// A soft shadow under `rect`: `spread` pixels of falloff, moved down by `drop`.
    Shadow {
        rect: Rect,
        radius: f32,
        spread: f32,
        drop: f32,
        color: Rgba,
    },
    /// Clips what follows to `rect` (intersected with any clip already pushed) until the
    /// matching [`Op::Unclip`]. Scroll views wrap their content in a pair.
    Clip { rect: Rect },
    /// Ends the innermost [`Op::Clip`].
    Unclip,
}

impl Op {
    /// Every pixel this operation may touch, antialiasing included.
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Clipped { rect, .. } => *rect,
            Self::Emphasis { rect, style, .. } => rect.inflate(2.0 + style.size * 0.1),
            Self::Arc {
                center,
                radius,
                width,
                ..
            } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            )
            .inflate(width / 2.0 + 1.0),
            Self::Image { rect, .. } | Self::TextField { rect, .. } => rect.inflate(1.0),
            Self::Fill { rect, .. } => rect.inflate(1.0),
            Self::Stroke { rect, width, .. } => rect.inflate(width / 2.0 + 1.0),
            Self::Line {
                from, to, width, ..
            } => Rect {
                left: from.x.min(to.x),
                top: from.y.min(to.y),
                right: from.x.max(to.x),
                bottom: from.y.max(to.y),
            }
            .inflate(width + 1.0),
            // Glyphs may overhang their layout box a little.
            Self::Text { rect, style, .. } => rect.inflate(2.0 + style.size * 0.1),
            Self::Shadow {
                rect, spread, drop, ..
            } => rect.inflate(spread + 1.0).offset(0.0, *drop),
            // A changed clip changes what everything inside it may show.
            Self::Clip { rect } => *rect,
            Self::Unclip => Rect::default(),
        }
    }
}

/// Something the pointer, the keyboard and assistive technology can land on.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: HitId,
    pub rect: Rect,
    pub role: Role,
    pub label: Arc<str>,
    /// Disabled items are still announced, but never selected.
    pub enabled: bool,
    /// For check items: whether the mark is on.
    pub checked: Option<bool>,
}

/// One frame of a surface.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayList {
    pub ops: Vec<Op>,
    /// Hit items, topmost last.
    pub hits: Vec<Hit>,
    /// The hit item with keyboard focus.
    pub focus: Option<HitId>,
}

impl DisplayList {
    /// The topmost hit item under `p`.
    pub fn hit_at(&self, p: Point) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.rect.contains(p))
    }

    /// What has to be redrawn to go from `old` to this list: the bounds of every operation that
    /// differs (position by position), or `None` when they paint the same.
    pub fn damage(&self, old: &DisplayList) -> Option<Rect> {
        damage_ops(&old.ops, &self.ops)
    }

    /// Disjoint damage for dashboards: a CPU and network change need not redraw the labels
    /// or meters between them. Layered-window presentation still happens just once.
    pub fn damage_regions(&self, old: &DisplayList) -> Vec<Rect> {
        let mut regions = Vec::new();
        collect_damage(&old.ops, &self.ops, None, &mut regions);
        let mut merged: Vec<Rect> = Vec::new();
        for mut rect in regions {
            while let Some(index) = merged
                .iter()
                .position(|old| old.inflate(1.0).intersects(&rect))
            {
                rect = rect.union(merged.swap_remove(index));
            }
            merged.push(rect);
            if merged.len() > 64 {
                return self.damage(old).into_iter().collect();
            }
        }
        merged
    }
}

fn collect_damage(old: &[Op], new: &[Op], clip: Option<Rect>, out: &mut Vec<Rect>) {
    for i in 0..old.len().max(new.len()) {
        let changed = match (old.get(i), new.get(i)) {
            (Some(a), Some(b)) if a == b => continue,
            (
                Some(Op::Clipped {
                    rect: a,
                    ops: before,
                }),
                Some(Op::Clipped {
                    rect: b,
                    ops: after,
                }),
            ) if a == b => {
                let clip = clip.map_or(*a, |outer| intersection(outer, *a));
                collect_damage(before, after, Some(clip), out);
                continue;
            }
            (Some(a), Some(b)) => arc_change(a, b).unwrap_or_else(|| a.bounds().union(b.bounds())),
            (Some(one), None) | (None, Some(one)) => one.bounds(),
            (None, None) => continue,
        };
        let changed = clip.map_or(changed, |clip| intersection(changed, clip));
        if !changed.is_empty() {
            out.push(changed);
        }
    }
}

/// The renderer constructs arcs in fixed quarter turns. Preserve complete unchanged quarters;
/// the final partial quarter can change its curve approximation all the way to its start.
fn arc_change(old: &Op, new: &Op) -> Option<Rect> {
    let (
        Op::Arc {
            center: a,
            radius: ar,
            sweep: before,
            width: aw,
            color: ac,
        },
        Op::Arc {
            center: b,
            radius: br,
            sweep: after,
            width: bw,
            color: bc,
        },
    ) = (old, new)
    else {
        return None;
    };
    if a != b
        || ar != br
        || aw != bw
        || ac != bc
        || !before.is_finite()
        || !after.is_finite()
        || *before >= std::f32::consts::TAU - 0.0001
        || *after >= std::f32::consts::TAU - 0.0001
    {
        return None;
    }
    let quarter = std::f32::consts::FRAC_PI_2;
    let start = (before.min(*after).max(0.0) / quarter).floor() * quarter;
    let end = ((before.max(*after).max(0.0) / quarter).ceil() * quarter).min(std::f32::consts::TAU);
    let point = |angle: f32| Point::new(a.x + ar * angle.sin(), a.y - ar * angle.cos());
    let first = point(start);
    let mut rect = Rect::new(first.x, first.y, 0.0, 0.0);
    for angle in [
        end,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::PI,
        3.0 * std::f32::consts::FRAC_PI_2,
    ] {
        if angle >= start && angle <= end {
            let p = point(angle);
            rect.left = rect.left.min(p.x);
            rect.right = rect.right.max(p.x);
            rect.top = rect.top.min(p.y);
            rect.bottom = rect.bottom.max(p.y);
        }
    }
    Some(rect.inflate(aw / 2.0 + 2.0))
}
fn intersection(a: Rect, b: Rect) -> Rect {
    Rect {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    }
}

fn damage_ops(old: &[Op], new: &[Op]) -> Option<Rect> {
    let count = new.len().max(old.len());
    (0..count)
        .filter_map(|i| match (old.get(i), new.get(i)) {
            (Some(a), Some(b)) if a == b => None,
            (
                Some(Op::Clipped {
                    rect: a,
                    ops: before,
                }),
                Some(Op::Clipped {
                    rect: b,
                    ops: after,
                }),
            ) if a == b => {
                let changed = damage_ops(before, after)?;
                let clipped = Rect {
                    left: changed.left.max(a.left),
                    top: changed.top.max(a.top),
                    right: changed.right.min(a.right),
                    bottom: changed.bottom.min(a.bottom),
                };
                (!clipped.is_empty()).then_some(clipped)
            }
            (a, b) => {
                let bounds = |op: Option<&Op>| op.map(Op::bounds).filter(|r| !r.is_empty());
                match (bounds(a), bounds(b)) {
                    (Some(a), Some(b)) => Some(a.union(b)),
                    (one, other) => one.or(other),
                }
            }
        })
        .reduce(Rect::union)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(x: f32) -> Op {
        Op::Fill {
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            radius: 0.0,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        }
    }

    fn list(ops: Vec<Op>) -> DisplayList {
        DisplayList {
            ops,
            ..Default::default()
        }
    }

    #[test]
    fn identical_lists_need_no_redraw_whatever_their_hits_say() {
        let a = list(vec![fill(0.0), fill(20.0)]);
        let mut b = a.clone();
        b.focus = Some(3);
        assert_eq!(b.damage(&a), None);
    }

    #[test]
    fn a_changed_scrolled_meter_damages_its_own_bounds_and_not_the_whole_viewport() {
        let viewport = Rect::new(0.0, 0.0, 400.0, 300.0);
        let before = list(vec![Op::Clipped {
            rect: viewport,
            ops: vec![fill(0.0), fill(100.0), fill(300.0)].into(),
        }]);
        let after = list(vec![Op::Clipped {
            rect: viewport,
            ops: vec![fill(0.0), fill(105.0), fill(300.0)].into(),
        }]);
        assert_eq!(
            after.damage(&before),
            Some(Rect::new(99.0, 0.0, 17.0, 11.0))
        );
        let outside = list(vec![Op::Clipped {
            rect: viewport,
            ops: vec![fill(0.0), fill(100.0), fill(500.0)].into(),
        }]);
        let changed_outside = list(vec![Op::Clipped {
            rect: viewport,
            ops: vec![fill(0.0), fill(100.0), fill(510.0)].into(),
        }]);
        assert_eq!(changed_outside.damage(&outside), None);
    }

    #[test]
    fn meter_sweep_damage_keeps_the_unchanged_ring() {
        let arc = |sweep| Op::Arc {
            center: Point::new(50.0, 50.0),
            radius: 40.0,
            sweep,
            width: 3.0,
            color: Rgba::HIT,
        };
        let before = list(vec![arc(0.20 * std::f32::consts::TAU)]);
        let after = list(vec![arc(0.21 * std::f32::consts::TAU)]);
        let regions = after.damage_regions(&before);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].width() < 50.0 && regions[0].height() < 50.0);
        assert!(regions[0].left > 46.0 && regions[0].bottom < 54.0);
        assert_eq!(before.damage_regions(&after), regions);
    }

    #[test]
    fn sparse_damage_keeps_distant_changes_separate_and_merges_neighbors() {
        let before = list(vec![fill(0.0), fill(15.0), fill(100.0)]);
        let after = list(vec![fill(4.0), fill(17.0), fill(110.0)]);
        let mut regions = after.damage_regions(&before);
        regions.sort_by(|a, b| a.left.total_cmp(&b.left));
        assert_eq!(
            regions,
            vec![
                Rect::new(-1.0, -1.0, 29.0, 12.0),
                Rect::new(99.0, -1.0, 22.0, 12.0),
            ]
        );
        assert!(before.damage_regions(&before).is_empty());
    }

    #[test]
    fn damage_covers_only_the_operations_that_changed() {
        let a = list(vec![fill(0.0), fill(20.0), fill(40.0)]);
        let b = list(vec![fill(0.0), fill(25.0), fill(40.0)]);
        // The old and the new place of the moved fill, nothing else.
        assert_eq!(b.damage(&a), Some(Rect::new(19.0, -1.0, 17.0, 12.0)));
        // A removed operation is damage where it was.
        let shorter = list(vec![fill(0.0)]);
        let gone = shorter.damage(&a).unwrap();
        assert!(gone.left <= 19.0 && gone.right >= 51.0);
        assert!(gone.left > 10.0, "the kept fill is not redrawn");
    }
}
