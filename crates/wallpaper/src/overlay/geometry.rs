//! Where an overlay's glass reads and draws, in plain rectangles (virtual-screen physical px
//! unless noted).

use mypaper_desktop::Rect;

use super::request::Material;

/// glass.wgsl's blur cap, in px.
pub const MAX_BLUR: f32 = 12.0;

/// The rim band's width (`band_of` in glass.wgsl).
fn band_of(width: f32, height: f32, radius: f32) -> f32 {
    let r = radius.min(width.min(height) / 2.0);
    (r * 0.5 + 5.0).min(width.min(height) / 2.0 * 0.45)
}

/// How far outside the surface the glass samples what is behind it: the rim's reach out, its
/// chromatic split, the blur and a filter pixel (as `glass_pass::regions`), rounded up.
pub fn reach(rect: Rect, radius: f32, material: &Material) -> i32 {
    let band = band_of(rect.width as f32, rect.height as f32, radius);
    let reach = band * material.refraction.clamp(0.0, 1.0) * 1.06
        + material.blur.clamp(0.0, MAX_BLUR)
        + 2.0;
    reach.ceil() as i32
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
    fn reach_covers_the_rims_pull_and_the_blur() {
        let menu = rect(0, 0, 280, 400);
        // Band min(12 * 0.5 + 5, 140 * 0.45) = 11; 11 * 0.6 * 1.06 + 2 = 9.0 -> 9.
        assert_eq!(reach(menu, 12.0, &Material::CLEAR), 9);
        let frosted = Material {
            blur: 30.0,
            ..Material::CLEAR
        };
        assert_eq!(reach(menu, 12.0, &frosted), 9 + 12, "blur caps at 12 px");
        let flat = Material {
            refraction: 0.0,
            ..Material::CLEAR
        };
        assert_eq!(reach(menu, 12.0, &flat), 2);
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
