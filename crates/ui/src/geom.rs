//! Points, rectangles and colours, in device pixels of a surface's bitmap.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    /// A rectangle from its top-left corner and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }

    /// Grown by `by` on every side (shrunk when negative).
    pub fn inflate(self, by: f32) -> Self {
        Self {
            left: self.left - by,
            top: self.top - by,
            right: self.right + by,
            bottom: self.bottom + by,
        }
    }

    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }

    /// The smallest rectangle holding both.
    pub fn union(self, other: Rect) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    /// The overlap of both (empty when they don't meet).
    pub fn intersect(self, other: Rect) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    pub fn center(&self) -> Point {
        Point::new(
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }

    /// Shrunk by `insets` (layout's padding).
    pub fn inset(self, insets: crate::layout::Insets) -> Self {
        Self {
            left: self.left + insets.left,
            top: self.top + insets.top,
            right: self.right - insets.right,
            bottom: self.bottom - insets.bottom,
        }
    }

    /// The top `height` of this rectangle, and what is left under it after `gap`.
    pub fn take_top(self, height: f32, gap: f32) -> (Self, Self) {
        let cut = (self.top + height).min(self.bottom);
        (
            Self {
                bottom: cut,
                ..self
            },
            Self {
                top: (cut + gap).min(self.bottom),
                ..self
            },
        )
    }

    /// The bottom `height`, and what is left above it after `gap`.
    pub fn take_bottom(self, height: f32, gap: f32) -> (Self, Self) {
        let cut = (self.bottom - height).max(self.top);
        (
            Self { top: cut, ..self },
            Self {
                bottom: (cut - gap).max(self.top),
                ..self
            },
        )
    }

    /// The left `width`, and what is left after `gap`.
    pub fn take_left(self, width: f32, gap: f32) -> (Self, Self) {
        let cut = (self.left + width).min(self.right);
        (
            Self {
                right: cut,
                ..self
            },
            Self {
                left: (cut + gap).min(self.right),
                ..self
            },
        )
    }

    /// The right `width`, and what is left after `gap`.
    pub fn take_right(self, width: f32, gap: f32) -> (Self, Self) {
        let cut = (self.right - width).max(self.left);
        (
            Self { left: cut, ..self },
            Self {
                right: (cut - gap).max(self.left),
                ..self
            },
        )
    }

    /// A `width`×`height` box placed in this one by `x` and `y`, on whole pixels.
    pub fn place(
        &self,
        width: f32,
        height: f32,
        x: crate::layout::Align,
        y: crate::layout::Align,
    ) -> Self {
        let left = x.offset(self.left, self.width(), width);
        let top = y.offset(self.top, self.height(), height);
        Self::new(left.round(), top.round(), width, height)
    }

    /// Whole pixels covering this rectangle, clipped to a `width`×`height` bitmap.
    pub fn pixels(&self, width: u32, height: u32) -> Option<PixelRect> {
        let left = self.left.floor().max(0.0) as u32;
        let top = self.top.floor().max(0.0) as u32;
        let right = (self.right.ceil().max(0.0) as u32).min(width);
        let bottom = (self.bottom.ceil().max(0.0) as u32).min(height);
        (right > left && bottom > top).then(|| PixelRect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// Whole pixels of a bitmap: what gets redrawn and copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub fn to_rect(self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

/// A straight-alpha colour (Direct2D brushes take straight alpha; the bitmap is premultiplied).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    /// Invisible but hit-testable. A layered window lets clicks through only where alpha is 0,
    /// so every surface fills its shape with at least this (docs/ui.md, "Hit-testing").
    pub const HIT: Self = Self::new(0.0, 0.0, 0.0, 1.0 / 255.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An 8-bit sRGB colour with alpha `a` (0..=1).
    pub fn rgb8([r, g, b]: [u8; 3], a: f32) -> Self {
        Self::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            a,
        )
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_cover_fractional_edges_and_clip_to_the_bitmap() {
        let r = Rect {
            left: 1.5,
            top: -3.0,
            right: 10.2,
            bottom: 4.0,
        };
        assert_eq!(
            r.pixels(8, 8),
            Some(PixelRect {
                x: 1,
                y: 0,
                width: 7,
                height: 4
            })
        );
        assert_eq!(Rect::new(20.0, 0.0, 5.0, 5.0).pixels(8, 8), None);
        assert_eq!(Rect::new(2.0, 2.0, 0.0, 5.0).pixels(8, 8), None);
    }
}
#[path = "continuous.rs"]
pub mod continuous;

