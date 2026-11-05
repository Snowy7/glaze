//! Activity: finished tasks and focus sessions per day. **Weeks** (a column per week, as
//! GitHub's graph, with the total and a legend), **month** (this month's days as squares with
//! their numbers) or **streak** (the days in a row, and the last four weeks as a strip).
//! Squares step from the quiet fill to the accent in four levels.
use super::{Ctx, ITEM};
use mypaper_ui::calendar::{Date, MONTHS, MonthGrid, WEEKDAYS};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Rect, Rgba, Role, TextRole};

/// A day's level, 0..=4, against the busier days (the 85th percentile).
fn levels(c: &Ctx) -> impl Fn(u32) -> u32 + use<> {
    let mut busy: Vec<_> = c.m.counts.values().filter(|&&v| v > 0).copied().collect();
    busy.sort_unstable();
    let max = busy.get(busy.len() * 85 / 100).copied().unwrap_or(1).max(1);
    move |n| if max <= 4 { n.min(4) } else { (n * 4).div_ceil(max).min(4) }
}

fn shade(c: &Ctx, level: u32, future: bool) -> Rgba {
    let color = c.color();
    if future {
        return color.well;
    }
    match level {
        0 => color.fill,
        1 => color.accent.with_alpha(0.42),
        2 => color.accent.with_alpha(0.66),
        3 => color.accent.with_alpha(0.86),
        _ => color.accent,
    }
}

pub fn draw(c: &mut Ctx, r: Rect) {
    match c.m.str("variant", "weeks").as_str() {
        "streak" => streak(c, r),
        "month" => month(c, r),
        _ => weeks(c, r),
    }
}

fn count(c: &Ctx, date: Date) -> u32 {
    c.m.counts.get(&date.key()).copied().unwrap_or(0)
}

/// A square for `date` with its hit (and its tooltip text as the label).
fn square(c: &mut Ctx, id: u32, rect: Rect, date: Date, level: &impl Fn(u32) -> u32) {
    let n = count(c, date);
    let future = date > c.now.date;
    let radius = (rect.width() * 0.28).round();
    c.f.fill(rect, radius, shade(c, level(n), future));
    let label = if n == 0 {
        format!("{}: no activity", super::super::time::date_label(date, false))
    } else {
        format!("{}: {n} done", super::super::time::date_label(date, false))
    };
    c.f.wash(id, rect, radius, true);
    c.f.hit(id, rect, Role::Button, &label, true, None);
}

