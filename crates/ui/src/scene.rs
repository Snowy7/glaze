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
    /// Moves a list into a cropped surface without changing its hit identities.
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let mut list = self.clone();
        for op in &mut list.ops {
            match op {
                Op::Arc { center, .. } => {
                    center.x += dx;
                    center.y += dy;
                }
                Op::Line { from, to, .. } => {
                    from.x += dx;
                    from.y += dy;
                    to.x += dx;
                    to.y += dy;
                }
                Op::Image { rect, .. }
                | Op::TextField { rect, .. }
                | Op::Fill { rect, .. }
                | Op::Stroke { rect, .. }
                | Op::Text { rect, .. }
                | Op::Clip { rect }
                | Op::Shadow { rect, .. } => *rect = rect.offset(dx, dy),
                Op::Unclip => {}
            }
        }
        for hit in &mut list.hits {
            hit.rect = hit.rect.offset(dx, dy);
        }
        list
    }

    /// The topmost hit item under `p`.
    pub fn hit_at(&self, p: Point) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.rect.contains(p))
    }

    /// What has to be redrawn to go from `old` to this list: the bounds of every operation that
    /// differs (position by position), or `None` when they paint the same.
    pub fn damage(&self, old: &DisplayList) -> Option<Rect> {
        let count = self.ops.len().max(old.ops.len());
        (0..count)
            .filter_map(|i| match (old.ops.get(i), self.ops.get(i)) {
                (Some(a), Some(b)) if a == b => None,
                (a, b) => {
                    // `Unclip` paints nothing and has no area.
                    let bounds = |op: Option<&Op>| op.map(Op::bounds).filter(|r| !r.is_empty());
                    match (bounds(a), bounds(b)) {
                        (Some(a), Some(b)) => Some(a.union(b)),
                        (one, other) => one.or(other),
                    }
                }
            })
            .reduce(Rect::union)
    }
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
