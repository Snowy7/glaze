//! System monitor: **rings** (each metric a gauge with its value inside and its name under it,
//! on the grid that makes them largest), **bars** (a row per metric: icon, name and detail,
//! value, a track) and **graphs** (a tile per metric with its value and recent history).
//! Arcs and tracks are the accent (the label's ink in the calm looks); a warning is red,
//! charging green.
use super::Ctx;
use crate::widgets::metrics::{self, Metric};
use glaze_ui::icons::Icon;
use glaze_ui::{Align, Insets, Op, Point, Rect, Rgba, TextRole};

pub fn draw(c: &mut Ctx, r: Rect) {
    if c.m.stats.is_none() {
        return c.empty(r, Some(Icon::Gauge), "Reading your PC", None);
    }
    let list = metrics::visible(c.m);
    if list.is_empty() {
        return c.empty(r, Some(Icon::Gauge), "Nothing to show", Some("Turn on a metric in Settings"));
    }
    match c.m.str("variant", "rings").as_str() {
        "bars" => bars(c, r, &list),
        "spark" => graphs(c, r, &list),
        _ => rings(c, r, &list),
    }
}

/// The metric's colour: warnings red, charging green, else the accent.
fn tint(c: &Ctx, metric: &Metric) -> Rgba {
    let color = c.color();
    let full = metric.id.starts_with("disk-") && metric.percent >= 90.0;
    match (metric.id.as_str(), metric.detail.as_str()) {
        ("battery", "Low") => color.destructive,
        ("battery", "Charging") => color.success,
        _ if full => color.destructive,
        _ => color.accent,
    }
}

fn icon(metric: &Metric) -> Icon {
    match metric.id.as_str() {
        "cpu" => Icon::Cpu,
        "ram" => Icon::Layers,
        "gpu" => Icon::Gauge,
        "net" => Icon::Download,
        "battery" => Icon::Battery((metric.percent / 10.0).round() as u8),
        _ => Icon::Folder,
    }
}

/// The value shown big: the download rate for the network (upload goes in the detail).
fn short(metric: &Metric) -> (String, String) {
    if metric.id == "net" {
        let (down, up) = metric.value.split_once(' ').unwrap_or((&metric.value, ""));
        (down.trim_start_matches('↓').to_owned(), up.to_owned())
    } else {
        (metric.value.clone(), metric.detail.clone())
    }
}

/// The number and its unit apart ("23" and "%", "1.2" and "MB/s").
fn split_unit(value: &str) -> (&str, &str) {
    let end = value
        .char_indices()
        .find(|(_, ch)| !(ch.is_ascii_digit() || *ch == '.' || *ch == ','))
        .map_or(value.len(), |(i, _)| i);
    (&value[..end], value[end..].trim())
}

