//! Physical-pixel geometry for Studio's monochrome desktop chrome.
use mypaper_bars::config::{BarsConfig, DockPosition};
use mypaper_desktop::Monitor;
use mypaper_ui::{Op, Point, Rect, Rgba};

pub(super) fn frame(rect: Rect, scale: f32, selected: bool) -> Vec<Op> {
    let mut ops = Vec::new();
    if selected {
        ops.push(Op::Stroke {
            rect,
            radius: 10.0 * scale,
            width: 4.0 * scale,
            color: Rgba::rgb8([20, 20, 20], 1.0),
        });
    }
    ops.push(Op::Stroke {
        rect,
        radius: 10.0 * scale,
        width: if selected { 1.5 } else { 1.0 } * scale,
        color: Rgba::rgb8([245, 245, 245], if selected { 1.0 } else { 0.45 }),
    });
    if selected {
        for (width, color) in [(5.0, [20, 20, 20]), (2.0, [255, 255, 255])] {
            for (x, dx) in [(rect.left, 1.0), (rect.right, -1.0)] {
                for (y, dy) in [(rect.top, 1.0), (rect.bottom, -1.0)] {
                    for to in [
                        Point::new(x + dx * 13.0 * scale, y),
                        Point::new(x, y + dy * 13.0 * scale),
                    ] {
                        ops.push(Op::Line {
                            from: Point::new(x, y),
                            to,
                            width: width * scale,
                            color: Rgba::rgb8(color, 1.0),
                        });
                    }
                }
            }
        }
    }
    ops
}

/// OS work area intersected with mypaper's actual bar bands. Reserve the reveal dock too.
pub(super) fn work_area(monitor: &Monitor, bars: &BarsConfig) -> Rect {
    let mut work = crate::widgets::usable_screen(monitor, bars);
    let bars = bars
        .for_monitor(&monitor.device, monitor.primary)
        .unwrap_or_else(|_| bars.clone());
    if bars.dock.enabled && bars.dock.autohide {
        let band = bars.dock.band() as f32 * monitor.dpi as f32 / 96.0;
        match bars.dock.position {
            DockPosition::Bottom => {
                work.bottom = work
                    .bottom
                    .min((monitor.rect.y + monitor.rect.height) as f32 - band)
            }
            DockPosition::Top => work.top = work.top.max(monitor.rect.y as f32 + band),
            DockPosition::Left => work.left = work.left.max(monitor.rect.x as f32 + band),
            DockPosition::Right => {
                work.right = work
                    .right
                    .min((monitor.rect.x + monitor.rect.width) as f32 - band)
            }
        }
    }
    work
}

pub(super) fn name_position(
    target: Rect,
    (width, height): (f32, f32),
    work: Option<Rect>,
    scale: f32,
) -> Point {
    let gap = 4.0 * scale;
    let y = if work.is_some_and(|w| target.top - height - gap < w.top) {
        target.bottom + gap
    } else {
        target.top - height - gap
    };
    let Some(work) = work else {
        return Point::new(target.left, y);
    };
    Point::new(
        target
            .left
            .clamp(work.left, (work.right - width).max(work.left)),
        y.clamp(work.top, (work.bottom - height).max(work.top)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_work_area_on_all_edges_and_negative_displays() {
        let work = Rect::new(-2560.0, 40.0, 2560.0, 1300.0);
        for target in [
            Rect::new(-2560.0, 9.0, 200.0, 160.0),
            Rect::new(-100.0, 1320.0, 100.0, 120.0),
            work,
        ] {
            let p = name_position(target, (350.0, 24.0), Some(work), 1.0);
            assert!(p.x >= work.left && p.x + 350.0 <= work.right);
            assert!(p.y >= work.top && p.y + 24.0 <= work.bottom);
        }
    }

    #[test]
    fn selected_stroke_has_an_opaque_dark_halo_and_white_line() {
        let ops = frame(Rect::new(2.0, 2.0, 100.0, 100.0), 1.0, true);
        assert!(
            matches!(&ops[0], Op::Stroke { width: 4.0, color, .. } if *color == Rgba::rgb8([20, 20, 20], 1.0))
        );
        assert!(
            matches!(&ops[1], Op::Stroke { color, .. } if *color == Rgba::rgb8([245, 245, 245], 1.0))
        );
        assert_eq!(
            frame(Rect::new(2.0, 2.0, 100.0, 100.0), 1.0, false).len(),
            1
        );
    }
}