fn weeks(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let total: u32 = c.m.counts.values().sum();
    let level = levels(c);
    let h = c.rect.height() / s;
    let labels = c.m.flag("labels", true);
    let legend = c.m.flag("legend", true) && h > 120.0;
    // The total over its caption at the top.
    let (head, rest) = if h > 120.0 {
        let size = (h * 0.15).clamp(20.0, 40.0);
        let style = c.display(size, 500);
        let big = (style.size * 1.1).round();
        let (head, rest) = r.take_top(big, c.px(8.0));
        let tw = c.f.text.measure(&total.to_string(), &style).0;
        c.styled(head, &total.to_string(), &style, color.label, Align::Start, Align::Center);
        let caption = Rect { left: head.left + tw + c.px(8.0), ..head };
        c.text(caption, "done", TextRole::Footnote, color.secondary, Align::Start);
        (Some(head), rest)
    } else {
        (None, r)
    };
    let (plot, foot) = if legend {
        let (foot, plot) = rest.take_bottom(c.f.line(TextRole::Caption), c.px(6.0));
        (plot, Some(foot))
    } else {
        (rest, None)
    };
    let gap = c.px(3.0);
    let month_row = if labels && plot.height() >= c.px(70.0) { c.f.line(TextRole::Caption) } else { 0.0 };
    let cell = ((plot.height() - month_row - gap * 6.0) / 7.0).clamp(c.px(4.0), c.px(20.0)).floor();
    let gap = if cell < c.px(9.0) { c.px(2.0) } else { gap };
    let label_w = if labels && cell >= c.px(9.0) { c.px(26.0) } else { 0.0 };
    let cols = ((plot.width() - label_w + gap) / (cell + gap)).floor().clamp(4.0, 53.0) as usize;
    let week_start = if c.m.str("weekStart", "monday") == "sunday" { 0 } else { 1 };
    let today = c.now.date;
    let first = today.add_days(-i64::from((today.weekday() + 7 - week_start) % 7) - (cols as i64 - 1) * 7);
    let grid_w = cols as f32 * (cell + gap) - gap;
    let left = plot.right - grid_w;
    // A short strip keeps the total at its left when the weeks leave room for it.
    if head.is_none() && left - plot.left - label_w >= c.px(90.0) {
        let style = c.display(24.0, 500);
        let (a, b) = ((style.size * 1.15).round(), c.f.line(TextRole::Caption));
        let y = plot.top + ((plot.height() - a - b) / 2.0).round();
        let w = left - plot.left - label_w - c.px(12.0);
        c.styled(Rect::new(plot.left, y, w, a), &total.to_string(), &style, color.label, Align::Start, Align::Center);
        c.text(Rect::new(plot.left, y + a, w, b), "done", TextRole::Caption, color.secondary, Align::Start);
    }
    let top = plot.top + month_row + ((plot.height() - month_row - 7.0 * (cell + gap) + gap) / 2.0).max(0.0).round();
    if label_w > 0.0 {
        for row in [1, 3, 5] {
            let d = (week_start + row) % 7;
            let at = Rect::new(plot.left, top + row as f32 * (cell + gap), label_w, cell);
            c.text(at, WEEKDAYS[d as usize], TextRole::Caption, color.tertiary, Align::Start);
        }
    }
    let mut label_end = f32::MIN;
    for col in 0..cols {
        for row in 0..7 {
            let date = first.add_days((col * 7 + row) as i64);
            let x = left + col as f32 * (cell + gap);
            if month_row > 0.0 && row == 0 && date.day <= 7 && x >= label_end {
                let name = &MONTHS[date.month as usize - 1][..3];
                // Only where the whole name fits before the plot's right edge.
                if x + c.measure(name, TextRole::Caption).0 <= plot.right {
                    let at = Rect::new(x, plot.top, plot.right - x, month_row);
                    label_end = x + c.text(at, name, TextRole::Caption, color.secondary, Align::Start) + c.px(6.0);
                }
            }
            if date > today {
                continue;
            }
            let rect = Rect::new(x, top + row as f32 * (cell + gap), cell, cell);
            square(c, ITEM + (col * 7 + row) as u32, rect, date, &level);
        }
    }
    if let Some(foot) = foot {
        // Less ▪▪▪▪▪ More, at the right.
        let d = c.px(10.0);
        let words = foot.width() > c.px(340.0);
        let more_w = if words { c.measure("More", TextRole::Caption).0 + c.px(6.0) } else { 0.0 };
        let less_w = if words { c.measure("Less", TextRole::Caption).0 + c.px(6.0) } else { 0.0 };
        let width = less_w + 5.0 * (d + c.px(3.0)) - c.px(3.0) + more_w;
        let mut x = foot.right - width;
        if words {
            c.text(Rect::new(x, foot.top, less_w, foot.height()), "Less", TextRole::Caption, color.tertiary, Align::Start);
            x += less_w;
        }
        for l in 0..5 {
            let sq = Rect::new(x, (foot.center().y - d / 2.0).round(), d, d);
            c.f.fill(sq, (d * 0.28).round(), shade(c, l, false));
            x += d + c.px(3.0);
        }
        if words {
            c.text(Rect::new(x + c.px(3.0), foot.top, more_w, foot.height()), "More", TextRole::Caption, color.tertiary, Align::Start);
        }
    }
}

fn month(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let level = levels(c);
    let today = c.now.date;
    let week_start = if c.m.str("weekStart", "monday") == "sunday" { 0 } else { 1 };
    let grid = MonthGrid::new(today, week_start);
    let rows = grid.cells.len() / 7;
    let wide = r.width() >= r.height() * 1.5;
    let (title_box, plot) = if wide {
        r.take_left((r.width() * 0.34).round(), c.px(16.0))
    } else {
        let size = (c.rect.height() / s * 0.13).clamp(18.0, 40.0);
        r.take_top((size * 1.2 * s).round(), c.px(8.0))
    };
    let size = if wide {
        (c.rect.height() / s * 0.24).min(c.rect.width() / s * 0.08).clamp(18.0, 56.0)
    } else {
        (c.rect.height() / s * 0.13).clamp(18.0, 40.0)
    };
    let style = c.display(size, 500);
    let total: u32 = grid.cells.iter().flatten().map(|d| count(c, *d)).sum();
    if wide {
        let (a, b) = ((style.size * 1.15).round(), c.f.line(TextRole::Footnote));
        let y = title_box.top + ((title_box.height() - a - b) / 2.0).round();
        c.styled(Rect::new(title_box.left, y, title_box.width(), a), MONTHS[today.month as usize - 1], &style, color.label, Align::Start, Align::Center);
        c.text(Rect::new(title_box.left, y + a, title_box.width(), b), &format!("{total} done"), TextRole::Footnote, color.secondary, Align::Start);
    } else {
        c.styled(title_box, MONTHS[today.month as usize - 1], &style, color.label, Align::Start, Align::Center);
        c.text(title_box, &format!("{total} done"), TextRole::Footnote, color.secondary, Align::End);
    }
    let u = (plot.width() / 7.0).min(plot.height() / rows as f32).floor();
    let gap = (u * 0.1).max(c.px(2.0)).round();
    let left = plot.left + ((plot.width() - 7.0 * u) / 2.0).round();
    let top = plot.top + ((plot.height() - rows as f32 * u) / 2.0).round();
    let number = c.display((u / s * 0.4).clamp(8.0, 15.0), 600);
    for (i, date) in grid.cells.iter().enumerate() {
        let Some(date) = date else { continue };
        let rect = Rect::new(left + (i % 7) as f32 * u, top + (i / 7) as f32 * u, u - gap, u - gap);
        square(c, ITEM + i as u32, rect, *date, &level);
        let lv = level(count(c, *date));
        let ink = if lv >= 3 && *date <= today {
            color.on_accent
        } else if *date == today {
            color.label
        } else {
            color.secondary
        };
        c.styled(rect, &date.day.to_string(), &number, ink, Align::Center, Align::Center);
        if *date == today && lv < 3 {
            // Today without an outline: its number in the label's ink and a dot under it.
            let d = c.px(3.0);
            c.f.fill(Rect::new((rect.center().x - d / 2.0).round(), rect.bottom - d - c.px(3.0), d, d), d / 2.0, color.label);
        }
    }
}