fn rings(c: &mut Ctx, r: Rect, list: &[Metric]) {
    let color = c.color();
    let s = c.s();
    let n = list.len();
    let gap = c.px(8.0);
    let captions = r.height() / n.div_ceil(n.min(((r.width() + gap) / c.px(68.0)) as usize).max(1)) as f32 > c.px(104.0);
    let caption_h = if captions { c.px(34.0) } else { 0.0 };
    // The column count that gives the largest ring; a lone ring on the last row costs a little.
    let (cols, d) = (1..=n)
        .map(|cols| {
            let rows = n.div_ceil(cols);
            let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
            let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
            let d = w.min(h - caption_h).min(c.px(132.0));
            let filled = (n as f32 / (rows * cols) as f32).sqrt();
            (cols, d, d * filled)
        })
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(cols, d, _)| (cols, d))
        .unwrap_or((1, c.px(40.0)));
    let rows = n.div_ceil(cols);
    let d = d.max(c.px(36.0)).floor();
    let cell_w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
    let cell_h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
    let stroke = (d / s * 0.075).clamp(4.0, 7.0).round() * s;
    for (i, metric) in list.iter().enumerate() {
        let (col, row) = (i % cols, i / cols);
        // The last row's rings are centred.
        let in_row = if row == rows - 1 { n - row * cols } else { cols };
        let offset = (cols - in_row) as f32 * (cell_w + gap) / 2.0;
        let cell = Rect::new(
            r.left + offset + col as f32 * (cell_w + gap),
            r.top + row as f32 * (cell_h + gap),
            cell_w,
            cell_h,
        );
        let block = d + caption_h;
        let top = cell.top + ((cell.height() - block) / 2.0).round();
        let center = Point::new(cell.center().x.round(), top + d / 2.0);
        let ink = tint(c, metric);
        c.f.ring(center, d / 2.0 - stroke / 2.0, stroke, (metric.percent / 100.0) as f32, Some(ink));
        let (value, detail) = short(metric);
        let (number, unit) = split_unit(&value);
        let size = (d / s * if metric.id == "net" { 0.2 } else { 0.25 }).clamp(11.0, 32.0);
        let big = c.display(size, 600);
        let small = c.display((size * 0.5).max(9.0), 600);
        let (nw, nh) = c.f.text.measure(number, &big);
        let uw = c.f.text.measure(unit, &small).0;
        let inner = d - 2.0 * stroke - c.px(6.0);
        let (big, small, nw, uw) = if nw + uw > inner {
            let k = inner / (nw + uw);
            let big = c.display((size * k).max(9.0), 600);
            let small = c.display((size * 0.5 * k).max(8.0), 600);
            (big.clone(), small.clone(), c.f.text.measure(number, &big).0, c.f.text.measure(unit, &small).0)
        } else {
            (big, small, nw, uw)
        };
        let x = center.x - (nw + uw) / 2.0;
        let label_inside = !captions && d > c.px(64.0);
        let y = center.y - nh / 2.0 - if label_inside { c.px(5.0) } else { 0.0 };
        c.styled(Rect::new(x, y, nw + 1.0, nh), number, &big, color.label, Align::Start, Align::Center);
        let baseline = Rect::new(x + nw, y, uw + 1.0, nh);
        c.styled(baseline.offset(0.0, nh * 0.12), unit, &small, color.secondary, Align::Start, Align::Center);
        if label_inside {
            let style = c.f.font(TextRole::Caption);
            let at = Rect::new(center.x - inner / 2.0, y + nh - c.px(1.0), inner, c.f.line(TextRole::Caption));
            c.styled(at, &metric.label, &style, color.secondary, Align::Center, Align::Center);
        }
        if captions {
            let y = top + d + c.px(6.0);
            let (a, b) = (c.f.line(TextRole::Subheadline), c.f.line(TextRole::Caption));
            c.text(Rect::new(cell.left, y, cell.width(), a), &metric.label, TextRole::Subheadline, color.label, Align::Center);
            c.text(Rect::new(cell.left, y + a, cell.width(), b), &detail, TextRole::Caption, color.secondary, Align::Center);
        }
    }
}

fn bars(c: &mut Ctx, r: Rect, list: &[Metric]) {
    let color = c.color();
    let n = list.len();
    let two = r.width() >= c.px(340.0) && n > 2;
    let cols = if two { 2 } else { 1 };
    let gap = c.px(18.0);
    // "This PC" and the uptime when there is a row to spare.
    let rows_fit = (r.height() / c.px(40.0)).floor().max(1.0) as usize;
    let header = rows_fit > n.div_ceil(cols) && r.height() >= c.px(150.0);
    let r = if header {
        let (head, rest) = r.take_top(c.f.line(TextRole::Headline), c.px(8.0));
        c.text(head, "This PC", TextRole::Headline, color.label, Align::Start);
        if let Some(stats) = &c.m.stats {
            let up = uptime(stats.uptime);
            c.text(head, &up, TextRole::Footnote, color.secondary, Align::End);
        }
        rest
    } else {
        r
    };
    let rows = n.div_ceil(cols).min(((r.height() / c.px(36.0)).floor() as usize).max(1));
    let shown = n.min(rows * cols);
    let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
    let row_h = (r.height() / rows as f32).min(c.px(58.0)).floor();
    let top = r.top + ((r.height() - row_h * rows as f32) / 2.0).max(0.0).round();
    for (i, metric) in list.iter().take(shown).enumerate() {
        let cell = Rect::new(
            r.left + (i / rows) as f32 * (w + gap),
            top + (i % rows) as f32 * row_h,
            w,
            row_h,
        );
        let ink = tint(c, metric);
        let with_icon = w > c.px(130.0) && row_h >= c.px(40.0);
        let content = if with_icon {
            let d = c.px(28.0);
            let (badge, rest) = cell.take_left(d, c.px(10.0));
            let badge = badge.place(d, d, Align::Start, Align::Center);
            c.f.fill(badge, c.px(8.0), ink.with_alpha(0.16));
            c.icon(badge, icon(metric), 15.0, ink);
            rest
        } else {
            cell
        };
        let (name_h, track_h) = (c.f.line(TextRole::Subheadline), c.px(5.0));
        let block = name_h + c.px(5.0) + track_h;
        let y = content.top + ((content.height() - block) / 2.0).round();
        let line = Rect::new(content.left, y, content.width(), name_h);
        let value_style = c.display(15.0, 600);
        let vw = c.f.text.measure(&metric.value, &value_style).0;
        let (value_box, label_box) = line.take_right(vw + 1.0, c.px(8.0));
        c.styled(value_box.inset(Insets::xy(0.0, -c.px(2.0))), &metric.value, &value_style, color.label, Align::End, Align::Center);
        let lw = c.text(label_box, &metric.label, TextRole::Subheadline, color.label, Align::Start);
        if w > c.px(200.0) && !metric.detail.is_empty() {
            let detail = Rect { left: label_box.left + lw + c.px(6.0), ..label_box };
            if detail.width() > c.px(30.0) {
                c.text(detail, &metric.detail, TextRole::Caption, color.secondary, Align::Start);
            }
        }
        let track = Rect::new(content.left, y + name_h + c.px(5.0), content.width(), track_h);
        if metric.id == "net" {
            sparkline(c, track.inset(Insets::xy(0.0, -c.px(4.0))), &metric.id, ink);
        } else {
            c.f.progress(track, (metric.percent / 100.0) as f32, Some(ink));
        }
    }
}

