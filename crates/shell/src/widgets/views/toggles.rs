//! Quick toggles: Control Center's tiles in a **grid** (the column count that makes them
//! largest), a **bar** of round buttons, or a **list** of rows. Each opens its Settings page;
//! Wi-Fi and battery show their live state.
use super::{Ctx, ITEM, TOGGLES};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Accessory, Align, Lead, ListRow, Rect, Role, TextRole, Tile};

struct Toggle {
    id: u32,
    icon: Icon,
    title: &'static str,
    status: String,
    on: bool,
}

fn toggles(c: &Ctx) -> Vec<Toggle> {
    use mypaper_widgets::stats::LinkKind;
    let stats = c.m.stats.as_ref();
    TOGGLES
        .iter()
        .enumerate()
        .filter(|(_, t)| c.m.flag(t.0, true))
        .map(|(i, (key, title, _, _))| {
            let (icon, status, on) = match *key {
                "wifi" => match stats.and_then(|s| s.network.as_ref()) {
                    Some(n) if n.kind == LinkKind::Wifi => {
                        let signal = n.signal.unwrap_or(0);
                        (
                            Icon::Wifi((signal / 26).min(3)),
                            n.name.clone().unwrap_or_else(|| "Connected".into()),
                            true,
                        )
                    }
                    Some(n) if n.kind == LinkKind::Ethernet => (Icon::Ethernet, "Ethernet".into(), true),
                    Some(_) => (Icon::Offline, "Not connected".into(), false),
                    None => (Icon::Wifi(3), "Wi-Fi".into(), false),
                },
                "bluetooth" => (Icon::Bluetooth, "Settings".into(), false),
                "focus" => (Icon::Moon, "Do not disturb".into(), false),
                "airplane" => (Icon::Airplane, "Off".into(), false),
                "nightlight" => (Icon::NightLight, "Schedule".into(), false),
                "sound" => (Icon::Volume(3), "Output".into(), false),
                "display" => (Icon::Brightness, "Brightness".into(), false),
                _ => match stats.and_then(|s| s.battery) {
                    Some(b) => (
                        if b.charging { Icon::Charging } else { Icon::Battery(b.percent / 10) },
                        format!("{}%{}", b.percent, if b.charging { " · Charging" } else { "" }),
                        b.charging,
                    ),
                    None => (Icon::Plug, "Mains power".into(), false),
                },
            };
            Toggle {
                id: ITEM + i as u32,
                icon,
                title,
                status,
                on,
            }
        })
        .collect()
}

pub fn draw(c: &mut Ctx, r: Rect) {
    let items = toggles(c);
    if items.is_empty() {
        return c.empty(r, Some(Icon::Controls), "No toggles", Some("Choose some in Settings"));
    }
    match c.m.str("variant", "grid").as_str() {
        "row" => bar(c, r, &items),
        "list" => list(c, r, &items),
        _ => grid(c, r, &items),
    }
}