fn streak(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let level = levels(c);
    let today = c.now.date;
    let mut day = if count(c, today) > 0 { today } else { today.add_days(-1) };
    let mut current = 0;
    while count(c, day) > 0 {
        current += 1;
        day = day.add_days(-1);
    }
    let total: u32 = c.m.counts.values().sum();
    let h = c.rect.height() / s;
    let strip_h = (h * 0.09).clamp(7.0, 16.0).round() * s;
    let (strip, top) = r.take_bottom(strip_h, c.px(10.0));
    // The last 28 days.
    let n = 28;
    let gap = c.px(2.0);
    let w = ((strip.width() + gap) / n as f32 - gap).floor();
    for i in 0..n {
        let date = today.add_days(i as i64 - (n as i64 - 1));
        let rect = Rect::new(strip.left + i as f32 * (w + gap), strip.top, w, strip.height());
        c.f.fill(rect, (strip.height() * 0.35).round(), shade(c, level(count(c, date)), false));
        c.hit(ITEM + i as u32, rect, Role::Button, &date.key());
    }
    let side = top.width() > c.px(190.0);
    let (stats, main) = if side { top.take_right((top.width() * 0.38).round(), c.px(12.0)) } else { (Rect::new(0.0, 0.0, 0.0, 0.0), top) };
    let flame = (h * 0.36).clamp(26.0, 70.0).round() * s;
    let size = (h * 0.48).clamp(34.0, 96.0);
    let style = c.display(size, 500);
    let nh = (style.size * 1.05).round();
    let (glyph, rest) = main.take_left(flame.min(main.width() * 0.3), c.px(8.0));
    c.icon(glyph.place(glyph.width(), glyph.width(), Align::Center, Align::Center), Icon::Star, glyph.width() / s * 0.9, color.accent);
    let unit = c.f.line(TextRole::Callout);
    let y = rest.top + ((rest.height() - nh - unit) / 2.0).round();
    c.styled(Rect::new(rest.left, y, rest.width(), nh), &current.to_string(), &style, color.label, Align::Start, Align::Center);
    c.text(Rect::new(rest.left, y + nh, rest.width(), unit), if current == 1 { "day in a row" } else { "days in a row" }, TextRole::Callout, color.secondary, Align::Start);
    if side {
        let best = best_streak(c);
        let lines = [("Best", best.to_string()), ("Total", total.to_string())];
        let (a, b) = (c.f.line(TextRole::Caption), c.f.line(TextRole::Title2));
        let block = 2.0 * (a + b) + c.px(8.0);
        let mut y = stats.top + ((stats.height() - block) / 2.0).round();
        for (label, value) in lines {
            c.text(Rect::new(stats.left, y, stats.width(), a), &label.to_uppercase(), TextRole::Caption, color.tertiary, Align::End);
            c.text(Rect::new(stats.left, y + a, stats.width(), b), &value, TextRole::Title2, color.label, Align::End);
            y += a + b + c.px(8.0);
        }
    }
}

fn best_streak(c: &Ctx) -> u32 {
    let mut days: Vec<Date> = c
        .m
        .counts
        .iter()
        .filter(|(_, n)| **n > 0)
        .filter_map(|(k, _)| Date::parse(k))
        .collect();
    days.sort_by_key(|d| d.ordinal());
    let (mut best, mut run, mut last) = (0, 0, None::<i64>);
    for d in days {
        let o = d.ordinal();
        run = if last == Some(o - 1) { run + 1 } else { 1 };
        best = best.max(run);
        last = Some(o);
    }
    best
}
