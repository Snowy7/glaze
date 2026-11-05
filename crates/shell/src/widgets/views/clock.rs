//! The clock's faces: **digital** (`desk`), **flip**, **analog**, **words** (a greeting) and
//! **world** (cities). Every face sizes itself from the box, as the web clock's container
//! queries did: big numerals, a quiet date under them, nothing else.
use super::Ctx;
use crate::widgets::time;
use mypaper_ui::calendar::{Date, MONTHS, WEEKDAYS};
use mypaper_ui::{Align, Op, Point, Rect, Rgba, TextRole};
use std::f32::consts::{PI, TAU};

pub fn draw(c: &mut Ctx, body: Rect) {
    match c.m.str("variant", "desk").as_str() {
        "analog" => analog(c, body),
        "world" => world(c, body),
        "words" => words(c, body),
        variant => digital(c, body, variant == "flip"),
    }
}

/// The weekday's full English name.
pub fn long_weekday(date: Date) -> &'static str {
    [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ][date.weekday() as usize]
}

/// "14:37" (or "2:37") and "PM" apart.
fn split_time(value: &str, format24: bool) -> (String, Option<String>) {
    match value.rsplit_once(' ') {
        Some((digits, period)) if !format24 => (digits.to_owned(), Some(period.to_owned())),
        _ => (value.to_owned(), None),
    }
}

