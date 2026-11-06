//! Continuous surface corners, using the engine's `smooth_corner` superellipse (scene.rs).
//! The radius budget, extent and exponent must stay identical on both sides of the glass.
use crate::{DisplayList, Op, Point, Rect};

/// Effective corner extent and exponent, matching liquid-rust's glass shader.
pub fn corner(radius: f32, smoothing: f32, budget: f32) -> (f32, f32) {
    let r = radius.min(budget).max(0.0);
    if r <= 0.0 {
        return (0.0, 2.0);
    }
    let s = smoothing.clamp(0.0, 1.0).min((budget / r - 1.0).max(0.0));
    (
        r * (1.0 + 0.42 * s + 0.18 * s * s),
        2.0 + 0.95 * s + 0.6 * s * s,
    )
}

/// Clockwise contour. Subpixel segments are cached by the software renderer per geometry.
pub(crate) fn outline(rect: Rect, radius: f32, smoothing: f32) -> Vec<Point> {
    let (r, n) = corner(radius, smoothing, rect.width().min(rect.height()) * 0.5);
    let mut points = Vec::with_capacity(260);
    for (cx, cy, quadrant) in [
        (rect.right - r, rect.bottom - r, 0),
        (rect.left + r, rect.bottom - r, 1),
        (rect.left + r, rect.top + r, 2),
        (rect.right - r, rect.top + r, 3),
    ] {
        for step in 0..=64 {
            let angle = (quadrant as f32 + step as f32 / 64.0) * std::f32::consts::FRAC_PI_2;
            let power = |v: f32| v.signum() * v.abs().powf(2.0 / n);
            points.push(Point::new(
                cx + r * power(angle.cos()),
                cy + r * power(angle.sin()),
            ));
        }
    }
    points
}

/// Changes only a body's fills and outline; controls inside keep their existing shapes.
/// The invisible fill is also the layered window's hit region.
pub fn surface(list: &mut DisplayList, body: Rect, smoothing: f32) {
    if smoothing <= 0.0 {
        return;
    }
    for op in &mut list.ops {
        let replacement = match *op {
            Op::Fill {
                rect,
                radius,
                color,
            } if rect == body => Some(Op::Continuous {
                rect,
                radius,
                smoothing,
                width: 0.0,
                color,
            }),
            Op::Stroke {
                rect,
                radius,
                width,
                color,
            } if rect.inflate(width * 0.5) == body => Some(Op::Continuous {
                rect,
                radius,
                smoothing,
                width,
                color,
            }),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *op = replacement;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_corner_matches_engine_extent_and_preserves_capsules() {
        let (r, n) = corner(12.0, 0.6, 50.0);
        assert!((r - 15.8016).abs() < 0.0001);
        assert!((n - 2.786).abs() < 0.0001);
        assert_eq!(corner(16.0, 0.6, 16.0), (16.0, 2.0));
        let body = Rect::new(7.0, 9.0, 200.0, 100.0);
        let points = outline(body, 12.0, 0.6);
        for p in points {
            assert!(p.x >= body.left - 0.001 && p.x <= body.right + 0.001);
            assert!(p.y >= body.top - 0.001 && p.y <= body.bottom + 0.001);
        }
    }

    #[test]
    fn continuous_hit_fill_clears_corners_and_translates_with_its_surface() {
        let mut ui = crate::Ui::new().unwrap();
        let body = Rect::new(8.0, 8.0, 120.0, 80.0);
        let mut list = DisplayList {
            ops: vec![Op::Fill {
                rect: body,
                radius: 20.0,
                color: crate::Rgba::HIT,
            }],
            ..Default::default()
        };
        surface(&mut list, body, 0.6);
        let image = crate::render(&mut ui, &list, 144, 104).unwrap();
        assert_eq!(
            image.pixel(8, 8)[3],
            0,
            "outside corner passes clicks through"
        );
        assert_eq!(image.pixel(68, 8)[3], 1, "straight edge remains hit");
        assert_eq!(image.pixel(68, 48)[3], 1, "glass body takes clicks");
        let moved = list.translated(10.0, 5.0);
        assert!(
            matches!(moved.ops[0], Op::Continuous { rect, .. } if rect == body.offset(10.0, 5.0))
        );
        assert_eq!(list.damage(&list), None, "settled contours draw nothing");
    }
}
