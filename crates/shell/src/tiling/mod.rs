//! Opt-in native tiling components; runtime activation is wired in a later step.
mod platform;
mod rules;
mod learned;
mod parking;
pub mod animation;
use mypaper_tiling::Rect;
fn near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 2.0
        && (a.y - b.y).abs() < 2.0
        && (a.w - b.w).abs() < 2.0
        && (a.h - b.h).abs() < 2.0
}
mod overview;
