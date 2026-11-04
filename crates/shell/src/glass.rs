//! N4 integration seam. N2 replaces `publish` with shared-memory glass transport. Keep widget
//! geometry in monitor-local physical pixels; callers publish changes and active gesture frames.
use mypaper_ui::{Rect, Rgba};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub clarity: f32,
    pub blur: f32,
    pub refraction: f32,
    pub highlight: f32,
    pub tint: Rgba,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    /// Native client HWND encoded as u64: stable per surface, and available for WGC exclusions.
    pub id: u64,
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
}
pub fn publish(_monitor: &str, _plates: &[Plate]) {}
