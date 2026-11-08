//! The launcher: a search field over the apps, as a **dock** (one row of icons, the hovered one
//! magnified), a **grid** of icons with names, or a **list** of rows. Short widgets keep only
//! the search field.
use super::{Ctx, ITEM, field};
use glaze_ui::icons::Icon;
use glaze_ui::{Accessory, Align, Lead, ListRow, Op, Rect, Role, TextRole};
use std::sync::Arc;

pub fn draw(c: &mut Ctx, r: Rect) {
    let s = c.s();
    let h = c.rect.height() / s;
    let search = c.m.flag("search", true);
    let apps = c.m.list("shortcuts");
    if search && (h <= 124.0 || apps.is_empty() && h <= 200.0) {
        let fh = (h * 0.46).clamp(34.0, 52.0) * s;
        let bar = r.place(r.width(), fh.min(r.height()), Align::Start, Align::Center);
        return field(c, bar, "Search the web", Some(Icon::Search));
    }
    let r = if search {
        let fh = (h * 0.17).clamp(30.0, 40.0).round() * s;
        let (bar, rest) = r.take_top(fh, c.px(10.0));
        field(c, bar, "Search the web", Some(Icon::Search));
        rest
    } else {
        r
    };
    if apps.is_empty() {
        return c.empty(r, Some(Icon::Apps), "No apps yet", Some("Add some in this widget's Settings"));
    }
    match c.m.str("variant", "dock").as_str() {
        "list" => list(c, r, apps.len()),
        "grid" => grid(c, r, apps.len()),
        _ => dock(c, r, apps.len()),
    }
}

fn label(c: &Ctx, i: usize) -> String {
    c.m.list("shortcuts")[i]["label"].as_str().unwrap_or("App").to_owned()
}

/// The launch bounce of app `i`, px up.
fn bounce(c: &Ctx, i: usize) -> f32 {
    if c.m.action_hit != Some(ITEM + i as u32) {
        return 0.0;
    }
    c.m.action_at.map_or(0.0, |at| {
        let t = c.instant.saturating_duration_since(at).as_secs_f32() / 0.5;
        if t >= 2.0 {
            0.0
        } else {
            (t * std::f32::consts::PI).sin().abs() * c.px(7.0)
        }
    })
}

/// App `i`'s icon in `rect`: its image, or its initial on a rounded square.
fn app_icon(c: &mut Ctx, rect: Rect, i: usize) {
    let color = c.color();
    match c.m.icons.get(&i).cloned() {
        Some(image) => contain(c, rect, &image),
        None => {
            c.f.fill(rect, (rect.width() * 0.24).round(), color.fill);
            let letter = label(c, i).chars().next().unwrap_or('•').to_uppercase().to_string();
            let style = c.display((rect.width() / c.s() * 0.42).max(9.0), 600);
            c.styled(rect, &letter, &style, color.label, Align::Center, Align::Center);
        }
    }
}

fn contain(c: &mut Ctx, rect: Rect, image: &Arc<glaze_ui::Image>) {
    let ratio = (rect.width() / image.width as f32).min(rect.height() / image.height as f32);
    let (w, h) = (image.width as f32 * ratio, image.height as f32 * ratio);
    c.f.push(Op::Image {
        rect: rect.place(w, h, Align::Center, Align::Center),
        image: image.clone(),
        opacity: 1.0,
        source: None,
    });
}

