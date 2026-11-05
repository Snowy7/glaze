//! Grid placement and edit chrome. Layout is committed only at the end of a gesture; a rejected
//! drop leaves the saved layout intact. Motion itself stays in physical pixels, at display rate.
use mypaper_bars::config::{BarsConfig, DockPosition, TopPosition};
use mypaper_ui::icons::Icon;
use mypaper_ui::{
    Accessory, Align, ButtonKind, DisplayList, Frame, HitId, Interaction, Lead, ListRow, Op, Rect,
    Rgba, Scheme, Scroll, Text, TextField, TextRole, Theme,
};
use mypaper_widgets::layout::{Grid, Instance};
use mypaper_widgets::{layout::RectF, manifest::WidgetManifest};

/// Intersect the OS work area with bands derived from config, even if appbar registration
/// failed. An autohiding dock reserves nothing: widgets reach the bottom edge and the dock
/// slides over them when it shows (as on macOS). All coordinates are DIPs.
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
    if bars.dock.enabled && !bars.dock.autohide && bars.dock.monitors.includes(device, primary) {
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
            [grid.cols, grid.rows][i].saturating_sub(cell[i])
        }
    });
    // A side handle keeps the other dimension; every size from min to max is allowed.
    let desired = [0, 1].map(|i| if moving[i] { desired[i] } else { f64::from(size[i]) });
    let next = if manifest.resizable {
        manifest.fit_size(desired, limits)?
    } else {
        size
    };
    let at = [0, 1].map(|i| {
        if edges[i] {
            cell[i] + size[i] - next[i]
        } else {
            cell[i]
        }
    });
    Some((at, next))
}

/// Shift-arrow grows or shrinks one cell along `axis`; None at the widget's min or max.
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
    let mut requested = size;
    requested[axis] = size[axis].saturating_add_signed(delta);
    manifest
        .fit_size(requested.map(f64::from), limits)
        .filter(|&next| next != size)
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

/// Where the edit pill sits: top centre, or bottom centre when widgets block the top and the
/// bottom is clear.
pub fn pill_origin(bounds: Rect, size: (u32, u32), scale: f32, widgets: &[Rect]) -> (i32, i32) {
    let x = bounds.left + (bounds.width() - size.0 as f32) / 2.0;
    let top = bounds.top + 10.0 * scale;
    let bottom = (bounds.bottom - size.1 as f32 - 10.0 * scale).max(bounds.top);
    let blocked = |y| {
        let bar = Rect::new(x, y, size.0 as f32, size.1 as f32);
        widgets.iter().any(|r| r.intersects(&bar))
    };
    let y = if blocked(top) && !blocked(bottom) {
        bottom
    } else {
        top
    };
    (x.round() as i32, y.ceil() as i32)
}

