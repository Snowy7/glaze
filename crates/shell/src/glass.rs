//! Sole plate-publishing boundary. N2's transport stub is replaced by `glass-ipc` at integration.
//! Rectangles are physical pixels relative to the monitor; motion is entirely shell-owned.
use mypaper_desktop::Monitor;
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
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
}

/// Replace a monitor's complete plate list, including `[]` when its surfaces disappear.
/// Callers compare with the last publication, so this is never called at rest.
pub fn publish(_monitor: &Monitor, _plates: &[Plate]) {
    static WARN: std::sync::Once = std::sync::Once::new();
    WARN.call_once(|| log::warn!("glass-ipc is not in this base: plate transport is a local stub"));
}