/// One row of icons, centred; the hovered one grows (as the web dock's magnification).
fn dock(c: &mut Ctx, r: Rect, n: usize) {
    let s = c.s();
    let labels = c.m.flag("labels", true);
    let label_h = if labels { c.f.line(TextRole::Caption) + c.px(4.0) } else { 0.0 };
    let want = c.m.num("iconSize", 48.0) as f32 * s;
    let gap = c.px(8.0);
    let by_width = (r.width() - gap * (n - 1) as f32) / n as f32;
    let size = want
        .min(by_width)
        .min((r.height() - label_h) * 0.8)
        .clamp(c.px(16.0), c.px(96.0))
        .floor();
    // Too many apps for one row: the grid, whose icons keep a readable size.
    if size < c.px(28.0) {
        return grid(c, r, n);
    }
    let gap = if size < c.px(36.0) { c.px(4.0) } else { gap };
    let cell = (size + gap).min(r.width() / n as f32);
    let left = r.left + ((r.width() - cell * n as f32) / 2.0).round();
    let top = r.top + ((r.height() - size - label_h) / 2.0).round();
    let color = c.color();
    for i in 0..n {
        let id = ITEM + i as u32;
        let cell_rect = Rect::new(left + i as f32 * cell, r.top, cell, r.height());
        let hovered = c.m.hover == Some(id);
        let k = if hovered { 1.18 } else { 1.0 };
        let d = (size * k).round();
        let icon = Rect::new(
            (cell_rect.center().x - d / 2.0).round(),
            top + size - d - bounce(c, i),
            d,
            d,
        );
        c.f.focus_ring(id, icon, (d * 0.24).round());
        c.f.hit(id, cell_rect, Role::Button, &label(c, i), true, None);
        app_icon(c, icon, i);
        if labels {
            let at = Rect::new(cell_rect.left - gap / 2.0, top + size + c.px(4.0), cell + gap, label_h - c.px(4.0));
            let name = label(c, i);
            c.text(at, &name, TextRole::Caption, if hovered { color.label } else { color.secondary }, Align::Center);
        }
    }
}

/// Icons with their names in a grid, scrolling when they don't fit.
fn grid(c: &mut Ctx, r: Rect, n: usize) {
    let color = c.color();
    let s = c.s();
    let icon = (c.m.num("iconSize", 48.0) as f32 * s).clamp(c.px(28.0), c.px(72.0)).floor();
    let min_w = icon * 1.25 + c.px(20.0);
    let cols = ((r.width() / min_w).floor() as usize).clamp(1, n);
    let w = r.width() / cols as f32;
    let labels = c.m.flag("labels", true);
    let row_h = icon + c.px(16.0) + if labels { c.f.line(TextRole::Caption) + c.px(4.0) } else { 0.0 };
    let rows = n.div_ceil(cols);
    let radius = c.inner_radius().min(c.px(13.0));
    c.scroll(r, rows as f32 * row_h, |c, top| {
        for i in 0..n {
            let cell = Rect::new(r.left + (i % cols) as f32 * w, top + (i / cols) as f32 * row_h, w, row_h);
            let id = ITEM + i as u32;
            let name = label(c, i);
            c.control(id, cell.inset(glaze_ui::Insets::all(c.px(2.0))), radius, Role::Button, &name, None);
            let at = Rect::new((cell.center().x - icon / 2.0).round(), cell.top + c.px(8.0) - bounce(c, i), icon, icon);
            app_icon(c, at, i);
            if labels {
                let y = cell.top + c.px(8.0) + icon + c.px(4.0);
                c.text(Rect::new(cell.left + c.px(4.0), y, cell.width() - c.px(8.0), c.f.line(TextRole::Caption)), &name, TextRole::Caption, color.secondary, Align::Center);
            }
        }
    });
}

/// Rows: the icon, the name.
fn list(c: &mut Ctx, r: Rect, n: usize) {
    let row_h = c.px(c.f.theme.metrics.row_single + 4.0);
    let cols = if r.width() >= c.px(300.0) && n > 4 { 2 } else { 1 };
    let w = r.width() / cols as f32;
    let rows = n.div_ceil(cols);
    c.scroll(r, rows as f32 * row_h, |c, top| {
        for i in 0..n {
            let rect = Rect::new(r.left + (i / rows) as f32 * w, top + (i % rows) as f32 * row_h, w, row_h);
            let name = label(c, i);
            let lead = match c.m.icons.get(&i) {
                Some(image) => Lead::Image(image.clone()),
                None => Lead::Monogram(name.chars().next().unwrap_or('•').to_ascii_uppercase()),
            };
            c.f.row(
                &ListRow {
                    id: ITEM + i as u32,
                    lead,
                    title: &name,
                    subtitle: None,
                    accessory: Accessory::None,
                    selected: false,
                    enabled: true,
                },
                rect.offset(0.0, -bounce(c, i)),
            );
        }
    });
}
