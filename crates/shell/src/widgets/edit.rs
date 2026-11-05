//! Grid placement and edit chrome. Layout is committed only at the end of a gesture; a rejected
//! drop leaves the saved layout intact. Motion itself stays in physical pixels, at display rate.
use mypaper_ui::{DisplayList, Hit, Op, Rect, Rgba, Role, TextStyle};
use mypaper_widgets::layout::{Grid, Instance};

pub fn overlaps(a: [u32; 2], size: [u32; 2], b: [u32; 2], other: [u32; 2]) -> bool {
    (0..2).all(|i| a[i] < b[i] + other[i] && b[i] < a[i] + size[i])
}
pub fn free(
    grid: &Grid,
    cell: [u32; 2],
    size: [u32; 2],
    instances: &[Instance],
    monitor: &str,
    except: Option<&str>,
) -> bool {
    grid.clamp(cell, size) == (cell, size)
        && !instances.iter().any(|i| {
            i.monitor == monitor
                && Some(i.instance.as_str()) != except
                && overlaps(cell, size, i.cell, i.size)
        })
}
pub fn vacancy(
    grid: &Grid,
    size: [u32; 2],
    instances: &[Instance],
    monitor: &str,
) -> Option<[u32; 2]> {
    (0..grid.rows)
        .flat_map(|row| (0..grid.cols).map(move |col| [col, row]))
        .find(|&cell| free(grid, cell, size, instances, monitor, None))
}
pub fn snapped(grid: &Grid, rect: Rect, scale: f64) -> ([u32; 2], [u32; 2]) {
    let cell = [
        ((rect.left as f64 / scale - grid.origin[0]) / (grid.cell[0] + grid.gap))
            .round()
            .max(0.0) as u32,
        ((rect.top as f64 / scale - grid.origin[1]) / (grid.cell[1] + grid.gap))
            .round()
            .max(0.0) as u32,
    ];
    let size = [
        ((rect.width() as f64 / scale + grid.gap) / (grid.cell[0] + grid.gap))
            .round()
            .max(1.0) as u32,
        ((rect.height() as f64 / scale + grid.gap) / (grid.cell[1] + grid.gap))
            .round()
            .max(1.0) as u32,
    ];
    grid.clamp(cell, size)
}