/// Digital and flip: the time as large as the box allows (by its measured width and the face's
/// height in ems), seconds and AM/PM as a small stack after it, the weekday and date under it
/// (or beside it in a wide strip).
fn digital(c: &mut Ctx, r: Rect, flip: bool) {
    let m = c.m;
    let now = c.now;
    let format24 = m.flag("format24", true);
    let seconds = m.flag("showSeconds", false);
    let show_date = m.flag("showDate", true);
    let (digits, period) = split_time(&now.clock(format24, false), format24);
    let s = c.s();
    let narrow = r.width() <= c.px(130.0);
    let short = r.height() <= c.px(124.0);
    let wide = r.width() >= 3.0 * r.height() && !narrow;
    let side = seconds || period.is_some();
    // The face's height in ems: the numerals' line and the date lines under them.
    let ems = match (show_date, wide, narrow, short) {
        (false, ..) => 0.92,
        (true, true, ..) => 1.0,
        (true, _, true, _) => 1.4,
        (true, _, _, true) => 1.26,
        _ => 1.62,
    };
    let weight = if flip { 600 } else { 500 };
    // Width at 100 px of "00:00" (digits are tabular) and the small stack after it.
    let template: String = digits
        .chars()
        .map(|ch| if ch.is_ascii_digit() { '0' } else { ch })
        .collect();
    let unit = c.display(100.0, weight);
    let digits_w = c.f.text.measure(&template, &unit).0 * if flip { 1.18 } else { 1.0 };
    let small_w = if side {
        let style = c.display(22.0, 650);
        let w = c.f.text.measure("00", &style).0.max(
            period
                .as_deref()
                .map_or(0.0, |p| c.f.text.measure(p, &style).0),
        );
        w + 8.0 * s
    } else {
        0.0
    };
    // The date column of the wide strip takes about a third.
    let room = if wide && show_date { r.width() * 0.62 } else { r.width() };
    let size = ((room / (digits_w + small_w)) * 100.0 / s)
        .min(r.height() / s / ems)
        .clamp(12.0, 220.0)
        .floor();
    let style = c.display(size, weight);
    let line = (size * 0.92 * s).round();
    let date_room = if show_date && !wide {
        r.height() - line
    } else {
        0.0
    };
    let time_w = (digits_w + small_w) * size / 100.0;
    let date_h = if show_date && !wide {
        (size * if short || narrow { 0.3 } else { 0.62 } * s).min(date_room)
    } else {
        0.0
    };
    let top = r.top + ((r.height() - line - date_h) / 2.0).max(0.0).round();
    // A wide strip centres the numerals and the date column beside them as one group.
    let weekday = long_weekday(now.date);
    let wide_date = (wide && show_date).then(|| {
        let big = c.display((size * 0.26).clamp(12.0, 26.0), 600);
        let small = c.display((size * 0.2).clamp(11.0, 20.0), 500);
        let room = r.width() - time_w - c.px(18.0);
        let name = if c.f.text.measure(weekday, &big).0 <= room {
            weekday
        } else {
            &weekday[..3]
        };
        let date = format!("{} {}", now.date.day, MONTHS[now.date.month as usize - 1]);
        let w = c.f.text.measure(name, &big).0.max(c.f.text.measure(&date, &small).0);
        (big, small, name, date, w.min(room))
    });
    let group = time_w + wide_date.as_ref().map_or(0.0, |d| c.px(18.0) + d.4);
    let left = r.left + ((r.width() - group) / 2.0).max(0.0).round();
    let at = Rect::new(left, top, time_w, line);
    if flip {
        flip_digits(c, at, &digits, size, &style);
    } else {
        numerals(c, at, &digits, &style);
    }
    if side {
        // Seconds over the day period, top-aligned with the numerals' cap height.
        let small = c.display((size * 0.22).max(10.0), 650);
        let x = left + digits_w * size / 100.0 + 6.0 * s;
        let col = Rect::new(x, top + line * 0.12, small_w, line * 0.8);
        let color = c.color();
        let (stack, _) = col.take_top(line * 0.3, 0.0);
        if seconds {
            c.styled(
                stack,
                &format!("{:02}", now.second),
                &small,
                color.label,
                Align::Start,
                Align::Center,
            );
        }
        if let Some(period) = &period {
            let under = stack.offset(0.0, if seconds { line * 0.3 } else { 0.0 });
            c.styled(under, period, &small, color.secondary, Align::Start, Align::Center);
        }
    }
    if !show_date {
        return;
    }
    let color = c.color();
    let month = MONTHS[now.date.month as usize - 1];
    if let Some((big, small, weekday, date, w)) = wide_date {
        // A column beside the numerals: the weekday over the date.
        let col = Rect::new((left + time_w + c.px(18.0)).min(r.right), r.top, w + 1.0, r.height());
        let lines = (big.size * 1.25, small.size * 1.3);
        let y = r.top + ((r.height() - lines.0 - lines.1) / 2.0).round();
        c.styled(
            Rect::new(col.left, y, col.width(), lines.0),
            weekday,
            &big,
            color.secondary,
            Align::Start,
            Align::Center,
        );
        c.styled(
            Rect::new(col.left, y + lines.0, col.width(), lines.1),
            &date,
            &small,
            color.tertiary,
            Align::Start,
            Align::Center,
        );
        return;
    }
    let below = Rect::new(r.left, top + line, r.width(), date_h);
    if short || narrow {
        // One line: "Wednesday, 4 November", else shorter forms; never an ellipsis.
        let style = c.display((size * 0.24).clamp(11.0, 18.0), 500);
        let options = [
            format!("{weekday}, {} {month}", now.date.day),
            format!("{} {} {}", &weekday[..3], now.date.day, &month[..3]),
            format!("{} {}", &weekday[..3], now.date.day),
        ];
        let text = options
            .into_iter()
            .find(|d| c.f.text.measure(d, &style).0 <= r.width())
            .unwrap_or_else(|| now.date.day.to_string());
        c.styled(below, &text, &style, color.secondary, Align::Center, Align::Center);
        return;
    }
    let big = c.display((size * 0.24).clamp(12.0, 30.0), 600);
    let small = c.display((size * 0.19).clamp(11.0, 24.0), 500);
    let (a, b) = (big.size * 1.3, small.size * 1.3);
    let y = below.top + ((below.height() - a - b) / 2.0).max(0.0).round();
    c.styled(
        Rect::new(r.left, y, r.width(), a),
        weekday,
        &big,
        color.secondary,
        Align::Center,
        Align::Center,
    );
    c.styled(
        Rect::new(r.left, y + a, r.width(), b),
        &format!("{} {month} {}", now.date.day, now.date.year),
        &small,
        color.tertiary,
        Align::Center,
        Align::Center,
    );
}