fn grid(c: &mut Ctx, r: Rect, items: &[Toggle]) {
    let gap = c.px(8.0);
    let n = items.len();
    // The column count that makes tiles largest (by their smaller side, wide tiles preferred).
    let (cols, _) = (1..=n)
        .map(|cols| {
            let rows = n.div_ceil(cols);
            let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
            let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
            let score = w.min(h * 2.6).min(c.px(200.0)) * h.min(w).min(c.px(90.0));
            (cols, score)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((1, 0.0));
    let rows = n.div_ceil(cols);
    let w = ((r.width() - gap * (cols - 1) as f32) / cols as f32).floor();
    let h = ((r.height() - gap * (rows - 1) as f32) / rows as f32).floor();
    for (i, t) in items.iter().enumerate() {
        let rect = Rect::new(
            r.left + (i % cols) as f32 * (w + gap),
            r.top + (i / cols) as f32 * (h + gap),
            w,
            h,
        );
        tile(c, rect, t);
    }
}

/// One tile: Control Center's wide tile when there is room, else a compact one (the icon in
/// its circle over the name), or the icon alone.
fn tile(c: &mut Ctx, rect: Rect, t: &Toggle) {
    let color = c.color();
    if rect.width() >= c.px(132.0) && rect.height() >= c.px(46.0) {
        c.f.tile(
            &Tile {
                id: t.id,
                icon: t.icon,
                title: t.title,
                subtitle: &t.status,
                on: t.on,
                enabled: true,
                more: None,
            },
            rect,
        );
        return;
    }
    let radius = c.inner_radius().min(c.px(15.0)).min(rect.height() / 2.0);
    c.f.fill(rect, radius, color.card);
    c.control(t.id, rect, radius, Role::Switch, t.title, Some(t.on));
    let (fill, glyph) = if t.on {
        (color.accent, color.on_accent)
    } else {
        (color.fill, color.label)
    };
    let side = rect.width().min(rect.height());
    if rect.height() <= c.px(44.0) || rect.width() <= c.px(56.0) {
        // The icon in its circle, alone (with the name beside it when the tile is wide).
        let d = (side - c.px(10.0)).clamp(c.px(20.0), c.px(32.0));
        let room = rect.width() - d - c.px(16.0);
        let label = room >= c.px(44.0);
        let disc = if label {
            Rect::new(rect.left + c.px(6.0), rect.top + ((rect.height() - d) / 2.0).round(), d, d)
        } else {
            rect.place(d, d, Align::Center, Align::Center)
        };
        c.f.fill(disc, d / 2.0, fill);
        c.icon(disc, t.icon, (d / c.s() * 0.5).round(), glyph);
        if label {
            let text = Rect { left: disc.right + c.px(8.0), right: rect.right - c.px(6.0), ..rect };
            c.text(text, t.title, TextRole::Subheadline, color.label, Align::Start);
        }
        return;
    }
    let inner = rect.inset(mypaper_ui::Insets::all((side * 0.13).clamp(c.px(8.0), c.px(14.0)).round()));
    let d = (side * 0.4).clamp(c.px(24.0), c.px(40.0)).round();
    let disc = Rect::new(inner.left, inner.top, d, d);
    c.f.fill(disc, d / 2.0, fill);
    c.icon(disc, t.icon, (d / c.s() * 0.5).round(), glyph);
    let (a, b) = (c.f.line(TextRole::Subheadline), c.f.line(TextRole::Caption));
    let status = inner.height() - d >= a + b;
    let y = inner.bottom - a - if status { b } else { 0.0 };
    c.text(Rect::new(inner.left, y, inner.width(), a), t.title, TextRole::Subheadline, color.label, Align::Start);
    if status {
        c.text(Rect::new(inner.left, y + a, inner.width(), b), &t.status, TextRole::Caption, color.secondary, Align::Start);
    }
}

/// A capsule of round buttons.
fn bar(c: &mut Ctx, r: Rect, items: &[Toggle]) {
    let color = c.color();
    let n = items.len() as f32;
    let d = (r.height().min(r.width() / n - c.px(4.0))).clamp(c.px(24.0), c.px(48.0)).floor();
    let step = (r.width() / n).min(d + c.px(12.0));
    let left = r.left + ((r.width() - step * n) / 2.0).round();
    for (i, t) in items.iter().enumerate() {
        let cell = Rect::new(left + i as f32 * step, r.top, step, r.height());
        let disc = cell.place(d, d, Align::Center, Align::Center);
        let (fill, glyph) = if t.on {
            (color.accent, color.on_accent)
        } else {
            (color.fill, color.label)
        };
        c.f.fill(disc, d / 2.0, fill);
        c.control(t.id, disc, d / 2.0, Role::Switch, t.title, Some(t.on));
        c.icon(disc, t.icon, (d / c.s() * 0.5).round(), glyph);
    }
}

/// Rows: the icon in its circle, the name and the status.
fn list(c: &mut Ctx, r: Rect, items: &[Toggle]) {
    let row_h = c.f.row_height(&ListRow {
        id: 0,
        lead: Lead::None,
        title: "",
        subtitle: Some(""),
        accessory: Accessory::None,
        selected: false,
        enabled: true,
    });
    let height = items.len() as f32 * row_h;
    c.scroll(r, height, |c, top| {
        for (i, t) in items.iter().enumerate() {
            let row = ListRow {
                id: t.id,
                lead: Lead::Badge(t.icon, t.on),
                title: t.title,
                subtitle: Some(&t.status),
                accessory: Accessory::Chevron,
                selected: false,
                enabled: true,
            };
            c.f.row(&row, Rect::new(r.left, top + i as f32 * row_h, r.width(), row_h));
        }
    });
}
