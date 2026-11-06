//! One monochrome One UI palette for the standalone window and docked panel.

use mypaper_ui::Rgba;
use mypaper_ui::controls::Palette;

pub fn hex(v: u32) -> Rgba {
    Rgba::rgb8([(v >> 16) as u8, (v >> 8) as u8, v as u8], 1.0)
}

#[cfg(test)]
pub use mypaper_ui::controls::segment_fill as thumb;

pub fn palette(dark: bool) -> Palette {
    let mut p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let choose = |d, l| hex(if dark { d } else { l });
    p.chrome = choose(0x050505, 0xf2f2f2);
    p.bg = choose(0x0d0d0d, 0xe9e9e9);
    p.panel = choose(0x171717, 0xffffff);
    p.fill = choose(0x262626, 0xececec);
    p.well = p.fill;
    p.hover = choose(0x222222, 0xe5e5e5);
    p.selected = choose(0x383838, 0xd4d4d4);
    p.line = choose(0x2a2a2a, 0xe4e4e4);
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