pub fn overlaps(a: [u32; 2], size: [u32; 2], b: [u32; 2], other: [u32; 2]) -> bool {
    (0..2).all(|i| {
        u64::from(a[i]) < u64::from(b[i]) + u64::from(other[i])
            && u64::from(b[i]) < u64::from(a[i]) + u64::from(size[i])
    })
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
/// Hit ids of the edit pill.
pub const PILL_ADD: HitId = 1;
pub const PILL_DONE: HitId = 2;
/// Hit ids of the picker: its Add button, and row `i` of the filtered list at `PICK_ROW + i`.
pub const PICK_ADD: HitId = 3;
pub const PICK_ROW: HitId = 100;

/// The edit chrome's kit theme: the user's style, dark, and monochrome like the widgets (the
/// primary action takes the label's ink).
pub fn chrome_theme() -> Theme {
    let mut theme = Theme::new(
        mypaper_ui::theme::active(),
        Scheme::Dark,
        mypaper_ui::theme::SYSTEM_BLUE,
    );
    let p = &mut theme.color;
    (p.accent, p.on_accent) = (Rgba::rgb8([242; 3], 1.0), Rgba::rgb8([17, 17, 19], 1.0));
    p.accent_wash = p.accent.with_alpha(0.16);
    theme
}

/// The edit pill: "+" (add a widget) and Done, a small capsule that floats over the desktop.
pub fn pill(theme: &Theme, s: f32, hot: &Interaction, text: &mut Text) -> (DisplayList, (u32, u32)) {
    let mut f = Frame::new(text, theme, s, hot);
    let (pad, gap, add) = (f.px(6.0), f.px(4.0), f.px(36.0));
    let (done_w, _) = f.button_size("Done", false);
    let size = (
        (pad * 2.0 + add + gap + done_w).ceil(),
        (pad * 2.0 + add).ceil(),
    );
    let body = Rect::new(0.0, 0.0, size.0, size.1);
    f.surface_shaped(body, size.1 / 2.0, false);
    f.icon_button(
        PILL_ADD,
        Rect::new(pad, pad, add, add),
        Icon::Add,
        36.0,
        "Add widget",
        true,
    );
    f.button(
        PILL_DONE,
        Rect::new(pad + add + gap, pad, done_w, add),
        "Done",
        None,
        ButtonKind::Filled,
        true,
    );
    (f.list, (size.0 as u32, size.1 as u32))
}

/// A widget the picker offers.
pub struct Choice {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    /// Its default size in cells.
    pub size: [u32; 2],
}

/// The picker's groups, in order; anything else lands after them, then "Other".
const CATEGORIES: [&str; 6] = ["Time", "Info", "Media", "Productivity", "System", "Integrations"];

/// Orders choices by category, then name.
pub fn sort_choices(choices: &mut [Choice]) {
    let rank = |c: &Choice| match CATEGORIES.iter().position(|&k| k == c.category) {
        Some(i) => (i, String::new()),
        None if c.category == "Other" => (CATEGORIES.len() + 1, String::new()),
        None => (CATEGORIES.len(), c.category.clone()),
    };
    choices.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
}

/// The choices a search shows: a case-insensitive match on name, category or id.
pub fn filter<'a>(choices: &'a [Choice], query: &str) -> Vec<&'a Choice> {
    let query = query.trim().to_lowercase();
    choices
        .iter()
        .filter(|c| {
            query.is_empty()
                || [&c.name, &c.category, &c.id]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&query))
        })
        .collect()
}

/// The picker popover's size in device px at `s`.
pub fn picker_size(s: f32) -> (u32, u32) {
    ((600.0 * s).round() as u32, (420.0 * s).round() as u32)
}

/// What the picker draws besides its choices.
pub struct PickerState<'a> {
    pub query: &'a str,
    pub field: Option<&'a TextField>,
    /// Index into the filtered choices (the preview's widget).
    pub selected: usize,
    pub scroll: &'a Scroll,
    pub now: std::time::Instant,
    /// A refusal to show under the preview ("No room on this display").
    pub notice: Option<&'a str>,
    /// The selected widget drawn at its default size, `(list, width, height)` in device px.
    pub preview: Option<(DisplayList, f32, f32)>,
}

