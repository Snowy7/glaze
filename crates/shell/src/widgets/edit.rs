//! Grid placement and edit chrome. Layout is committed only at the end of a gesture; a rejected
//! drop leaves the saved layout intact. Motion itself stays in physical pixels, at display rate.
use mypaper_bars::config::{BarsConfig, DockPosition, TopPosition};
use mypaper_ui::icons::{self, Icon};
use mypaper_ui::interact::State;
use mypaper_ui::{
    DisplayList, Hit, HitId, Ink, Interaction, Op, Rect, Rgba, Role, Step, Text, TextStyle,
    Typeface,
};
use mypaper_widgets::layout::{Grid, Instance};
use mypaper_widgets::{layout::RectF, manifest::WidgetManifest};

/// Intersect the OS work area with bands derived from config, even if appbar registration
/// failed or an autohiding dock has released its reservation. All coordinates are DIPs.
pub fn usable_area(
    area: RectF,
    work: RectF,
    bars: &BarsConfig,
    device: &str,
    primary: bool,
) -> RectF {
    let mut inset = [0.0_f64; 4]; // left, top, right, bottom
    if bars.top.enabled && bars.top.monitors.includes(device, primary) {
        inset[if bars.top.position == TopPosition::Top {
            1
        } else {
            3
        }] += bars.top.band();
    }
    if bars.dock.enabled && bars.dock.monitors.includes(device, primary) {
        inset[match bars.dock.position {
            DockPosition::Left => 0,
            DockPosition::Right => 2,
            DockPosition::Top => 1,
            DockPosition::Bottom => 3,
        }] += bars.dock.band();
    }
    let left = (area.x + inset[0]).max(work.x).min(area.x + area.width);
    let top = (area.y + inset[1]).max(work.y).min(area.y + area.height);
    let right = (area.x + area.width - inset[2])
        .min(work.x + work.width)
        .max(left);
    let bottom = (area.y + area.height - inset[3])
        .min(work.y + work.height)
        .max(top);
    RectF {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// Snaps the pointer's requested rectangle to a supported size. Top/left resizing keeps
/// the opposite edge fixed; a side handle never changes the other dimension.
pub fn resize_target(
    grid: &Grid,
    manifest: &WidgetManifest,
    cell: [u32; 2],
    size: [u32; 2],
    requested: RectF,
    edges: [bool; 4],
) -> Option<([u32; 2], [u32; 2])> {
    let moving = [edges[0] || edges[2], edges[1] || edges[3]];
    let desired = [
        (requested.width + grid.gap) / (grid.cell[0] + grid.gap),
        (requested.height + grid.gap) / (grid.cell[1] + grid.gap),
    ];
    let limits = [0, 1].map(|i| {
        if edges[i] {
            cell[i] + size[i]
        } else {
            [grid.cols, grid.rows][i] - cell[i]
        }
    });
    let fits = |s: &[u32; 2]| (0..2).all(|i| s[i] <= limits[i] && (moving[i] || s[i] == size[i]));
    let next = if !manifest.resizable {
        Some(size)
    } else if manifest.size.allowed.is_empty() {
        manifest.fit_size(desired, limits)
    } else {
        manifest
            .size
            .allowed
            .iter()
            .filter(|s| fits(s))
            .copied()
            .min_by(|a, b| {
                let distance = |s: [u32; 2]| {
                    (0..2)
                        .map(|i| (f64::from(s[i]) - desired[i]).powi(2))
                        .sum::<f64>()
                };
                distance(*a)
                    .total_cmp(&distance(*b))
                    .then_with(|| (b[0] + b[1]).cmp(&(a[0] + a[1])))
            })
    }?;
    let at = [0, 1].map(|i| {
        if edges[i] {
            cell[i] + size[i] - next[i]
        } else {
            cell[i]
        }
    });
    Some((at, next))
}

/// Shift-arrow moves to the next supported extent, including gaps between declared sizes.
pub fn nudge_size(
    manifest: &WidgetManifest,
    size: [u32; 2],
    axis: usize,
    delta: i32,
    limits: [u32; 2],
) -> Option<[u32; 2]> {
    if !manifest.resizable {
        return None;
    }
    if manifest.size.allowed.is_empty() {
        let mut requested = size;
        requested[axis] = size[axis].saturating_add_signed(delta);
        return manifest.fit_size(requested.map(f64::from), limits);
    }
    manifest
        .size
        .allowed
        .iter()
        .copied()
        .filter(|s| {
            s[1 - axis] == size[1 - axis]
                && (0..2).all(|i| s[i] <= limits[i])
                && if delta > 0 {
                    s[axis] > size[axis]
                } else {
                    s[axis] < size[axis]
                }
        })
        .min_by_key(|s| s[axis].abs_diff(size[axis]))
}

/// Clamps a physical pointer-following rectangle without changing its dimensions.
pub fn clamp_rect(rect: Rect, bounds: Rect) -> Rect {
    Rect::new(
        rect.left
            .clamp(bounds.left, (bounds.right - rect.width()).max(bounds.left)),
        rect.top
            .clamp(bounds.top, (bounds.bottom - rect.height()).max(bounds.top)),
        rect.width().min(bounds.width()),
        rect.height().min(bounds.height()),
    )
}

/// A cropped, click-through target window; it never redraws the monitor-sized grid.
pub fn ghost_view(size: (u32, u32), scale: f32, valid: bool) -> DisplayList {
    let rect = Rect::new(
        1.0,
        1.0,
        size.0.saturating_sub(2) as f32,
        size.1.saturating_sub(2) as f32,
    );
    let color = if valid {
        Rgba::rgb8([150, 205, 255], 1.0)
    } else {
        Rgba::rgb8([255, 135, 120], 1.0)
    };
    DisplayList {
        ops: vec![
            Op::Fill {
                rect,
                radius: 12.0 * scale,
                color: color.with_alpha(0.12),
            },
            Op::Stroke {
                rect,
                radius: 12.0 * scale,
                width: 2.0 * scale,
                color: color.with_alpha(0.8),
            },
        ],
        ..Default::default()
    }
}

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

/// Reflow only instances affected by a bounds/size change. Valid neighbors retain their cells.
pub fn constrain(
    grid: &Grid,
    instances: &[Instance],
    monitor: &str,
    registry: &mypaper_widgets::registry::Registry,
) -> Vec<Instance> {
    let mut next = instances.to_vec();
    let mut changed = Vec::new();
    for (index, instance) in next
        .iter_mut()
        .enumerate()
        .filter(|(_, i)| i.monitor == monitor)
    {
        let Some(manifest) = registry.get(&instance.widget).map(|i| &i.manifest) else {
            continue;
        };
        let Some(size) = manifest.fit_size(instance.size.map(f64::from), [grid.cols, grid.rows])
        else {
            continue;
        };
        let (cell, size) = grid.clamp(instance.cell, size);
        if instance.cell != cell || instance.size != size {
            instance.cell = cell;
            instance.size = size;
            changed.push(index);
        }
    }
    for index in changed {
        let instance = &next[index];
        if !free(
            grid,
            instance.cell,
            instance.size,
            &next,
            monitor,
            Some(&instance.instance),
        ) {
            let others: Vec<_> = next
                .iter()
                .filter(|i| i.instance != instance.instance)
                .cloned()
                .collect();
            if let Some(cell) = vacancy(grid, instance.size, &others, monitor) {
                next[index].cell = cell;
            }
        }
    }
    next
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
/// The "Add widgets" palette: a header with Done, then a grid of tiles (icon and name) with
/// the shared hover and press washes from `hot`. Hit ids are the tiles' indices into `names`;
/// Done is `names.len()`.
pub fn palette(
    names: &[(String, String)],
    scale: f32,
    error: Option<&str>,
    hot: &Interaction,
    text: &mut Text,
) -> (DisplayList, (u32, u32)) {
    let s = scale;
    let face = Typeface::system(text);
    let family = icons::system_family(text);
    let ink = Ink::of(Rgba::rgb8([245, 245, 247], 1.0));
    let mut list = DisplayList::default();
    let cols = 6;
    let (tile_w, tile_h, gap, pad, header) = (150.0 * s, 44.0 * s, 6.0 * s, 12.0 * s, 44.0 * s);
    let rows = names.len().div_ceil(cols);
    let error_h = if error.is_some() { 40.0 * s } else { 0.0 };
    let size = (
        (pad * 2.0 + cols as f32 * tile_w + (cols - 1) as f32 * gap).ceil() as u32,
        (header + rows as f32 * (tile_h + gap) - gap + pad + error_h).ceil() as u32,
    );
    let panel = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    list.ops.push(Op::Fill {
        rect: panel,
        radius: 18.0 * s,
        color: Rgba::rgb8([22, 24, 29], 0.96),
    });
    list.ops.push(Op::Stroke {
        rect: panel.inflate(-0.5),
        radius: 18.0 * s,
        width: 1.0,
        color: Rgba::new(1.0, 1.0, 1.0, 0.12),
    });
    let label = |list: &mut DisplayList,
                 text: &mut Text,
                 rect: Rect,
                 value: &str,
                 style: TextStyle,
                 color: Rgba| {
        let value = text.fit(value, &style, rect.width().max(1.0));
        let (w, h) = text.measure(&value, &style);
        list.ops.push(Op::Text {
            rect: Rect::new(
                rect.left.round(),
                (rect.top + (rect.height() - h) / 2.0).round(),
                w,
                h,
            ),
            text: value,
            style,
            color,
        });
    };
    label(
        &mut list,
        text,
        Rect::new(pad + 4.0 * s, 0.0, 300.0 * s, header),
        "Add widgets",
        face.style(Step::Headline, s),
        ink.primary,
    );
    // Done: a light chip with dark text, the one filled control.
    let done_id = names.len() as HitId;
    let done = Rect::new(panel.right - pad - 76.0 * s, 9.0 * s, 76.0 * s, 26.0 * s);
    let done_fill = match hot.state(done_id, true) {
        State::Hover => Rgba::rgb8([255; 3], 1.0),
        State::Pressed => Rgba::rgb8([200, 202, 208], 1.0),
        _ => Rgba::rgb8([236, 237, 240], 1.0),
    };
    list.ops.push(Op::Fill {
        rect: done,
        radius: 13.0 * s,
        color: done_fill,
    });
    let style = face.style(Step::Label, s);
    let (w, _) = text.measure("Done", &style);
    label(
        &mut list,
        text,
        Rect::new(
            done.left + (done.width() - w) / 2.0,
            done.top,
            w + 1.0,
            done.height(),
        ),
        "Done",
        style,
        Rgba::rgb8([16, 16, 20], 1.0),
    );
    list.hits.push(Hit {
        id: done_id,
        rect: done,
        role: Role::Button,
        label: "Done (Esc)".into(),
        enabled: true,
        checked: None,
    });
    for (i, (id, name)) in names.iter().enumerate() {
        let r = Rect::new(
            pad + (i % cols) as f32 * (tile_w + gap),
            header + (i / cols) as f32 * (tile_h + gap),
            tile_w,
            tile_h,
        );
        let state = hot.state(i as HitId, true);
        list.ops.push(Op::Fill {
            rect: r,
            radius: 10.0 * s,
            color: match state {
                State::Hover => Rgba::new(1.0, 1.0, 1.0, 0.14),
                State::Pressed => Rgba::new(1.0, 1.0, 1.0, 0.22),
                _ => Rgba::new(1.0, 1.0, 1.0, 0.06),
            },
        });
        let icon = Rect::new(r.left + 8.0 * s, r.top, 26.0 * s, r.height());
        list.ops.push(icons::centered(
            text,
            &family,
            widget_icon(id),
            (16.0 * s).round(),
            icon,
            ink.secondary,
        ));
        label(
            &mut list,
            text,
            Rect::new(r.left + 38.0 * s, r.top, r.width() - 44.0 * s, r.height()),
            name,
            face.style(Step::Body, s),
            ink.primary,
        );
        list.hits.push(Hit {
            id: i as HitId,
            rect: r,
            role: Role::Button,
            label: format!("Add {name}").into(),
            enabled: true,
            checked: None,
        });
    }
    if let Some(error) = error {
        label(
            &mut list,
            text,
            Rect::new(
                pad + 4.0 * s,
                size.1 as f32 - error_h,
                panel.width() - 2.0 * pad,
                error_h - 8.0 * s,
            ),
            error,
            face.style(Step::Caption, s),
            Rgba::rgb8([255, 170, 150], 1.0),
        );
    }
    (list, size)
}

/// The palette's icon for a built-in widget id.
fn widget_icon(id: &str) -> Icon {
    match id {
        "calendar" => Icon::Calendar,
        "clock" => Icon::Clock,
        "countdown" | "timer" => Icon::Timer,
        "crypto" => Icon::Gauge,
        "heatmap" | "todo" => Icon::Tasks,
        "launcher" => Icon::Apps,
        "media-pill" | "now-playing" => Icon::Music,
        "notes" | "quote" => Icon::Notes,
        "photo" => Icon::Photo,
        "quick-toggles" => Icon::Settings,
        "rss" => Icon::Feed,
        "system-monitor" => Icon::Cpu,
        "weather" => Icon::Brightness,
        _ => Icon::Apps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_widgets::layout::{GridSpec, Layout, RectF};
    #[test]
    fn configured_bands_apply_without_appbar_and_do_not_double_reserve() {
        let area = RectF {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
        };
        let config=BarsConfig::parse(r#"{"top":{"height":32,"layout":"floating","margin":8},"dock":{"enabled":true,"autohide":true,"monitors":"all","position":"bottom","size":48,"padding":5,"margin":6}}"#).unwrap();
        let usable = usable_area(area, area, &config, "DISPLAY1", true);
        assert_eq!(
            usable,
            RectF {
                x: 0.0,
                y: 48.0,
                width: 2560.0,
                height: 1322.0
            }
        );
        assert_eq!(usable_area(area, usable, &config, "DISPLAY1", true), usable);
        let overridden=BarsConfig::parse(r#"{"top":{"enabled":false},"dock":{"enabled":false},"overrides":{"secondary":{"top":{"enabled":true,"height":40,"position":"bottom","margin":0},"dock":{"enabled":true,"position":"left","monitors":"all","size":40,"padding":4,"margin":6}}}}"#).unwrap().for_monitor("DISPLAY2",false).unwrap();
        assert_eq!(
            usable_area(area, area, &overridden, "DISPLAY2", false),
            RectF {
                x: 60.0,
                y: 0.0,
                width: 2500.0,
                height: 1400.0
            }
        );
    }
    #[test]
    fn weather_resizes_live_to_supported_tall_sizes_and_keeps_opposite_edges() {
        let registry = mypaper_widgets::registry::builtins();
        let manifest = &registry.get("weather").unwrap().manifest;
        let grid = Grid {
            origin: [16.0, 64.0],
            cell: [96.0; 2],
            gap: 8.0,
            cols: 12,
            rows: 10,
        };
        for size in [[4, 4], [4, 5], [5, 5], [5, 6]] {
            let requested = grid.rect([2, 1], size);
            assert_eq!(
                resize_target(
                    &grid,
                    manifest,
                    [2, 1],
                    [3, 2],
                    requested,
                    [false, false, true, true]
                ),
                Some(([2, 1], size))
            );
        }
        let requested = grid.rect([0, 0], [5, 6]);
        assert_eq!(
            resize_target(
                &grid,
                manifest,
                [2, 3],
                [3, 3],
                requested,
                [true, true, false, false]
            ),
            Some(([0, 0], [5, 6]))
        );
        let huge = RectF {
            width: 5000.0,
            height: 5000.0,
            ..requested
        };
        assert_eq!(
            resize_target(
                &grid,
                manifest,
                [9, 8],
                [3, 2],
                huge,
                [false, false, true, true]
            ),
            Some(([9, 8], [3, 2]))
        );
        // Every corner/side preserves the edge it did not grab.
        for edges in [
            [true, false, false, false],
            [false, true, false, false],
            [false, false, true, false],
            [false, false, false, true],
        ] {
            let (at, size) =
                resize_target(&grid, manifest, [2, 3], [3, 3], requested, edges).unwrap();
            for i in 0..2 {
                if edges[i] {
                    assert_eq!(at[i] + size[i], 5 + i as u32)
                } else {
                    assert_eq!(at[i], [2, 3][i])
                }
            }
        }
    }
    #[test]
    fn sparse_sizes_keyboard_steps_and_clamped_layout_keep_valid_neighbors() {
        let registry = mypaper_widgets::registry::builtins();
        let timer = &registry.get("timer").unwrap().manifest;
        assert_eq!(nudge_size(timer, [4, 4], 1, -1, [6, 6]), Some([4, 2]));
        assert_eq!(nudge_size(timer, [4, 2], 1, 1, [6, 6]), Some([4, 4]));
        let grid = Grid {
            origin: [0.0; 2],
            cell: [96.0; 2],
            gap: 8.0,
            cols: 8,
            rows: 6,
        };
        let mut layout = Layout::default();
        let stable = layout.add("notes", "m", [4, 2], [4, 4]);
        let clamped = layout.add("weather", "m", [7, 5], [5, 6]);
        let next = constrain(&grid, &layout.instances, "m", &registry);
        assert_eq!(
            next.iter().find(|i| i.instance == stable),
            layout.get(&stable)
        );
        let weather = next.iter().find(|i| i.instance == clamped).unwrap();
        assert_eq!(
            grid.clamp(weather.cell, weather.size),
            (weather.cell, weather.size)
        );
        // There is no room for a 5x6 beside a 4x4: bounds still win; ordinary collision
        // resolution refuses a gesture atomically rather than cascading unrelated widgets.
        assert!(
            resolve(
                &grid,
                &layout.instances,
                &clamped,
                "m",
                [3, 0],
                [5, 6],
                true
            )
            .is_none()
        );
    }
    #[test]
    fn tiny_usable_areas_never_expand_back_to_the_nominal_icon_cell() {
        let work = RectF {
            x: 24.0,
            y: 80.0,
            width: 22.0,
            height: 30.0,
        };
        let grid = Grid::new(&GridSpec::default(), [96.0; 2], work);
        let rect = grid.rect([0, 0], [1, 1]);
        assert!(rect.x >= work.x && rect.y >= work.y);
        assert!(rect.x + rect.width <= work.x + work.width);
        assert!(rect.y + rect.height <= work.y + work.height);
        let bounds = Rect::new(-2560.0, 50.0, 2560.0, 1310.0);
        let live = clamp_rect(Rect::new(-2700.0, -100.0, 400.0, 500.0), bounds);
        assert_eq!(live, Rect::new(-2560.0, 50.0, 400.0, 500.0));
    }
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
