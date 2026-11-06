//! v16's opaque surface steps. No shared kit defaults are changed.

use mypaper_ui::Rgba;
use mypaper_ui::controls::Palette;

pub fn hex(v: u32) -> Rgba {
    Rgba::rgb8([(v >> 16) as u8, (v >> 8) as u8, v as u8], 1.0)
}

pub fn thumb(dark: bool) -> Rgba {
    hex(if dark { 0x6b6b6b } else { 0x5e5e5e })
}

pub fn palette(dark: bool) -> Palette {
    let mut p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let choose = |d, l| hex(if dark { d } else { l });
    p.chrome = choose(0x141414, 0xe8e8e8);
    p.bg = p.chrome;
    p.panel = choose(0x232323, 0xffffff);
    p.fill = choose(0x333333, 0xececec);
    p.well = p.fill;
    p.hover = choose(0x2c2c2c, 0xf2f2f2);
    p.selected = choose(0x4a4a4a, 0xd4d4d4);
    p.line = choose(0x2f2f2f, 0xe4e4e4);
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
