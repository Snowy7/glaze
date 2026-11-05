//! Liquid Glass under a surface, drawn by the engine in a window of its own directly under ours
//! (plan, "Overlays"). This layer only says where: a [`Surface`](crate::Surface) with a
//! [`Glass`] tells the thread's [`Backdrop`] (the shell's overlay client) when it shows, moves,
//! reshapes and hides. A move is told **before** the window moves, so the engine moves its window
//! in the same composition frame.
//!
//! The surface still fills its whole body at least at [`Rgba::HIT`](crate::Rgba::HIT): clicks
//! land on it, and the glass under it is never hit. Without a backdrop (no engine, a golden
//! test) [`available`] is false and views draw their own fill instead.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::HWND;

use crate::geom::{Rect, Rgba};

/// The glass's material (plan, "Material controls"); values are 0..=1 unless noted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassMaterial {
    /// 1 is clear glass, 0 fully tinted (iOS 27's slider, `docs/glass.md`).
    pub clarity: f32,
    /// Frost. Themes and [`crate::theme::Knobs`] give it in design px; what reaches the engine
    /// (a plate, `Surface::set_glass`) is physical px, the surface's scale times that.
    pub blur: f32,
    /// How far the rim bends what is behind it.
    pub refraction: f32,
    /// The specular rim's strength.
    pub highlight: f32,
    /// The tint colour; its alpha is the stained tint's strength
    /// ([`GlassMaterial::tuned`] fades it with clarity).
    pub tint: Rgba,
}

/// A surface's glass: its body in the surface's pixels, and its corner radius in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    pub body: Rect,
    pub radius: f32,
    pub material: GlassMaterial,
}

/// A glass shape on screen: physical virtual-screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Glass {
    /// The body on screen, for a window whose top-left is at `origin`.
    pub fn shape(&self, origin: (i32, i32)) -> Shape {
        let left = self.body.left.round() as i32;
        let top = self.body.top.round() as i32;
        Shape {
            x: origin.0 + left,
            y: origin.1 + top,
            width: (self.body.right.round() as i32 - left).max(1),
            height: (self.body.bottom.round() as i32 - top).max(1),
        }
    }
}

/// Where glass requests go: the engine, through the shell's overlay client. Every call is for
/// `client`, the surface's window.
pub trait Backdrop {
    /// Whether glass can show now (the engine runs). Views draw a fill of their own when not.
    fn available(&self) -> bool;
    /// Creates the glass under `client`; `false` when it was refused.
    fn open(&self, client: HWND, shape: Shape, radius: f32, material: &GlassMaterial) -> bool;
    fn place(&self, client: HWND, shape: Shape, radius: f32);
    fn material(&self, client: HWND, material: &GlassMaterial);
    fn close(&self, client: HWND);
}

thread_local! {
    static BACKDROP: RefCell<Option<Rc<dyn Backdrop>>> = const { RefCell::new(None) };
    /// The material popups use unless their tokens say otherwise (the active style's), or
    /// `None` for styles without blur (solid, mono), which draw their own fill.
    static STYLE: RefCell<Option<GlassMaterial>> = const { RefCell::new(None) };
}

/// Sends this thread's glass requests to `backdrop` from now on.
pub fn install(backdrop: Rc<dyn Backdrop>) {
    BACKDROP.with_borrow_mut(|b| *b = Some(backdrop));
}

/// Sets the popups' material for the active style (`None`: no glass, a drawn fill).
pub fn set_style(material: Option<GlassMaterial>) {
    STYLE.with_borrow_mut(|s| *s = material);
}

/// The popups' material, if glass can show now.
pub fn style() -> Option<GlassMaterial> {
    STYLE.with_borrow(|s| *s).filter(|_| available())
}

/// Whether a backdrop is installed and can show glass now.
pub fn available() -> bool {
    backdrop().is_some_and(|b| b.available())
}

pub(crate) fn backdrop() -> Option<Rc<dyn Backdrop>> {
    BACKDROP.with_borrow(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shape_is_the_body_on_screen_in_whole_pixels() {
        let glass = Glass {
            body: Rect::new(14.0, 14.0, 240.4, 180.6),
            radius: 10.0,
            material: GlassMaterial {
                clarity: 1.0,
                blur: 0.0,
                refraction: 0.6,
                highlight: 0.5,
                tint: Rgba::TRANSPARENT,
            },
        };
        assert_eq!(
            glass.shape((-2000, 300)),
            Shape {
                x: -1986,
                y: 314,
                width: 240,
                height: 181,
            }
        );
    }
}
