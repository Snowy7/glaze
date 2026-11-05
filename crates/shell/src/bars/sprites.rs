//! The dock over DirectComposition: everything it draws but its hit mask (icons, extras,
//! badges, running dots, dividers) is a sprite, made once and then placed and scaled by DWM on
//! each frame. The layered window keeps only the invisible hit fills, laid out so a magnification
//! frame damages little of it (the plate in fixed tiles; hit under magnified icons only).
use anyhow::Result;
use mypaper_ui::{
    DisplayList, Image, Op, Rect, Rgba, Ui,
    compose::{Compositor, Sprite},
};
use std::sync::Arc;
use windows::Win32::Foundation::HWND;

/// How a cached sprite was made: so a frame finds the one it needs.
enum Key {
    /// An icon bitmap, shown at any size (DWM scales it).
    Image(Arc<Image>),
    /// Any other operation, moved to its own canvas's origin: shown 1:1.
    Op(Op),
}

pub struct Sprites {
    compositor: Compositor,
    /// Each sprite made, how, and whether this frame uses it.
    cache: Vec<(Key, Sprite, bool)>,
}

/// Plate tiles' length along the dock: fixed window positions, so a growing plate redraws only
/// its ends.
const TILE: f32 = 64.0;

impl Sprites {
    pub fn new(hwnd: HWND) -> Result<Self> {
        Ok(Self {
            compositor: Compositor::new(hwnd)?,
            cache: vec![],
        })
    }

