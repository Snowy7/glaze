//! One monochrome One UI palette for the standalone window and docked panel.

use glaze_ui::Rgba;
use glaze_ui::controls::Palette;

/// Studio's radius scale, independent of the desktop style being edited.
pub mod radius {
    pub const XS: f32 = 4.0;
    pub const SMALL: f32 = 6.0;
    pub const CONTROL: f32 = 8.0;
    pub const MEDIUM: f32 = 12.0;
    pub const CARD: f32 = 16.0;
}

pub fn hex(v: u32) -> Rgba {
    Rgba::rgb8([(v >> 16) as u8, (v >> 8) as u8, v as u8], 1.0)
}

#[cfg(test)]
pub use glaze_ui::controls::segment_fill as thumb;

pub fn palette(dark: bool) -> Palette {
    let mut p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let choose = |d, l| hex(if dark { d } else { l });
    p.chrome = choose(0x050505, 0xf2f2f2);
    p.bg = choose(0x0b0b0b, 0xe9e9e9);
    p.panel = choose(0x1b1b1b, 0xffffff);
    p.fill = choose(0x2b2b2b, 0xe3e3e3);
    p.well = p.fill;
    p.hover = choose(0x262626, 0xe8e8e8);
    p.selected = choose(0x3a3a3a, 0xd4d4d4);
    p.line = choose(0x303030, 0xe0e0e0);
    p.edge = choose(0x777777, 0x8a8a8a);
    p.ink1 = choose(0xf5f5f5, 0x0a0a0a);
    p.ink2 = choose(0xc8c8c8, 0x333333);
    p.ink3 = choose(0xababab, 0x555555);
    p.ink4 = p.ink3;
    p.solid = p.ink1;
    p.on_solid = p.chrome;
    p.solid_hover = p.ink2;
    p
}