/// The numerals with a half-strength colon, as the web clock.
fn numerals(c: &mut Ctx, at: Rect, digits: &str, style: &mypaper_ui::TextStyle) {
    let color = c.color();
    let mut x = at.left;
    for (i, part) in digits.split(':').enumerate() {
        if i > 0 {
            let w = c.f.text.measure(":", style).0;
            c.styled(
                Rect::new(x, at.top, w + 1.0, at.height()),
                ":",
                style,
                color.label.with_alpha(0.5),
                Align::Start,
                Align::Center,
            );
            x += w;
        }
        let w = c.f.text.measure(part, style).0;
        c.styled(
            Rect::new(x, at.top, w + 1.0, at.height()),
            part,
            style,
            color.label,
            Align::Start,
            Align::Center,
        );
        x += w;
    }
}

/// Flip cards: each digit on a card with a hinge, the changed ones folding for a moment.
fn flip_digits(c: &mut Ctx, at: Rect, digits: &str, size: f32, style: &mypaper_ui::TextStyle) {
    let color = c.color();
    let m = c.m;
    let s = c.s();
    let format24 = m.flag("format24", true);
    let phase = m
        .flip_at
        .map(|at| (c.instant.saturating_duration_since(at).as_secs_f32() / 0.5).clamp(0.0, 1.0));
    let previous = m
        .previous_clock
        .map(|old| split_time(&old.clock(format24, false), format24).0);
    let count = digits.chars().count() as f32;
    let cell = at.width() / count.max(1.0);
    let card_style = mypaper_ui::TextStyle {
        size: (style.size * 0.84).round(),
        ..style.clone()
    };
    let radius = (size * 0.12 * s).round();
    for (i, ch) in digits.chars().enumerate() {
        let rect = Rect::new(at.left + i as f32 * cell, at.top, cell, at.height());
        let old = previous.as_ref().and_then(|old| old.chars().nth(i));
        if !ch.is_ascii_digit() {
            c.styled(rect, &ch.to_string(), &card_style, color.tertiary, Align::Center, Align::Center);
            continue;
        }
        let card = rect.inflate(-(size * 0.04 * s).max(1.0));
        c.f.fill(card, radius, color.card);
        let folding = phase.filter(|p| *p < 1.0 && old.is_some_and(|o| o != ch));
        let shown = match folding {
            Some(p) if p < 0.5 => old.unwrap_or(ch),
            _ => ch,
        };
        c.styled(card, &shown.to_string(), &card_style, color.label, Align::Center, Align::Center);
        // The hinge, and while folding a shade over the turning half.
        let mid = (card.top + card.height() / 2.0).round();
        c.f.fill(
            Rect::new(card.left, mid, card.width(), (size * 0.028 * s).max(1.0)),
            0.0,
            color.shadow,
        );
        let fold = folding.map_or(0.0, |p| (p * PI).sin() * card.height() * 0.5);
        c.f.fill(
            Rect::new(card.left, mid - fold, card.width(), fold),
            radius.min(fold / 2.0),
            if fold > 0.0 { color.shadow } else { Rgba::TRANSPARENT },
        );
    }
}

