//! Countdown to a date: the leading unit large ("12 days"), the rest under it ("4 h 20 min"),
//! the title over it and the date at the foot, with a track of the time gone. The **ring**
//! style puts the number inside a ring of the same progress. After the date it counts up
//! ("Since …") or stops.
use super::Ctx;
use crate::widgets::time::{self, Stamp};
use mypaper_ui::{Align, Point, Rect, TextRole};

pub fn draw(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let m = c.m;
    let now = c.now;
    let (target, start) = m.countdown(now);
    let left = target - now.ms;
    let after = left <= 0;
    let up = m.str("afterwards", "countUp") == "countUp";
    let seconds_shown = m.flag("showSeconds", false);
    let quantum = if seconds_shown { 1000 } else { 60_000 };
    let seconds = if after {
        (-left) / quantum * quantum / 1000
    } else {
        (left + quantum - 1) / quantum * quantum / 1000
    };
    let units = [
        (seconds / 86400, "day", "d"),
        (seconds / 3600 % 24, "hour", "h"),
        (seconds / 60 % 60, "minute", "min"),
        (seconds % 60, "second", "s"),
    ];
    let end = if seconds_shown { 4 } else { 3 };
    let lead = units[..end].iter().position(|(n, ..)| *n > 0).unwrap_or(end - 1);
    let progress = ((now.ms - start) as f64 / (target - start).max(1) as f64).clamp(0.0, 1.0) as f32;
    let target_day = Stamp::at(target).date;
    let stopped = after && (!up || target_day == now.date);
    let (hero, unit) = if stopped {
        (if target_day == now.date { "Today!" } else { "Done" }.to_owned(), String::new())
    } else {
        let (n, word, _) = units[lead];
        (n.to_string(), format!("{word}{}", if n == 1 { "" } else { "s" }))
    };
    let rest = if stopped {
        String::new()
    } else {
        units[lead + 1..end]
            .iter()
            .filter(|(n, ..)| *n > 0)
            .map(|(n, _, short)| format!("{n} {short}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let title = m.str("title", "");
    let title = if after && up && !title.is_empty() && !stopped {
        format!("Since {title}")
    } else {
        title
    };
    let foot = time::date_label(target_day, false);
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let tiny = w < 120.0 || h < 80.0;
    if m.str("style", "digits") == "ring" && !tiny {
        // The ring with the number inside, the words beside or under it.
        let beside = w / h >= 1.5;
        let (ring_box, text) = if beside {
            r.take_left(r.height(), c.px(16.0))
        } else {
            r.take_top((r.height() - c.px(48.0)).min(r.width()), c.px(6.0))
        };
        let d = ring_box.width().min(ring_box.height()).floor();
        let center = ring_box.center();
        let stroke = (d / s * 0.05).clamp(3.0, 6.0).round() * s;
        c.f.ring(Point::new(center.x, center.y), d / 2.0 - stroke / 2.0, stroke, progress, Some(color.accent));
        let size = (d / s * 0.3).clamp(14.0, 56.0);
        let style = c.display(size, 500);
        let inner = Rect::new(center.x - d * 0.36, center.y - d * 0.3, d * 0.72, d * 0.42);
        c.styled(inner, &hero, &style, color.label, Align::Center, Align::End);
        let below = Rect::new(inner.left, inner.bottom, inner.width(), c.f.line(TextRole::Caption));
        c.text(below, &unit.to_uppercase(), TextRole::Caption, color.secondary, Align::Center);
        let lines = [(title.as_str(), TextRole::Headline, color.label), (rest.as_str(), TextRole::Callout, color.secondary), (foot.as_str(), TextRole::Footnote, color.tertiary)];
        let total: f32 = lines.iter().filter(|l| !l.0.is_empty()).map(|l| c.f.line(l.1)).sum();
        let mut y = text.top + if beside { ((text.height() - total) / 2.0).round() } else { 0.0 };
        for (value, role, ink) in lines {
            if value.is_empty() {
                continue;
            }
            let lh = c.f.line(role);
            c.text(Rect::new(text.left, y, text.width(), lh), value, role, ink, if beside { Align::Start } else { Align::Center });
            y += lh;
        }
        return;
    }
    // Digits: the number large with its unit word after it.
    let short = h <= 124.0 && w >= 170.0;
    let title_h = if title.is_empty() || tiny { 0.0 } else { c.f.line(TextRole::Subheadline) };
    let rest_h = if rest.is_empty() || tiny { 0.0 } else { c.f.line(TextRole::Callout) };
    let foot_h = if h >= 130.0 { c.f.line(TextRole::Footnote) } else { 0.0 };
    let track_h = if h >= 118.0 && !after { c.px(10.0) } else { 0.0 };
    let probe = c.display(100.0, 500);
    let unit_probe = c.display(34.0, 500);
    let hero_w = c.f.text.measure(&hero, &probe).0 + c.f.text.measure(&unit, &unit_probe).0 + 8.0 * s;
    let room_h = r.height() - title_h - rest_h - foot_h - track_h;
    let size = ((r.width() / hero_w * 100.0 / s).min(room_h / s / 1.05)).clamp(16.0, 96.0).floor();
    let style = mypaper_ui::TextStyle {
        tracking: -0.035 * size * s,
        ..c.display(size, 500)
    };
    let unit_style = c.display((size * 0.34).max(11.0), 500);
    let hero_h = (style.size * 1.05).round();
    let total = title_h + hero_h + rest_h + foot_h + track_h;
    let mut y = r.top + ((r.height() - total) / 2.0).max(0.0).round();
    let align = if short { Align::Start } else { Align::Center };
    if title_h > 0.0 {
        let tw = (c.measure(&title, TextRole::Subheadline).0 + c.px(2.0)).min(r.width() - c.px(12.0));
        let d = c.px(6.0);
        let group = Rect::new(r.left, y, r.width(), title_h).place(tw + d + c.px(6.0), title_h, align, Align::Center);
        c.f.fill(Rect::new(group.left, (group.center().y - d / 2.0).round(), d, d), d / 2.0, color.accent);
        c.text(Rect { left: group.left + d + c.px(6.0), ..group }, &title, TextRole::Subheadline, color.secondary, Align::Start);
        y += title_h;
    }
    let (nw, _) = c.f.text.measure(&hero, &style);
    let uw = if unit.is_empty() { 0.0 } else { c.f.text.measure(&unit, &unit_style).0 + c.px(6.0) };
    let group = Rect::new(r.left, y, r.width(), hero_h).place((nw + uw).min(r.width()), hero_h, align, Align::Center);
    c.styled(Rect { right: group.left + nw + 1.0, ..group }, &hero, &style, color.label, Align::Start, Align::Center);
    if !unit.is_empty() {
        let at = Rect { left: group.left + nw + c.px(6.0), top: group.top + hero_h * 0.22, ..group };
        c.styled(at, &unit, &unit_style, color.secondary, Align::Start, Align::Center);
    }
    y += hero_h;
    if rest_h > 0.0 {
        c.text(Rect::new(r.left, y, r.width(), rest_h), &rest, TextRole::Callout, color.secondary, align);
        y += rest_h;
    }
    if track_h > 0.0 {
        let bar = Rect::new(r.left, y + c.px(4.0), r.width(), c.px(4.0));
        c.f.progress(bar, progress, Some(color.accent));
        y += track_h;
    }
    if foot_h > 0.0 {
        c.text(Rect::new(r.left, y, r.width(), foot_h), &foot, TextRole::Footnote, color.tertiary, align);
    }
}