/// The widget picker: a search field over a scrolling list grouped by category on the left,
/// and the selected widget's preview, name, size and Add on the right.
pub fn picker(
    theme: &Theme,
    s: f32,
    hot: &Interaction,
    text: &mut Text,
    shown: &[&Choice],
    state: PickerState,
) -> DisplayList {
    let mut f = Frame::new(text, theme, s, hot);
    let (w, h) = picker_size(s);
    let body = Rect::new(0.0, 0.0, w as f32, h as f32);
    f.surface(body, false);
    let inner = body.inset(mypaper_ui::Insets::all(f.px(theme.metrics.pad)));
    let (list, pane) = inner.take_left(f.px(250.0), f.px(theme.metrics.gap));
    let (search, rows) = list.take_top(f.px(theme.metrics.control), f.px(8.0));
    let field_rect = f.field_frame(
        search,
        "Search widgets",
        state.query.is_empty(),
        state.field.is_some(),
        Some(Icon::Search),
    );
    if let Some(field) = state.field {
        field.set_rect(field_rect);
        f.push(Op::TextField {
            rect: field_rect,
            field: field.clone(),
            revision: field.revision(),
        });
    }
    let row_h = f.px(theme.metrics.row_single);
    let header_h = (f.px(theme.metrics.row_single) * 2.0 / 3.0).round();
    if shown.is_empty() {
        f.empty(rows, Some(Icon::Search), "No widgets match", None);
    } else {
        f.scroll(rows, state.scroll, state.now, |f, origin| {
            let mut y = origin.y;
            let mut category: Option<&str> = None;
            for (i, choice) in shown.iter().enumerate() {
                if category != Some(choice.category.as_str()) {
                    category = Some(&choice.category);
                    f.section_header(
                        Rect::new(origin.x, y, rows.width(), header_h),
                        &choice.category,
                        None,
                    );
                    y += header_h;
                }
                f.row(
                    &ListRow {
                        id: PICK_ROW + i as HitId,
                        lead: Lead::Icon(widget_icon(&choice.id)),
                        title: &choice.name,
                        subtitle: None,
                        accessory: Accessory::None,
                        selected: i == state.selected,
                        enabled: true,
                    },
                    Rect::new(origin.x, y, rows.width(), row_h),
                );
                y += row_h;
            }
            y - origin.y
        });
    }
    // The preview pane.
    let card = f.card(pane);
    if let Some(choice) = shown.get(state.selected) {
        let c = theme.color;
        let (title, rest) = card.take_bottom(f.px(theme.metrics.control), f.px(10.0));
        let (info, stage) = rest.take_bottom(f.px(64.0), f.px(10.0));
        if let Some((preview, pw, ph)) = state.preview {
            let at = stage.place(pw, ph, Align::Center, Align::Center);
            f.push(Op::Clipped {
                rect: stage,
                ops: preview.translated(at.left, at.top).ops.into(),
            });
        }
        f.label(
            info.left,
            info.top,
            &choice.name,
            TextRole::Title3,
            c.label,
            info.width() - f.px(56.0),
        );
        let size = format!("{} × {}", choice.size[0], choice.size[1]);
        f.text_in(
            Rect::new(info.left, info.top, info.width(), f.line(TextRole::Title3)),
            &size,
            TextRole::Footnote,
            c.tertiary,
            Align::End,
        );
        let detail = state.notice.unwrap_or(&choice.description);
        f.paragraph(
            info.left,
            info.top + f.line(TextRole::Title3) + f.px(2.0),
            info.width(),
            detail,
            TextRole::Callout,
            if state.notice.is_some() { c.warning } else { c.secondary },
            2,
        );
        f.button(PICK_ADD, title, "Add widget", Some(Icon::Add), ButtonKind::Filled, true);
    }
    f.list
}