    /// Puts `list`'s visible operations on screen as sprites and returns the hit mask for the
    /// window's own pixels; `None` (sprites hidden) when the list has something sprites cannot
    /// show alone (clips, text fields), to be drawn as before.
    pub fn update(&mut self, ui: &mut Ui, list: &DisplayList) -> Result<Option<DisplayList>> {
        if list.ops.iter().any(|op| {
            matches!(
                op,
                Op::Clip { .. } | Op::Unclip | Op::Clipped { .. } | Op::TextField { .. }
            )
        }) {
            self.hide()?;
            return Ok(None);
        }
        let plates: Vec<Rect> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Fill { rect, color, .. } if *color == Rgba::HIT => Some(*rect),
                _ => None,
            })
            .collect();
        let mut mask = Vec::new();
        let mut placed = Vec::new();
        for (_, _, used) in &mut self.cache {
            *used = false;
        }
        let mut previous: Option<&Op> = None;
        for op in &list.ops {
            let (index, rect, opacity) = match op {
                Op::Fill {
                    rect,
                    radius,
                    color,
                } if *color == Rgba::HIT => {
                    // A plate's hit fill comes twice (the body's and the glass surface's).
                    if previous != Some(op) {
                        tile(*rect, *radius, &mut mask);
                    }
                    previous = Some(op);
                    continue;
                }
                Op::Image {
                    rect,
                    image,
                    opacity,
                    source: None,
                } => (self.image(image)?, *rect, *opacity),
                // A magnified glyph (the extras) keeps its stroke weight as it grows, so it is
                // drawn at each size, on whole pixels (≤ half a pixel off), not scaled.
                Op::Icon {
                    rect,
                    icon,
                    weight,
                    color,
                } => {
                    let side = rect.width().round();
                    let snapped = Op::Icon {
                        rect: Rect::new(
                            (rect.left + (rect.width() - side) / 2.0).round(),
                            (rect.top + (rect.height() - side) / 2.0).round(),
                            side,
                            side,
                        ),
                        icon: *icon,
                        weight: *weight,
                        color: *color,
                    };
                    let (index, at) = self.op(ui, &snapped)?;
                    (index, at, 1.0)
                }
                other => {
                    let (index, at) = self.op(ui, other)?;
                    (index, at, 1.0)
                }
            };
            self.cache[index].2 = true;
            // Sprites stay clickable where they reach past the plate (as their pixels were).
            if !plates.iter().any(|p| contains(p, &rect)) {
                mask.push(Op::Fill {
                    rect,
                    radius: 0.0,
                    color: Rgba::HIT,
                });
            }
            placed.push((index, rect, opacity));
        }
        let shown: Vec<(&Sprite, Rect, f32)> = placed
            .iter()
            .map(|&(index, rect, opacity)| (&self.cache[index].1, rect, opacity))
            .collect();
        self.compositor.show(&shown)?;
        // Long-lived sprites only: drop the unused when the cache grows.
        if self.cache.len() > 160 {
            self.cache.retain(|(_, _, used)| *used);
        }
        Ok(Some(DisplayList {
            ops: mask,
            hits: list.hits.clone(),
            focus: list.focus,
        }))
    }

    /// Takes every sprite off screen (the window draws its full list again).
    pub fn hide(&mut self) -> Result<()> {
        self.compositor.show(&[])
    }

    fn image(&mut self, image: &Arc<Image>) -> Result<usize> {
        if let Some(i) = self
            .cache
            .iter()
            .position(|(k, ..)| matches!(k, Key::Image(m) if Arc::ptr_eq(m, image)))
        {
            return Ok(i);
        }
        let sprite = self.compositor.sprite(image)?;
        self.cache.push((Key::Image(image.clone()), sprite, true));
        Ok(self.cache.len() - 1)
    }

    /// `op` drawn on its own canvas (at the same subpixel offset), and where that canvas goes.
    fn op(&mut self, ui: &mut Ui, op: &Op) -> Result<(usize, Rect)> {
        let bounds = op.bounds();
        let (x, y) = (bounds.left.floor() - 1.0, bounds.top.floor() - 1.0);
        let (w, h) = (
            (bounds.right.ceil() + 1.0 - x).max(1.0),
            (bounds.bottom.ceil() + 1.0 - y).max(1.0),
        );
        let moved = DisplayList {
            ops: vec![op.clone()],
            ..Default::default()
        }
        .translated(-x, -y)
        .ops
        .remove(0);
        let at = Rect::new(x, y, w, h);
        if let Some(i) = self
            .cache
            .iter()
            .position(|(k, ..)| matches!(k, Key::Op(o) if *o == moved))
        {
            return Ok((i, at));
        }
        let list = DisplayList {
            ops: vec![moved.clone()],
            ..Default::default()
        };
        let image = mypaper_ui::render(ui, &list, w as u32, h as u32)?;
        let sprite = self.compositor.sprite(&image)?;
        self.cache.push((Key::Op(moved), sprite, true));
        Ok((self.cache.len() - 1, at))
    }
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

/// A hit fill as fixed-position square tiles along its length, with its rounded ends: as the
/// plate grows only the ends change. (Tiles overlap the ends by the radius; two hit fills over
/// each other are still invisible.)
fn tile(rect: Rect, radius: f32, out: &mut Vec<Op>) {
    let horizontal = rect.width() >= rect.height();
    let (from, to) = if horizontal {
        (rect.left, rect.right)
    } else {
        (rect.top, rect.bottom)
    };
    let a = ((from + radius) / TILE).ceil() * TILE;
    let b = ((to - radius) / TILE).floor() * TILE;
    let span = |s: f32, e: f32| {
        if horizontal {
            Rect::new(s, rect.top, e - s, rect.height())
        } else {
            Rect::new(rect.left, s, rect.width(), e - s)
        }
    };
    let fill = |rect: Rect, radius: f32| Op::Fill {
        rect,
        radius,
        color: Rgba::HIT,
    };
    if b - a < TILE {
        out.push(fill(rect, radius));
        return;
    }
    out.push(fill(span(from, a + radius), radius));
    let mut at = a;
    while at < b {
        out.push(fill(span(at, at + TILE), 0.0));
        at += TILE;
    }
    out.push(fill(span(b - radius, to), radius));
}
