//! Drawing endpoints stay in workspace coordinates; native parking coordinates never drive a proxy.
use glaze_tiling::{Axis, Direction, Layout, Rect, Target};

/// Preserve off-view spacing, extending a partly visible endpoint until its whole proxy leaves.
fn outside(mut rect: Rect, clip: Rect, direction: Direction, gap: f64) -> Rect {
    match direction {
        Direction::Left => rect.x = rect.x.min(clip.x - rect.w - gap),
        Direction::Right => rect.x = rect.x.max(clip.x + clip.w + gap),
        Direction::Up => rect.y = rect.y.min(clip.y - rect.h - gap),
        Direction::Down => rect.y = rect.y.max(clip.y + clip.h + gap),
    }
    rect
}

/// Workspaces move across the strip axis; columns leave along it, including on portrait outputs.
pub(super) fn endpoint(layout: &Layout, target: &Target) -> Rect {
    let Some((mi, wi)) = layout.locate(target.window) else {
        return target.rect;
    };
    let monitor = &layout.monitors[mi];
    let active = monitor
        .workspaces
        .iter()
        .position(|w| w.id == monitor.active)
        .unwrap_or(wi);
    let gap = layout.settings.gaps * monitor.scale;
    let mut rect = target.rect;
    if wi != active {
        let delta = wi as f64 - active as f64;
        return match monitor.axis {
            Axis::Horizontal => {
                rect.y += delta * monitor.full.h;
                outside(
                    rect,
                    monitor.full,
                    if delta < 0.0 {
                        Direction::Up
                    } else {
                        Direction::Down
                    },
                    gap,
                )
            }
            Axis::Vertical => {
                rect.x += delta * monitor.full.w;
                outside(
                    rect,
                    monitor.full,
                    if delta < 0.0 {
                        Direction::Left
                    } else {
                        Direction::Right
                    },
                    gap,
                )
            }
        };
    }
    if target.visible {
        return rect;
    }
    let direction = match monitor.axis {
        Axis::Horizontal if rect.x < monitor.area.x => Direction::Left,
        Axis::Horizontal if rect.x + rect.w > monitor.area.x + monitor.area.w => Direction::Right,
        Axis::Vertical if rect.y < monitor.area.y => Direction::Up,
        Axis::Vertical if rect.y + rect.h > monitor.area.y + monitor.area.h => Direction::Down,
        _ => {
            // Inactive tabs occupy the same logical rectangle as the active tab.
            let tiles = monitor.workspaces[wi].columns.iter().find_map(|c| {
                c.tiles
                    .iter()
                    .position(|t| t.id == target.window)
                    .map(|index| (index, c.active))
            });
            match (
                monitor.axis,
                tiles.is_some_and(|(index, active)| index < active),
            ) {
                (Axis::Horizontal, true) => Direction::Up,
                (Axis::Horizontal, false) => Direction::Down,
                (Axis::Vertical, true) => Direction::Left,
                (Axis::Vertical, false) => Direction::Right,
            }
        }
    };
    outside(rect, monitor.full, direction, gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glaze_tiling::{Command, Constraints, Settings, WindowId};
    fn layout(rect: Rect) -> Layout {
        let mut layout = Layout::new(Settings::default());
        layout.add_monitor("test".into(), rect, rect, 1.0);
        layout
    }
    fn open(layout: &mut Layout, id: u64) {
        layout.insert(WindowId(id), Constraints::default(), None, None, true);
    }
    #[test]
    fn outgoing_columns_leave_the_correct_edge_without_using_native_parking() {
        for rect in [
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1200.0,
                h: 800.0,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 1200.0,
            },
        ] {
            let mut layout = layout(rect);
            for id in 1..=3 {
                open(&mut layout, id);
            }
            let target = layout
                .targets()
                .into_iter()
                .find(|t| t.window == WindowId(1))
                .unwrap();
            assert!(!target.visible);
            let end = endpoint(&layout, &target);
            if rect.w > rect.h {
                assert!(end.x + end.w < rect.x);
            } else {
                assert!(end.y + end.h < rect.y);
            }
        }
    }
    #[test]
    fn workspaces_leave_on_the_perpendicular_axis() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 1200.0,
            h: 800.0,
        };
        let mut layout = layout(rect);
        open(&mut layout, 1);
        layout.command(Command::Workspace(Direction::Down));
        let target = layout
            .targets()
            .into_iter()
            .find(|t| t.window == WindowId(1))
            .unwrap();
        let end = endpoint(&layout, &target);
        assert!(end.y + end.h < rect.y);
        assert_eq!(end.x, target.rect.x);
    }
}
