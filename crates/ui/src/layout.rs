//! Layout helpers, so views stop hand-placing rectangles: insets, alignment, stacks that hand
//! out the next box along an axis, weighted rows, grids, and size classes for widgets.
//!
//! Everything works on [`Rect`]s in device pixels and allocates nothing. Components report
//! their intrinsic size (`Frame::button_size`, `Frame::row_height`, ...); a stack places them.
//!
//! ```ignore
//! let content = panel.inset(Insets::all(f.px(t.metrics.pad)));
//! let (header, rest) = content.take_top(f.px(32.0), f.px(8.0));
//! let mut column = Stack::column(rest, f.px(6.0));
//! for row in rows { f.row(&row, column.next(f.row_height(&row))); }
//! ```

use crate::geom::Rect;

/// Padding on each side.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    /// `x` left and right, `y` top and bottom.
    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            top: y,
            right: x,
            bottom: y,
            left: x,
        }
    }
}

/// Where a box sits along one axis of the space it's given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

impl Align {
    /// The start of a `size` box in the span `start..start + room`.
    pub fn offset(self, start: f32, room: f32, size: f32) -> f32 {
        match self {
            Self::Start => start,
            Self::Center => start + (room - size) / 2.0,
            Self::End => start + room - size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// Hands out boxes one after another along an axis, `gap` apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stack {
    pub rect: Rect,
    pub axis: Axis,
    pub gap: f32,
    /// Where the next box starts.
    pub cursor: f32,
}

impl Stack {
    /// Top to bottom.
    pub fn column(rect: Rect, gap: f32) -> Self {
        Self {
            rect,
            axis: Axis::Column,
            gap,
            cursor: rect.top,
        }
    }

    /// Left to right.
    pub fn row(rect: Rect, gap: f32) -> Self {
        Self {
            rect,
            axis: Axis::Row,
            gap,
            cursor: rect.left,
        }
    }

    /// The next box, `size` long along the axis and the full cross size.
    pub fn next(&mut self, size: f32) -> Rect {
        let rect = match self.axis {
            Axis::Column => Rect::new(self.rect.left, self.cursor, self.rect.width(), size),
            Axis::Row => Rect::new(self.cursor, self.rect.top, size, self.rect.height()),
        };
        self.cursor += size + self.gap;
        rect
    }

    /// Skips `space` more than the gap.
    pub fn space(&mut self, space: f32) {
        self.cursor += space;
    }

    /// What is left after the boxes handed out.
    pub fn rest(&self) -> Rect {
        match self.axis {
            Axis::Column => Rect {
                top: self.cursor.min(self.rect.bottom),
                ..self.rect
            },
            Axis::Row => Rect {
                left: self.cursor.min(self.rect.right),
                ..self.rect
            },
        }
    }

    /// How far the boxes reach (the content's extent: height of a column, width of a row).
    pub fn used(&self) -> f32 {
        let start = match self.axis {
            Axis::Column => self.rect.top,
            Axis::Row => self.rect.left,
        };
        (self.cursor - self.gap - start).max(0.0)
    }
}

/// A length in a [`split`]: fixed device px, or a share of what the fixed ones leave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Fixed(f32),
    Fill(f32),
}

/// Splits `rect` along `axis` into `lengths.len()` boxes, `gap` apart, into `out` (whole
/// pixels; fills share the rest by weight).
pub fn split(rect: Rect, axis: Axis, gap: f32, lengths: &[Length], out: &mut [Rect]) {
    let total = match axis {
        Axis::Row => rect.width(),
        Axis::Column => rect.height(),
    };
    let fixed: f32 = lengths
        .iter()
        .map(|l| match l {
            Length::Fixed(v) => *v,
            Length::Fill(_) => 0.0,
        })
        .sum();
    let weights: f32 = lengths
        .iter()
        .map(|l| match l {
            Length::Fill(w) => *w,
            Length::Fixed(_) => 0.0,
        })
        .sum();
    let free = (total - fixed - gap * lengths.len().saturating_sub(1) as f32).max(0.0);
    let mut stack = match axis {
        Axis::Row => Stack::row(rect, gap),
        Axis::Column => Stack::column(rect, gap),
    };
    for (length, slot) in lengths.iter().zip(out.iter_mut()) {
        let size = match length {
            Length::Fixed(v) => *v,
            Length::Fill(w) if weights > 0.0 => (free * w / weights).floor(),
            Length::Fill(_) => 0.0,
        };
        *slot = stack.next(size);
    }
}

/// Cell `index` of a grid of `columns` equal columns, rows `height` tall, `gap` apart.
pub fn grid(rect: Rect, columns: usize, gap: f32, height: f32, index: usize) -> Rect {
    let columns = columns.max(1);
    let width = ((rect.width() - gap * (columns - 1) as f32) / columns as f32).floor();
    let (col, row) = (index % columns, index / columns);
    Rect::new(
        rect.left + col as f32 * (width + gap),
        rect.top + row as f32 * (height + gap),
        width,
        height,
    )
}

/// How much room a widget has, so it can adapt its layout (design px of its smaller side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SizeClass {
    /// Under 150 px: one value, one glyph.
    Compact,
    /// Under 300 px: a title, a value and a detail line.
    Regular,
    /// Room for lists and charts.
    Large,
}

impl SizeClass {
    pub fn of(width: f32, height: f32) -> Self {
        let side = width.min(height);
        if side < 150.0 {
            Self::Compact
        } else if side < 300.0 {
            Self::Regular
        } else {
            Self::Large
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_splits_and_grids_place_boxes_without_overlap() {
        let mut column = Stack::column(Rect::new(10.0, 20.0, 100.0, 200.0), 8.0);
        assert_eq!(column.next(30.0), Rect::new(10.0, 20.0, 100.0, 30.0));
        assert_eq!(column.next(10.0).top, 58.0);
        assert_eq!(column.used(), 48.0);
        assert_eq!(column.rest().top, 76.0);
        let mut out = [Rect::default(); 3];
        split(
            Rect::new(0.0, 0.0, 300.0, 40.0),
            Axis::Row,
            10.0,
            &[Length::Fixed(40.0), Length::Fill(1.0), Length::Fill(2.0)],
            &mut out,
        );
        assert_eq!(out[0].width(), 40.0);
        assert_eq!(out[1].width(), 80.0);
        assert_eq!(out[2].width(), 160.0);
        assert_eq!(out[2].right, 300.0);
        let cell = grid(Rect::new(0.0, 0.0, 210.0, 400.0), 2, 10.0, 50.0, 3);
        assert_eq!(cell, Rect::new(110.0, 60.0, 100.0, 50.0));
        assert_eq!(SizeClass::of(160.0, 160.0), SizeClass::Regular);
        assert_eq!(SizeClass::of(340.0, 140.0), SizeClass::Compact);
    }
}
