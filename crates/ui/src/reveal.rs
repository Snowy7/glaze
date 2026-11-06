//! Opt-in popup presentation. A short-lived bitmap scales together with its hit regions and
//! glass body; settlement restores the original list and releases the bitmap.
use crate::{DisplayList, Hit, Image, Op, Point, Rect, Ui};

/// The same uniform transform for pixels, glass, hits and pointer coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub from: Rect,
    pub to: Rect,
}
impl Transform {
    pub fn scale(self) -> f32 {
        self.to.width() / self.from.width().max(1.0)
    }
    pub fn rect(self, rect: Rect) -> Rect {
        let s = self.scale();
        Rect::new(
            self.to.left + (rect.left - self.from.left) * s,
            self.to.top + (rect.top - self.from.top) * s,
            rect.width() * s,
            rect.height() * s,
        )
    }
    pub fn inverse(self, point: Point) -> Point {
        let s = self.scale().max(0.001);
        Point::new(
            self.from.left + (point.x - self.to.left) / s,
            self.from.top + (point.y - self.to.top) / s,
        )
    }
}
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveal {
    Slide,
    Fade,
    Pop,
    Scale,
    Morph,
    None,
}

impl Reveal {
    pub fn scales(self) -> bool {
        matches!(self, Self::Pop | Self::Scale | Self::Morph)
    }

    /// The body's rectangle at this point of a reveal, in window-local physical pixels.
    pub fn body(self, full: Rect, anchor: Rect, progress: f32) -> Rect {
        let p = progress.clamp(0.0, 1.0);
        let initial = match self {
            Self::Pop => 0.92,
            Self::Scale => 0.75,
            Self::Morph => (anchor.width().min(anchor.height())
                / full.width().max(full.height()).max(1.0))
            .clamp(0.06, 0.4),
            _ => 1.0,
        };
        let scale = initial + (1.0 - initial) * p;
        let center = (
            (full.left + full.right) / 2.0,
            (full.top + full.bottom) / 2.0,
        );
        let (x, y) = if self == Self::Morph {
            let ax = ((anchor.left + anchor.right) / 2.0).clamp(full.left, full.right);
            let ay = ((anchor.top + anchor.bottom) / 2.0).clamp(full.top, full.bottom);
            (ax + (center.0 - ax) * p, ay + (center.1 - ay) * p)
        } else {
            center
        };
        Rect::new(
            x - full.width() * scale / 2.0,
            y - full.height() * scale / 2.0,
            full.width() * scale,
            full.height() * scale,
        )
    }
}

pub struct Snapshot {
    pub original: DisplayList,
    image: Arc<Image>,
    size: (u32, u32),
}

impl Snapshot {
    pub fn new(ui: &mut Ui, list: DisplayList, size: (u32, u32)) -> anyhow::Result<Self> {
        let image = Arc::new(crate::render(ui, &list, size.0, size.1)?);
        Ok(Self {
            original: list,
            image,
            size,
        })
    }

    pub fn frame(&self, full: Rect, body: Rect) -> DisplayList {
        let transform = Transform {
            from: full,
            to: body,
        };
        let rect = |r| transform.rect(r);
        DisplayList {
            ops: vec![Op::Image {
                rect: rect(Rect::new(0.0, 0.0, self.size.0 as f32, self.size.1 as f32)),
                image: self.image.clone(),
                opacity: 1.0,
                source: None,
            }],
            hits: self
                .original
                .hits
                .iter()
                .map(|hit| Hit {
                    rect: rect(hit.rect),
                    ..hit.clone()
                })
                .collect(),
            focus: self.original.focus,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_styles_have_distinct_starts_and_an_exact_final_body() {
        let full = Rect::new(10.0, 10.0, 400.0, 600.0);
        let anchor = Rect::new(20.0, 0.0, 24.0, 24.0);
        for style in [
            Reveal::Slide,
            Reveal::Fade,
            Reveal::Pop,
            Reveal::Scale,
            Reveal::Morph,
            Reveal::None,
        ] {
            assert_eq!(style.body(full, anchor, 1.0), full);
        }
        assert!(
            Reveal::Pop.body(full, anchor, 0.0).width()
                > Reveal::Scale.body(full, anchor, 0.0).width()
        );
        assert!(
            Reveal::Morph.body(full, anchor, 0.0).width()
                < Reveal::Scale.body(full, anchor, 0.0).width()
        );
    }

    #[test]
    fn pointer_coordinates_and_rectangles_follow_the_same_transform() {
        let transform = Transform {
            from: Rect::new(10.0, 10.0, 400.0, 600.0),
            to: Rect::new(40.0, 70.0, 200.0, 300.0),
        };
        let hit = Rect::new(40.0, 50.0, 80.0, 30.0);
        let painted = transform.rect(hit);
        assert_eq!(
            transform.inverse(Point::new(painted.left, painted.top)),
            Point::new(hit.left, hit.top)
        );
        assert_eq!(painted.width(), hit.width() * 0.5);
    }
}