fn graphs(c: &mut Ctx, r: Rect, list: &[Metric]) {
    let color = c.color();
    let gap = c.px(8.0);
    let n = list.len();
    let cols = ((r.width() + gap) / c.px(112.0)).floor().clamp(1.0, n as f32) as usize;
    let rows = n.div_ceil(cols).min(((r.height() + gap) / c.px(70.0)).floor().max(1.0) as usize);
    let shown = n.min(rows * cols);
    let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
    let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
    let radius = c.inner_radius().min(c.px(14.0));
    let s = c.s();
    for (i, metric) in list.iter().take(shown).enumerate() {
        let tile = Rect::new(
            r.left + (i % cols) as f32 * (w + gap),
            r.top + (i / cols) as f32 * (h + gap),
            w,
            h,
        );
        c.f.fill(tile, radius, color.card);
        let inner = tile.inset(Insets::xy(c.px(11.0), c.px(9.0)));
        let ink = tint(c, metric);
        let (head, rest) = inner.take_top(c.f.line(TextRole::Caption), c.px(2.0));
        c.text(head, &metric.label, TextRole::Caption, color.secondary, Align::Start);
        let size = (tile.width().min(tile.height()) / s * 0.2).clamp(17.0, 28.0);
        let style = c.display(size, 600);
        let (value_box, plot) = rest.take_top((style.size * 1.15).round(), c.px(4.0));
        let (value, _) = short(metric);
        c.styled(value_box, &value, &style, color.label, Align::Start, Align::Center);
        sparkline(c, plot, &metric.id, ink);
    }
}

/// The metric's recent history as a line across `r` (network scaled to its own peak).
fn sparkline(c: &mut Ctx, r: Rect, id: &str, ink: Rgba) {
    let Some(history) = c.m.history.get(id) else { return };
    if history.len() < 2 || r.height() <= 1.0 {
        return;
    }
    let axis = if id == "net" {
        history.iter().copied().fold(262_144.0_f64, f64::max)
    } else {
        100.0
    };
    let step = r.width() / (history.len() - 1) as f32;
    let y = |v: f64| r.bottom - (v / axis).clamp(0.0, 1.0) as f32 * r.height();
    let width = c.px(1.7).max(1.5);
    for (i, pair) in history.iter().collect::<Vec<_>>().windows(2).enumerate() {
        c.f.push(Op::Line {
            from: Point::new(r.left + i as f32 * step, y(*pair[0])),
            to: Point::new(r.left + (i + 1) as f32 * step, y(*pair[1])),
            width,
            color: ink,
        });
    }
}

fn uptime(seconds: f64) -> String {
    let minutes = (seconds / 60.0) as u64;
    match minutes {
        0..60 => format!("Up {minutes} min"),
        60..1440 => format!("Up {} h {} min", minutes / 60, minutes % 60),
        _ => format!("Up {} d {} h", minutes / 1440, minutes / 60 % 24),
    }
}