/// Old arranging policy: one neighbor swaps into the original cell on a move; otherwise
/// overlapped neighbors shift down, then right. Failure leaves the entire layout unchanged.
pub fn resolve(
    grid: &Grid,
    instances: &[Instance],
    id: &str,
    monitor: &str,
    cell: [u32; 2],
    size: [u32; 2],
    resize: bool,
) -> Option<Vec<Instance>> {
    if grid.clamp(cell, size) != (cell, size) {
        return None;
    }
    let original = instances.iter().find(|i| i.instance == id)?;
    let mut next = instances.to_vec();
    let moved = next.iter_mut().find(|i| i.instance == id)?;
    moved.cell = cell;
    moved.size = size;
    moved.monitor = monitor.into();
    let mut displaced: Vec<_> = next
        .iter()
        .filter(|i| {
            i.instance != id && i.monitor == monitor && overlaps(cell, size, i.cell, i.size)
        })
        .cloned()
        .collect();
    if displaced.is_empty() {
        return Some(next);
    }
    next.retain(|i| !displaced.iter().any(|d| d.instance == i.instance));
    if !resize && original.monitor == monitor && displaced.len() == 1 {
        let neighbor = &displaced[0];
        if free(grid, original.cell, neighbor.size, &next, monitor, None) {
            let mut swapped = neighbor.clone();
            swapped.cell = original.cell;
            next.push(swapped);
            return Some(next);
        }
    }
    displaced.sort_by_key(|i| (i.cell[1], i.cell[0]));
    for mut neighbor in displaced {
        let from = neighbor.cell;
        let spot = (from[0]..grid.cols)
            .flat_map(|col| {
                let first = from[1] + u32::from(col == from[0]);
                (first..grid.rows).map(move |row| [col, row])
            })
            .find(|&at| free(grid, at, neighbor.size, &next, monitor, None))?;
        neighbor.cell = spot;
        next.push(neighbor);
    }
    // Preserve instance order for predictable focus and future serialization.
    Some(
        instances
            .iter()
            .filter_map(|i| next.iter().find(|n| n.instance == i.instance).cloned())
            .collect(),
    )
}
pub fn grid_view(grid: &Grid, size: (u32, u32), scale: f32) -> DisplayList {
    let mut list = DisplayList::default();
    list.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        radius: 0.0,
        color: Rgba::HIT,
    });
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let r = grid.rect([col, row], [1, 1]);
            list.ops.push(Op::Stroke {
                rect: Rect::new(
                    r.x as f32 * scale,
                    r.y as f32 * scale,
                    r.width as f32 * scale,
                    r.height as f32 * scale,
                ),
                radius: 8.0 * scale,
                width: scale,
                color: Rgba::new(0.8, 0.9, 1.0, 0.14),
            });
        }
    }
    list
}
pub fn palette(
    names: &[(String, String)],
    scale: f32,
    error: Option<&str>,
) -> (DisplayList, (u32, u32)) {
    let mut list = DisplayList::default();
    let cols = 6;
    let w = 120.0 * scale;
    let h = 38.0 * scale;
    let size = (
        (cols as f32 * w + 16.0 * scale) as u32,
        ((names.len() + 1).div_ceil(cols) as f32 * h
            + if error.is_some() { 62.0 } else { 16.0 } * scale) as u32,
    );
    list.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        radius: 16.0 * scale,
        color: Rgba::new(0.09, 0.11, 0.14, 0.98),
    });
    for (i, (_, name)) in names
        .iter()
        .chain(std::iter::once(&(String::new(), "Done · Esc".into())))
        .enumerate()
    {
        let r = Rect::new(
            8.0 * scale + (i % cols) as f32 * w,
            8.0 * scale + (i / cols) as f32 * h,
            w - 4.0 * scale,
            h - 4.0 * scale,
        );
        list.ops.push(Op::Fill {
            rect: r,
            radius: 6.0 * scale,
            color: Rgba::new(1.0, 1.0, 1.0, 0.06),
        });
        list.ops.push(Op::Text {
            rect: Rect::new(
                r.left + 8.0 * scale,
                r.top + 8.0 * scale,
                r.width() - 16.0 * scale,
                22.0 * scale,
            ),
            text: name.as_str().into(),
            style: TextStyle::new("Segoe UI", 11.0 * scale),
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        });
        list.hits.push(Hit {
            id: i as u32,
            rect: r,
            role: Role::Button,
            label: name.as_str().into(),
            enabled: true,
            checked: None,
        });
    }
    if let Some(error) = error {
        list.ops.push(Op::Text {
            rect: Rect::new(
                12.0 * scale,
                size.1 as f32 - 42.0 * scale,
                size.0 as f32 - 24.0 * scale,
                40.0 * scale,
            ),
            text: error.into(),
            style: TextStyle::new("Segoe UI", 10.0 * scale),
            color: Rgba::rgb8([255, 170, 150], 1.0),
        });
    }
    (list, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_widgets::layout::{GridSpec, Layout, RectF};
    #[test]
    fn arranging_swaps_shifts_and_refuses_atomically() {
        let grid = Grid {
            origin: [0.0; 2],
            cell: [96.0; 2],
            gap: 8.0,
            cols: 6,
            rows: 4,
        };
        let mut layout = Layout::default();
        let a = layout.add("notes", "m", [0, 0], [2, 2]);
        let b = layout.add("notes", "m", [2, 0], [2, 2]);
        let swapped = resolve(&grid, &layout.instances, &a, "m", [2, 0], [2, 2], false).unwrap();
        assert_eq!(
            swapped.iter().find(|i| i.instance == b).unwrap().cell,
            [0, 0]
        );
        let shifted = resolve(&grid, &layout.instances, &a, "m", [1, 0], [3, 2], true).unwrap();
        assert_eq!(
            shifted.iter().find(|i| i.instance == b).unwrap().cell,
            [2, 2]
        );
        let full = Grid {
            rows: 2,
            cols: 4,
            ..grid
        };
        assert!(resolve(&full, &layout.instances, &a, "m", [1, 0], [3, 2], true).is_none());
        assert_eq!(
            layout.get(&b).unwrap().cell,
            [2, 0],
            "refusal never mutates input"
        );
    }
    #[test]
    fn drag_resize_and_add_stay_on_grid_without_overlaps() {
        let grid = Grid::new(
            &GridSpec::default(),
            [96.0; 2],
            RectF {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            },
        );
        let mut layout = Layout::default();
        layout.add("notes", "D1", [0, 0], [2, 2]);
        assert!(!free(&grid, [1, 1], [2, 2], &layout.instances, "D1", None));
        assert!(free(
            &grid,
            [0, 0],
            [3, 3],
            &layout.instances,
            "D1",
            Some("notes-1")
        ));
        assert_eq!(
            vacancy(&grid, [2, 2], &layout.instances, "D1"),
            Some([2, 0])
        );
        assert!(!free(
            &grid,
            [grid.cols - 1, 0],
            [2, 2],
            &layout.instances,
            "D1",
            None
        ));
        let r = grid.rect([3, 2], [2, 3]);
        assert_eq!(
            snapped(
                &grid,
                Rect::new(
                    r.x as f32 * 1.5,
                    r.y as f32 * 1.5,
                    r.width as f32 * 1.5,
                    r.height as f32 * 1.5
                ),
                1.5
            ),
            ([3, 2], [2, 3])
        );
    }
}