/// An analog dial: a soft disc, minute and hour ticks, the hands, and the seconds hand in the
/// accent. Beside it in a wide box, the weekday and date.
fn analog(c: &mut Ctx, r: Rect) {
    let now = c.now;
    let color = c.color();
    let show_date = c.m.flag("showDate", true);
    let wide = r.width() >= r.height() * 1.8;
    let (dial_box, text_box) = if wide {
        let (dial, rest) = r.take_left(r.height(), c.px(16.0));
        (dial, Some(rest))
    } else {
        (r, None)
    };
    let radius = (dial_box.width().min(dial_box.height()) / 2.0).floor();
    let center = dial_box.center();
    dial(c, center, radius, now, c.m.flag("showSeconds", false), true);
    if let (Some(text), true) = (text_box, show_date) {
        let size = (r.height() / c.s() * 0.16).clamp(13.0, 30.0);
        let big = c.display(size, 650);
        let small = c.display(size * 0.72, 500);
        let (a, b) = (big.size * 1.25, small.size * 1.3);
        let y = text.top + ((text.height() - a - b) / 2.0).round();
        c.styled(
            Rect::new(text.left, y, text.width(), a),
            long_weekday(now.date),
            &big,
            color.label,
            Align::Start,
            Align::Center,
        );
        c.styled(
            Rect::new(text.left, y + a, text.width(), b),
            &format!("{} {}", now.date.day, MONTHS[now.date.month as usize - 1]),
            &small,
            color.secondary,
            Align::Start,
            Align::Center,
        );
    } else if show_date && radius > c.px(60.0) {
        // A date window at three o'clock, as a watch.
        let style = c.f.font(TextRole::Caption);
        let window = Rect::new(
            center.x + radius * 0.34,
            center.y - c.px(9.0),
            radius * 0.36,
            c.px(18.0),
        );
        c.styled(
            window,
            &format!("{} {}", WEEKDAYS[now.date.weekday() as usize].to_uppercase(), now.date.day),
            &style,
            color.secondary,
            Align::Center,
            Align::Center,
        );
    }
}

/// A dial centred on `center`: disc, ticks, hands. `detail` adds minute ticks on big dials.
fn dial(c: &mut Ctx, center: Point, radius: f32, time: super::Stamp, seconds: bool, detail: bool) {
    let color = c.color();
    let s = c.s();
    let disc = Rect::new(center.x - radius, center.y - radius, 2.0 * radius, 2.0 * radius);
    c.f.fill(disc, radius, color.card);
    // Stroke unit: the web dial's clamp(.6, .85% of the dial, 2.6) at 100 units across.
    let u = (radius * 2.0 / 100.0 / s).clamp(0.6, 2.6) * s;
    let minor = detail && radius > c.px(75.0);
    for i in 0..60 {
        let major = i % 5 == 0;
        if !major && !minor {
            continue;
        }
        let angle = i as f32 * TAU / 60.0 - TAU / 4.0;
        let (sin, cos) = angle.sin_cos();
        let outer = radius * 0.93;
        let inner = radius * if major { 0.81 } else { 0.88 };
        c.f.push(Op::Line {
            from: Point::new(center.x + cos * inner, center.y + sin * inner),
            to: Point::new(center.x + cos * outer, center.y + sin * outer),
            width: if major { 1.4 * u } else { 0.75 * u },
            color: if major { color.secondary } else { color.tertiary },
        });
    }
    let hand = |c: &mut Ctx, part: f32, length: f32, width: f32, ink: Rgba| {
        let angle = part * TAU - TAU / 4.0;
        let (sin, cos) = angle.sin_cos();
        c.f.push(Op::Line {
            from: Point::new(center.x - cos * radius * 0.08, center.y - sin * radius * 0.08),
            to: Point::new(center.x + cos * radius * length, center.y + sin * radius * length),
            width,
            color: ink,
        });
    };
    let hours = (time.hour % 12) as f32 / 12.0 + time.minute as f32 / 720.0;
    hand(c, hours, 0.5, 3.4 * u, color.label);
    hand(c, time.minute as f32 / 60.0, 0.76, 2.4 * u, color.label);
    if seconds {
        hand(c, time.second as f32 / 60.0, 0.84, (0.9 * u).max(1.0), color.warning);
    }
    let hub = (2.6 * u).max(c.px(3.0));
    c.f.fill(
        Rect::new(center.x - hub, center.y - hub, 2.0 * hub, 2.0 * hub),
        hub,
        if seconds { color.warning } else { color.label },
    );
}

