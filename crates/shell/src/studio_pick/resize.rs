//! Grid-snapped corner resizing. A gesture changes only its outline until release.
use mypaper_ui::{Point, Rect};
use mypaper_widgets::{layout::Grid, manifest::SizeSpec};

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub grid: Grid,
    pub cell: [u32; 2],
    pub size: [u32; 2],
    pub saved_cell: [u32; 2],
    pub saved_size: [u32; 2],
    pub limits: SizeSpec,
    pub origin: (i32, i32),
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Corner {
    pub left: bool,
    pub top: bool,
}

pub(super) const CORNERS: [Corner; 4] = [
    Corner {
        left: true,
        top: true,
    },
    Corner {
        left: false,
        top: true,
    },
    Corner {
        left: true,
        top: false,
    },
    Corner {
        left: false,
        top: false,
    },
];

pub(super) fn handle(rect: Rect, corner: Corner, scale: f32) -> Rect {
    let side = 20.0 * scale;
    Rect::new(
        if corner.left { rect.left } else { rect.right } - side / 2.0,
        if corner.top { rect.top } else { rect.bottom } - side / 2.0,
        side,
        side,
    )
}

impl Spec {
    pub(super) fn target(&self, corner: Corner, delta: Point) -> ([u32; 2], [u32; 2]) {
        let leading = [corner.left, corner.top];
        let delta = [delta.x, delta.y];
        let grid_limits = [self.grid.cols, self.grid.rows];
        let max = self.limits.max.unwrap_or(grid_limits);
        let size = [0, 1].map(|axis| {
            let limit = if leading[axis] {
                self.cell[axis] + self.size[axis]
            } else {
                grid_limits[axis].saturating_sub(self.cell[axis])
            };
            let step = (self.grid.cell[axis] + self.grid.gap) as f32 * self.scale;
            let change = delta[axis] / step * if leading[axis] { -1.0 } else { 1.0 };
            let high = max[axis].min(limit).max(1);
            (self.size[axis] as f32 + change)
                .round()
                .clamp(self.limits.min[axis].min(high) as f32, high as f32) as u32
        });
        let cell = [0, 1].map(|axis| {
            if leading[axis] {
                self.cell[axis] + self.size[axis] - size[axis]
            } else {
                self.cell[axis]
            }
        });
        (cell, size)
    }

    pub(super) fn rect(&self, cell: [u32; 2], size: [u32; 2]) -> Rect {
        let r = self.grid.rect(cell, size);
        Rect::new(
            self.origin.0 as f32 + r.x as f32 * self.scale,
            self.origin.1 as f32 + r.y as f32 * self.scale,
            r.width as f32 * self.scale,
            r.height as f32 * self.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> Spec {
        Spec {
            grid: Grid {
                origin: [16.0, 40.0],
                cell: [40.0, 30.0],
                gap: 8.0,
                cols: 20,
                rows: 20,
            },
            cell: [3, 4],
            size: [4, 5],
            saved_cell: [3, 4],
            saved_size: [4, 5],
            limits: SizeSpec {
                default: [4, 5],
                min: [2, 2],
                max: Some([10, 10]),
            },
            origin: (-2560, 9),
            scale: 2.0,
        }
    }
    #[test]
    fn every_corner_snaps_and_keeps_opposite_edges_fixed() {
        let s = spec();
        for c in CORNERS {
            let (cell, size) = s.target(
                c,
                Point::new(
                    if c.left { -96.0 } else { 96.0 },
                    if c.top { -76.0 } else { 76.0 },
                ),
            );
            assert_eq!(size, [5, 6]);
            assert_eq!(
                cell,
                [if c.left { 2 } else { 3 }, if c.top { 3 } else { 4 }]
            );
            let before = s.rect(s.cell, s.size);
            let after = s.rect(cell, size);
            assert_eq!(
                if c.left { before.right } else { before.left },
                if c.left { after.right } else { after.left }
            );
            assert_eq!(
                if c.top { before.bottom } else { before.top },
                if c.top { after.bottom } else { after.top }
            );
        }
    }
    #[test]
    fn resizing_clamps_to_manifest_and_grid_without_underflow() {
        let s = spec();
        assert_eq!(
            s.target(CORNERS[0], Point::new(-10000.0, -10000.0)),
            ([0, 0], [7, 9])
        );
        assert_eq!(
            s.target(CORNERS[3], Point::new(10000.0, 10000.0)),
            ([3, 4], [10, 10])
        );
        assert_eq!(
            s.target(CORNERS[3], Point::new(-10000.0, -10000.0)),
            ([3, 4], [2, 2])
        );
        assert_eq!(
            s.target(CORNERS[3], Point::new(40.0, 30.0)),
            (s.cell, s.size)
        );
    }
    #[test]
    fn handle_hit_boxes_cover_only_corners() {
        let r = Rect::new(-500.0, 100.0, 300.0, 240.0);
        let handles = CORNERS.map(|c| handle(r, c, 1.0));
        assert!(handles[0].contains(Point::new(r.left, r.top)));
        assert!(
            !handles
                .iter()
                .any(|h| h.contains(Point::new(-350.0, 100.0)))
        );
        assert!(
            !handles
                .iter()
                .any(|h| h.contains(Point::new(-350.0, 220.0)))
        );
    }
}
