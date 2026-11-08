//! Off-view placement is outside the bounding box of every connected output.
use mypaper_tiling::Rect;
pub(super) fn monitor_rect(r: mypaper_desktop::Rect) -> Rect {
    Rect {
        x: f64::from(r.x),
        y: f64::from(r.y),
        w: f64::from(r.width),
        h: f64::from(r.height),
    }
}
pub(super) fn bounds(monitors: &[mypaper_desktop::Monitor]) -> Option<Rect> {
    let first = monitors.first()?;
    let mut r = monitor_rect(first.rect);
    for m in monitors {
        let m = monitor_rect(m.rect);
        let right = (r.x + r.w).max(m.x + m.w);
        let bottom = (r.y + r.h).max(m.y + m.h);
        r.x = r.x.min(m.x);
        r.y = r.y.min(m.y);
        r.w = right - r.x;
        r.h = bottom - r.y;
    }
    Some(r)
}
/// Adjacent to the virtual-screen bounding box, avoiding WM_MOVE's signed 16-bit wrap.
pub(super) fn park(bounds: Rect, window: Rect) -> Rect {
    let right = bounds.x + bounds.w + 8.0;
    let bottom = bounds.y + bounds.h + 8.0;
    if right + window.w < 32767.0 {
        Rect {
            x: right,
            y: bounds.y,
            ..window
        }
    } else if bounds.x - window.w - 8.0 >= -32768.0 {
        Rect {
            x: bounds.x - window.w - 8.0,
            y: bounds.y,
            ..window
        }
    } else {
        Rect {
            x: bounds.x,
            y: bottom,
            ..window
        }
    }
}
