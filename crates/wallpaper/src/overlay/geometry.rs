//! Where an overlay's glass reads and draws, in plain rectangles (virtual-screen physical px
//! unless noted).

use mypaper_desktop::Rect;

use super::request::Material;

/// How far outside the surface the glass draws (its drop shadow) or samples what is behind it
/// (the bevel's refraction and the frost), in px on a screen of `scale` px per point:
/// liquid-rust's `Material::outset`, rounded up.
pub fn reach(rect: Rect, material: &Material, scale: f32) -> i32 {
    let s = scale.max(0.5);
    let (glass, appearance) = crate::material::liquid(&super::glass::controls(material), s);
    let min_side = rect.width.min(rect.height).max(1) as f32 / s;
    (glass.outset(min_side, &appearance) * s).ceil() as i32
}

/// The overlay window: the surface grown by `reach` on every side, kept on its monitor (the
/// capture ends there; samples past the edge clamp to it).
pub fn frame(rect: Rect, reach: i32, monitor: Rect) -> Rect {
    let x0 = (rect.x - reach).max(monitor.x);
    let y0 = (rect.y - reach).max(monitor.y);
    let x1 = (rect.x + rect.width + reach).min(monitor.x + monitor.width);
    let y1 = (rect.y + rect.height + reach).min(monitor.y + monitor.height);
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(1),
        height: (y1 - y0).max(1),
    }
}

/// `rect` in `origin`'s coordinates.
pub fn local(rect: Rect, origin: Rect) -> Rect {
    Rect {
        x: rect.x - origin.x,
        y: rect.y - origin.y,
        ..rect
    }
}

/// Whether two rects share a pixel.
pub fn touches(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: -2560,
        y: 9,
        width: 2560,
        height: 1440,
    };

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn reach_covers_the_shadow_and_scales_with_dpi() {
        let menu = rect(0, 0, 420, 600);
        let at = |scale| reach(menu, &Material::CLEAR, scale);
        // A full-thickness regular panel: its shadow (8 pt down, blur 15 + 33 pt) reaches
        // furthest: 8 + 1.5 * 48 + 1 = 81 pt.
        assert_eq!(at(1.0), 81);
        assert!(at(1.5) as f32 > at(1.0) as f32 * 1.3, "points, not pixels");
        // Tinted (the slider thins frost on clear glass), with the most frost there is.
        let frosted = Material {
            clarity: 0.0,
            blur: 300.0,
            ..Material::CLEAR
        };
        assert!(reach(menu, &frosted, 1.0) > at(1.0), "heavy frost samples further");
    }

    #[test]
    fn the_frame_grows_by_the_reach_and_stays_on_its_monitor() {
        let middle = rect(-2000, 300, 280, 400);
        assert_eq!(frame(middle, 9, SCREEN), rect(-2009, 291, 298, 418));
        // Flush with the monitor's top-left corner: nothing grows past it.
        let corner = rect(-2560, 9, 280, 36);
        assert_eq!(frame(corner, 9, SCREEN), rect(-2560, 9, 289, 45));
    }

    #[test]
    fn touching_needs_a_shared_pixel() {
        let a = rect(10, 10, 100, 50);
        assert!(touches(a, rect(109, 59, 5, 5)));
        assert!(!touches(a, rect(110, 10, 5, 5)), "edge to edge");
        assert!(!touches(a, rect(10, 60, 100, 5)));
        assert_eq!(local(a, rect(-5, 4, 1, 1)), rect(15, 6, 100, 50));
    }
}
