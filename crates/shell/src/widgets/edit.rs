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
pub fn palette(names: &[(String, String)], scale: f32) -> (DisplayList, (u32, u32)) {
    let mut list = DisplayList::default();
    let cols = 6;
    let w = 120.0 * scale;
    let h = 38.0 * scale;
    let size = (
        (cols as f32 * w + 16.0 * scale) as u32,
        ((names.len() + 1).div_ceil(cols) as f32 * h + 16.0 * scale) as u32,
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
    (list, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_widgets::layout::{GridSpec, Layout, RectF};
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