/// The greeting: "Good afternoon," over the name, the time and date under them.
fn words(c: &mut Ctx, r: Rect) {
    let m = c.m;
    let now = c.now;
    let color = c.color();
    let supplied = m.str("name", "");
    let display = if supplied.is_empty() {
        mypaper_widgets::userinfo::cached()
            .map(|u| u.name.clone())
            .unwrap_or_default()
    } else {
        supplied
    };
    let name = display
        .split_once(',')
        .map_or(display.as_str(), |(_, given)| given)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();
    let greeting = time::greeting(now.hour);
    let s = c.s();
    let footer = if m.flag("showDate", true) || r.height() > c.px(124.0) {
        format!(
            "{} · {}",
            now.clock(m.flag("format24", true), false),
            time::date_label(now.date, false)
        )
    } else {
        String::new()
    };
    if r.height() <= c.px(124.0) || name.is_empty() {
        // One line that fills the width: "Good afternoon, Ada".
        let text = if name.is_empty() {
            greeting.to_owned()
        } else {
            format!("{greeting}, {name}")
        };
        let probe = c.display(100.0, 650);
        let w = c.f.text.measure(&text, &probe).0;
        let size = (r.width() * 0.98 / w * 100.0 / s)
            .min(r.height() / s * if footer.is_empty() { 0.7 } else { 0.42 })
            .clamp(12.0, 64.0);
        let style = c.display(size, 650);
        let line = style.size * 1.2;
        let foot = c.f.line(TextRole::Callout);
        let total = line + if footer.is_empty() { 0.0 } else { foot };
        let y = r.top + ((r.height() - total) / 2.0).round();
        c.styled(Rect::new(r.left, y, r.width(), line), &text, &style, color.label, Align::Start, Align::Center);
        if !footer.is_empty() {
            c.text(Rect::new(r.left, y + line, r.width(), foot), &footer, TextRole::Callout, color.secondary, Align::Start);
        }
        return;
    }
    // The name as large as it fits; the greeting over it, the time and date under it.
    let probe = c.display(100.0, 650);
    let w = c.f.text.measure(&name, &probe).0;
    let size = (r.width() * 0.98 / w * 100.0 / s)
        .min(r.height() / s * 0.42)
        .clamp(16.0, 96.0);
    let style = c.display(size, 650);
    let head = c.display((size * 0.4).clamp(13.0, 30.0), 500);
    let foot = c.display((size * 0.23).clamp(12.0, 20.0), 500);
    let (a, b, d) = (head.size * 1.3, style.size * 1.08, foot.size * 1.5);
    let y = r.top + ((r.height() - a - b - d) / 2.0).max(0.0).round();
    c.styled(Rect::new(r.left, y, r.width(), a), &format!("{greeting},"), &head, color.secondary, Align::Start, Align::Center);
    c.styled(Rect::new(r.left, y + a, r.width(), b), &name, &style, color.label, Align::Start, Align::Center);
    c.styled(Rect::new(r.left, y + a + b, r.width(), d), &footer, &foot, color.tertiary, Align::Start, Align::End);
}