/// The picker's icon for a widget id (custom widgets get the generic one).
pub fn widget_icon(id: &str) -> Icon {
    match id {
        "calendar" => Icon::Calendar,
        "clock" => Icon::Clock,
        "countdown" | "timer" => Icon::Timer,
        "crypto" => Icon::Gauge,
        "heatmap" => Icon::Layers,
        "todo" => Icon::Tasks,
        "launcher" => Icon::Apps,
        "media-pill" | "now-playing" => Icon::Music,
        "notes" => Icon::Notes,
        "quote" => Icon::Message,
        "photo" => Icon::Photo,
        "quick-toggles" => Icon::Controls,
        "rss" => Icon::Feed,
        "system-monitor" => Icon::Cpu,
        "weather" => Icon::CloudSun,
        "t3" => Icon::Terminal,
        "discord" => Icon::Message,
        "grok" => Icon::Sparkles,
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
        // An autohiding dock reserves nothing: widgets reach the bottom edge.
        let usable = usable_area(area, area, &config, "DISPLAY1", true);
        assert_eq!(
            usable,
            RectF {
                x: 0.0,
                y: 48.0,
                width: 2560.0,
                height: 1392.0
            }
        );
        assert_eq!(usable_area(area, usable, &config, "DISPLAY1", true), usable);
        let fixed = BarsConfig::parse(r#"{"top":{"height":32,"layout":"floating","margin":8},"dock":{"enabled":true,"autohide":false,"monitors":"all","position":"bottom","size":48,"padding":5,"margin":6}}"#).unwrap();
        let usable = usable_area(area, area, &fixed, "DISPLAY1", true);
        assert_eq!((usable.y, usable.height), (48.0, 1322.0), "a fixed dock keeps its band");
        assert_eq!(usable_area(area, usable, &fixed, "DISPLAY1", true), usable);
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
    fn weather_resizes_one_cell_at_a_time_and_keeps_opposite_edges() {
        let registry = mypaper_widgets::registry::builtins();
        let manifest = &registry.get("weather").unwrap().manifest;
        let grid = Grid {
            origin: [16.0, 64.0],
            cell: [96.0; 2],
            gap: 8.0,
            cols: 12,
            rows: 10,
        };
        // Every size between min and max, odd ones included (T3-style 3×4 on the old grid).
        for size in [[4, 4], [4, 5], [5, 5], [5, 6], [7, 3], [9, 9]] {
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
                [6, 8],
                [4, 2],
                huge,
                [false, false, true, true]
            ),
            Some(([6, 8], [6, 2])),
            "bounded by the grid"
        );
        // A side handle changes one dimension by one cell.
        let wider = grid.rect([2, 1], [5, 3]);
        assert_eq!(
            resize_target(&grid, manifest, [2, 1], [4, 3], wider, [false, false, true, false]),
            Some(([2, 1], [5, 3]))
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
    fn keyboard_steps_one_cell_and_clamped_layout_keeps_valid_neighbors() {
        let registry = mypaper_widgets::registry::builtins();
        let timer = &registry.get("timer").unwrap().manifest;
        assert_eq!(nudge_size(timer, [4, 4], 1, -1, [6, 6]), Some([4, 3]));
        assert_eq!(nudge_size(timer, [4, 3], 1, 1, [6, 6]), Some([4, 4]));
        assert_eq!(nudge_size(timer, [2, 2], 0, -1, [6, 6]), None, "at its min");
        assert_eq!(nudge_size(timer, [6, 6], 0, 1, [6, 6]), None, "at the grid edge");
        let grid = Grid {
            origin: [0.0; 2],
            cell: [96.0; 2],
            gap: 8.0,
            cols: 8,
            rows: 6,
        };
        let mut layout = Layout::default();
        let stable = layout.add("notes", "m", [4, 2], [4, 4]);
        let clamped = layout.add("weather", "m", [9, 5], [2, 2]);
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
        assert!(free(
            &grid,
            weather.cell,
            weather.size,
            &next,
            "m",
            Some(&weather.instance)
        ));
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
    fn the_pill_uses_a_clear_edge_inside_the_usable_area() {
        let area = Rect::new(0.0, 48.0, 2560.0, 1322.0);
        let size = (954, 200);
        assert_eq!(pill_origin(area, size, 1.0, &[]), (803, 58));
        let widgets = [Rect::new(1072.0, 64.0, 415.0, 210.0)];
        assert_eq!(pill_origin(area, size, 1.0, &widgets), (803, 1160));
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
    fn the_picker_groups_by_category_and_searches_names_categories_and_ids() {
        let choice = |id: &str, name: &str, category: &str| Choice {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            description: String::new(),
            size: [4, 4],
        };
        let mut choices = vec![
            choice("my-gauge", "Gauge", "Other"),
            choice("t3", "T3 Code", "Integrations"),
            choice("weather", "Weather", "Info"),
            choice("clock", "Clock", "Time"),
            choice("calendar", "Calendar", "Time"),
            choice("stocks", "Stocks", "Finance"),
        ];
        sort_choices(&mut choices);
        let order: Vec<_> = choices.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(order, ["calendar", "clock", "weather", "t3", "stocks", "my-gauge"]);
        let ids = |q: &str| filter(&choices, q).iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("  CLO "), ["clock"]);
        assert_eq!(ids("time"), ["calendar", "clock"]);
        assert_eq!(ids("my-"), ["my-gauge"]);
        assert_eq!(ids("").len(), 6);
        assert!(ids("zzz").is_empty());
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