/// Cities: Apple's row of dials when the box is wide and tall enough, else rows of a small dial,
/// the city and its offset, and the time at the right.
fn world(c: &mut Ctx, r: Rect) {
    let zones = c.m.list("zones");
    let color = c.color();
    if zones.is_empty() {
        c.empty(r, Some(mypaper_ui::icons::Icon::Globe), "World clocks", Some("Choose cities in Studio"));
        return;
    }
    let now = c.now;
    let format24 = c.m.flag("format24", true);
    let utc = now.zone("UTC");
    let info = |zone: &serde_json::Value| {
        let tz = zone["tz"].as_str().unwrap_or("UTC").to_owned();
        let label = zone["label"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&tz)
            .to_owned();
        let time = now.zone(&tz);
        let detail = time.map_or_else(
            || "Unknown time zone".to_owned(),
            |t| {
                let day = match t.date.ordinal() - now.date.ordinal() {
                    -1 => "Yesterday",
                    0 => "Today",
                    1 => "Tomorrow",
                    _ => "",
                };
                let minutes = utc.map_or(0, |u| {
                    (t.date.ordinal() - u.date.ordinal()) * 1440
                        + i64::from(t.hour) * 60
                        + i64::from(t.minute)
                        - i64::from(u.hour) * 60
                        - i64::from(u.minute)
                });
                let local = now.zone("UTC").map_or(0, |u| {
                    (now.date.ordinal() - u.date.ordinal()) * 1440
                        + i64::from(now.hour) * 60
                        + i64::from(now.minute)
                        - i64::from(u.hour) * 60
                        - i64::from(u.minute)
                });
                let delta = (minutes - local) as f32 / 60.0;
                let hours = if delta.fract() == 0.0 {
                    format!("{:+}", delta as i64)
                } else {
                    format!("{delta:+.1}")
                };
                format!("{day}, {hours} h")
            },
        );
        (label, time, detail)
    };
    let s = c.s();
    let dials = r.height() >= c.px(150.0) && r.width() >= r.height() * 1.45;
    if dials {
        // Up to as many dials as fit at 80 px, each with its city and offset under it.
        let n = zones.len().min((r.width() / c.px(80.0)).floor().max(1.0) as usize);
        let w = r.width() / n as f32;
        let name_h = c.f.line(TextRole::Headline);
        let detail_h = c.f.line(TextRole::Footnote);
        let radius = ((w - c.px(12.0)).min(r.height() - name_h - detail_h - c.px(10.0)) / 2.0).floor();
        let block = radius * 2.0 + c.px(10.0) + name_h + detail_h;
        let top = r.top + ((r.height() - block) / 2.0).round();
        for (i, zone) in zones.iter().take(n).enumerate() {
            let (label, time, detail) = info(zone);
            let col = Rect::new(r.left + i as f32 * w, top, w, block);
            if let Some(time) = time {
                dial(c, Point::new(col.center().x, top + radius), radius, time, false, false);
            }
            let y = top + radius * 2.0 + c.px(10.0);
            c.text(Rect::new(col.left, y, col.width(), name_h), &label, TextRole::Headline, color.label, Align::Center);
            c.text(Rect::new(col.left, y + name_h, col.width(), detail_h), &detail, TextRole::Footnote, color.secondary, Align::Center);
        }
        return;
    }
    let cols = if r.width() >= c.px(400.0) && zones.len() > 3 { 2 } else { 1 };
    let rows = zones.len().div_ceil(cols);
    let gap = c.px(16.0);
    let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
    let row = (r.height() / rows as f32).clamp(c.px(44.0), c.px(64.0)).floor();
    let total = rows as f32 * row;
    c.scroll(r, total, |c, top| {
        let top = top + ((r.height() - total) / 2.0).max(0.0).round();
        for (i, zone) in zones.iter().enumerate() {
            let (label, time, detail) = info(zone);
            let cell = Rect::new(
                r.left + (i / rows) as f32 * (w + gap),
                top + (i % rows) as f32 * row,
                w,
                row,
            );
            let d = (row * 0.62).min(c.px(34.0)).floor();
            let (dial_box, rest) = cell.take_left(d, c.px(10.0));
            if let Some(time) = time {
                dial(c, dial_box.center(), d / 2.0, time, false, false);
            }
            let value = time.map(|t| t.clock(format24, false)).unwrap_or_default();
            let style = c.display((row / s * 0.4).clamp(17.0, 26.0), 500);
            let value_w = c.f.text.measure(&value, &style).0;
            let (value_box, text) = rest.take_right(value_w + 1.0, c.px(8.0));
            c.styled(value_box, &value, &style, color.label, Align::End, Align::Center);
            let (a, b) = (c.f.line(TextRole::Headline), c.f.line(TextRole::Footnote));
            let y = text.top + ((text.height() - a - b) / 2.0).round();
            c.text(Rect::new(text.left, y, text.width(), a), &label, TextRole::Headline, color.label, Align::Start);
            c.text(Rect::new(text.left, y + a, text.width(), b), &detail, TextRole::Footnote, color.secondary, Align::Start);
        }
    });
}
