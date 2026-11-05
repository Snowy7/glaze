//! The calendar. By its box, as the web calendar picked its mode: a **tile** (the weekday, the
//! date large, the next event), a **week strip**, the **month** with the selected day's events
//! under it when tall, or the **agenda** of what is coming. Today is the one filled disc.
use super::{
    CANCEL, Ctx, DELETE_TASK, EDIT_TASK, EVENT_COLOR, EVENT_COLORS, EVENT_DATE, EVENT_TIME, FIELD,
    ITEM, NEXT, PREVIOUS, PRIMARY, TODAY, field,
};
use crate::widgets::model::Model;
use crate::widgets::time;
use mypaper_ui::calendar::{Date, MONTHS, MonthGrid, WEEKDAYS};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, ButtonKind, Rect, Rgba, Role, TextRole};
use serde_json::Value;

pub fn event_color(index: usize) -> Rgba {
    Rgba::rgb8(EVENT_COLORS[index % EVENT_COLORS.len()], 1.0)
}

/// Imported calendars keep their provider colour; local events use the picker palette.
fn source_color(event: &Value) -> Rgba {
    if let Some(rgb) = event["calendarColor"].as_array().filter(|v| v.len() == 3) {
        if let (Some(r), Some(g), Some(b)) = (rgb[0].as_u64(), rgb[1].as_u64(), rgb[2].as_u64()) {
            return Rgba::rgb8([r.min(255) as u8, g.min(255) as u8, b.min(255) as u8], 1.0);
        }
    }
    event_color(event["color"].as_u64().unwrap_or(0) as usize)
}

fn week_start(c: &Ctx) -> u32 {
    match c.m.str("weekStart", "auto").as_str() {
        "sunday" => 0,
        "saturday" => 6,
        "monday" => 1,
        _ => time::week_start(),
    }
}

/// Events on `date`, with their indices.
fn on(m: &Model, date: Date) -> Vec<(usize, &Value)> {
    let key = date.key();
    m.events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["date"].as_str() == Some(key.as_str()))
        .collect()
}

pub fn draw(c: &mut Ctx, r: Rect) {
    if c.m.event_draft.is_some() {
        editor(c, r);
        return;
    }
    let s = c.s();
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let variant = c.m.str("variant", "month");
    if w < 130.0 || h < 70.0 {
        return tile(c, r);
    }
    match variant.as_str() {
        "agenda" if h < 130.0 => tile(c, r),
        "agenda" => agenda(c, r),
        "strip" => strip(c, r),
        _ if h < 170.0 && w < 240.0 => tile(c, r),
        _ if h < 170.0 => strip(c, r),
        _ => month(c, r),
    }
}

/// The header: "November 2026" (the year goes first when narrow), a week badge, and add,
/// previous and next. Its height.
fn header(c: &mut Ctx, r: Rect, title_month: Date) -> f32 {
    let color = c.color();
    let s = c.s();
    let size = (c.rect.width().min(c.rect.height()) / s * 0.064).clamp(13.0, 19.0);
    let button = (size * 1.8 * s).round();
    let row = Rect::new(r.left, r.top, r.width(), button);
    let (controls, title) = row.take_right(button * 3.0, c.px(4.0));
    for (i, (id, icon, label)) in [
        (PRIMARY, Icon::Add, "Add event for selected day"),
        (PREVIOUS, Icon::ChevronLeft, "Previous"),
        (NEXT, Icon::ChevronRight, "Next"),
    ]
    .into_iter()
    .enumerate()
    {
        let at = Rect::new(controls.left + i as f32 * button, row.top, button, button);
        c.f.icon_button(id, at, icon, button / s, label, false);
    }
    let name = MONTHS[title_month.month as usize - 1];
    let strong = c.display(size, 650);
    let plain = c.display(size, 400);
    let room = title.width();
    let full = c.f.text.measure(name, &strong).0;
    let year = title_month.year.to_string();
    let year_w = c.f.text.measure(&year, &plain).0;
    let name = if full <= room { name } else { &name[..3] };
    let name_w = c.f.text.measure(name, &strong).0;
    c.f.wash(TODAY, Rect::new(title.left - c.px(6.0), title.top, name_w + c.px(12.0), title.height()), button / 2.0, true);
    c.styled(title, name, &strong, color.label, Align::Start, Align::Center);
    let gap = c.px(6.0);
    if name_w + gap + year_w <= room && c.rect.width() > c.px(170.0) {
        let at = Rect { left: title.left + name_w + gap, ..title };
        c.styled(at, &year, &plain, color.secondary, Align::Start, Align::Center);
    }
    c.hit(TODAY, Rect { right: title.left + name_w + gap, ..title }, Role::Button, "Return to today");
    // The ISO week, as a badge after the title, when asked for and there is room.
    if c.m.flag("weekNumbers", false) && c.rect.width() > c.px(250.0) {
        let week = format!("W{}", c.m.selected.iso_week());
        let x = (title.left + name_w + gap + year_w + c.px(10.0)).min(title.right - c.px(36.0));
        if x > title.left + name_w {
            c.f.badge(x, row.top + button / 2.0, &week, None);
        }
    }
    button
}

/// A day: its number on the disc (filled today, a card when selected), a dot per event colour
/// under the number. The cell is the click target; the wash is the disc.
fn day(c: &mut Ctx, cell: Rect, date: Date, size: f32, muted: bool) {
    let color = c.color();
    let s = c.s();
    let is_today = date == c.now.date;
    let d = (size * 1.95 * s).min(cell.width()).min(cell.height()).floor();
    let disc = cell.place(d, d, Align::Center, Align::Center);
    let id = ITEM + date.day;
    if is_today {
        c.f.fill(disc, d / 2.0, color.accent);
    } else if date == c.m.selected {
        c.f.fill(disc, d / 2.0, color.fill);
    }
    c.f.wash(id, disc, d / 2.0, true);
    c.f.focus_ring(id, disc, d / 2.0);
    c.f.hit(id, cell, Role::Button, &date.key(), true, Some(date == c.m.selected));
    let dots: Vec<_> = on(c.m, date)
        .iter()
        .map(|(_, e)| source_color(e))
        .take(3)
        .collect();
    let weekend = matches!(date.weekday(), 0 | 6);
    let ink = if is_today {
        color.on_accent
    } else if muted {
        color.tertiary
    } else if weekend {
        color.secondary
    } else {
        color.label
    };
    let style = c.display(size, if is_today { 700 } else { 500 });
    let lift = if dots.is_empty() { 0.0 } else { (size * 0.1 * s).round() };
    c.styled(disc.offset(0.0, -lift), &date.day.to_string(), &style, ink, Align::Center, Align::Center);
    let dot = (size * 0.28 * s).max(c.px(3.0)).round();
    let step = dot + (size * 0.2 * s).round();
    let y = (disc.bottom - dot - (d * 0.14)).round();
    for (i, tint) in dots.iter().enumerate() {
        let x = disc.center().x + (i as f32 - (dots.len() as f32 - 1.0) / 2.0) * step - dot / 2.0;
        let ink = if is_today { color.on_accent } else { *tint };
        c.f.fill(Rect::new(x.round(), y, dot, dot), dot / 2.0, ink);
    }
}

/// Weekday initials (or short names when wide) along a row.
fn weekday_name(d: u32, wide: bool) -> String {
    let name = WEEKDAYS[d as usize % 7];
    if wide {
        name.to_uppercase()
    } else {
        name[..1].to_owned()
    }
}

fn month(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let head = header(c, r, c.m.month);
    let (_, rest) = r.take_top(head, c.px(6.0));
    let start = week_start(c);
    // The selected day's events under the grid when the widget is tall.
    let events = on(c.m, c.m.selected);
    let detail = if c.rect.height() >= c.px(340.0) {
        c.px(112.0)
    } else {
        0.0
    };
    let (grid, list) = if detail > 0.0 {
        let (list, grid) = rest.take_bottom(detail, c.px(8.0));
        (grid, Some(list))
    } else {
        (rest, None)
    };
    let weeks = c.m.flag("weekNumbers", false);
    let cols = if weeks { 8 } else { 7 };
    let month = MonthGrid::new(c.m.month, start);
    let rows = month.cells.len() / 7;
    let col_w = grid.width() / cols as f32;
    let row_h = grid.height() / (rows as f32 + 0.75);
    let size = (col_w * 0.36).min(row_h * 0.5) / s;
    let size = size.clamp(10.0, 19.0);
    let wide = col_w / s > 34.0;
    let head_h = (row_h * 0.75).round();
    let left = grid.left + if weeks { col_w } else { 0.0 };
    let caps = c.display((size * 0.62).max(9.0), 600);
    for col in 0..7u32 {
        let at = Rect::new(left + col as f32 * col_w, grid.top, col_w, head_h);
        c.styled(at, &weekday_name(col + start, wide), &caps, color.tertiary, Align::Center, Align::Center);
    }
    for (index, date) in month.cells.iter().enumerate() {
        let row = index / 7;
        let top = grid.top + head_h + row as f32 * row_h;
        if weeks && index % 7 == 0 {
            let first = month.cells[index..index + 7].iter().flatten().next();
            if let Some(first) = first {
                let at = Rect::new(grid.left, top, col_w, row_h);
                let style = c.display((size * 0.62).max(9.0), 500);
                c.styled(at, &first.iso_week().to_string(), &style, color.tertiary, Align::Center, Align::Center);
            }
        }
        let Some(date) = date else { continue };
        let cell = Rect::new(left + (index % 7) as f32 * col_w, top, col_w, row_h);
        day(c, cell, *date, size, false);
    }
    if let Some(list) = list {
        day_events(c, list, &events);
    }
}

/// A week: the weekday over each day's number; the selected day's events under it when tall.
fn strip(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let show_header = c.rect.height() > c.px(100.0);
    let rest = if show_header {
        let head = header(c, r, c.m.selected);
        r.take_top(head, c.px(4.0)).1
    } else {
        r
    };
    let start = week_start(c);
    let first = c
        .m
        .selected
        .add_days(-i64::from((c.m.selected.weekday() + 7 - start) % 7));
    let col_w = rest.width() / 7.0;
    let size = (col_w * 0.4 / s).clamp(14.0, 30.0);
    let caps = c.display((size * 0.42).max(9.0), 600);
    let cap_h = (caps.size * 1.5).round();
    let row_h = (size * 2.1 * s).round().min(rest.height() - cap_h);
    let detail = c.rect.height() >= c.px(160.0) && rest.height() - cap_h - row_h > c.px(30.0);
    let top = if detail {
        rest.top
    } else {
        rest.top + ((rest.height() - cap_h - row_h) / 2.0).max(0.0).round()
    };
    for i in 0..7 {
        let date = first.add_days(i);
        let x = rest.left + i as f32 * col_w;
        c.styled(
            Rect::new(x, top, col_w, cap_h),
            &weekday_name(date.weekday(), col_w / s > 30.0),
            &caps,
            color.tertiary,
            Align::Center,
            Align::Center,
        );
        day(c, Rect::new(x, top + cap_h, col_w, row_h), date, size, date.month != c.m.selected.month);
    }
    if detail {
        let list = Rect {
            top: top + cap_h + row_h + c.px(8.0),
            ..rest
        };
        let events = on(c.m, c.m.selected);
        day_events(c, list, &events);
    }
}

/// The selected day's events (or a quiet line), scrolling when there are many.
fn day_events(c: &mut Ctx, r: Rect, events: &[(usize, &Value)]) {
    let color = c.color();
    if events.is_empty() {
        c.text(
            Rect::new(r.left, r.top, r.width(), c.f.line(TextRole::Callout)),
            "Nothing planned",
            TextRole::Callout,
            color.tertiary,
            Align::Start,
        );
        return;
    }
    let row = c.px(30.0);
    c.scroll(r, events.len() as f32 * row, |c, top| {
        for (i, (index, event)) in events.iter().enumerate() {
            event_row(c, Rect::new(r.left, top + i as f32 * row, r.width(), row), *index, event, None);
        }
    });
}

/// An event: its colour dot, time, title, and a delete button that shows on hover.
fn event_row(c: &mut Ctx, row: Rect, index: usize, event: &Value, date: Option<&str>) {
    let color = c.color();
    let edit = EDIT_TASK + index as u32;
    let delete = DELETE_TASK + index as u32;
    let radius = c.px(8.0);
    let hovered = matches!(c.m.hover, Some(h) if h == edit || h == delete);
    let (del, main) = row.take_right(row.height(), 0.0);
    c.control(edit, main, radius, Role::Button, "Edit event", None);
    let inner = main.inset(mypaper_ui::Insets::xy(c.px(6.0), 0.0));
    let (dot, rest) = inner.take_left(c.px(7.0), c.px(8.0));
    let tint = source_color(event);
    let d = c.px(7.0);
    c.f.fill(dot.place(d, d, Align::Center, Align::Center), d / 2.0, tint);
    let time = event["time"].as_str().filter(|t| !t.is_empty());
    let when = match (date, time) {
        (Some(date), Some(time)) => format!("{date} {time}"),
        (Some(date), None) => date.to_owned(),
        (None, Some(time)) => time.to_owned(),
        (None, None) => "All day".to_owned(),
    };
    let when_w = c.measure(&when, TextRole::Footnote).0.max(c.px(38.0));
    let (when_box, title) = rest.take_left(when_w, c.px(8.0));
    c.text(when_box, &when, TextRole::Footnote, if time.is_some() { color.secondary } else { color.tertiary }, Align::Start);
    c.text(title, event["title"].as_str().unwrap_or("Event"), TextRole::Callout, color.label, Align::Start);
    // The delete button is always in the list; its glyph shows only while the row is hovered.
    let d = row.height() - c.px(6.0);
    let at = del.place(d, d, Align::Center, Align::Center);
    c.control(delete, at, d / 2.0, Role::Button, "Delete event", None);
    c.icon(at, Icon::Close, 12.0, if hovered { color.secondary } else { Rgba::TRANSPARENT });
}

/// Upcoming events grouped by day, under today's weekday and date and an add button.
fn agenda(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let now = c.now;
    let size = (c.rect.width().min(c.rect.height()) / s * 0.064).clamp(13.0, 19.0);
    let big = c.display(size * 1.3, 650);
    let small = c.display(size * 0.82, 400);
    let (a, b) = ((big.size * 1.2).round(), (small.size * 1.35).round());
    let (head, rest) = r.take_top(a + b, c.px(10.0));
    let add = (size * 2.0 * s).round();
    let (button, title) = head.take_right(add, c.px(8.0));
    c.f.icon_button(PRIMARY, button.place(add, add, Align::Center, Align::Center), Icon::Add, add / s, "Add event", true);
    c.styled(Rect::new(title.left, title.top, title.width(), a), super::clock::long_weekday(now.date), &big, color.label, Align::Start, Align::Center);
    let date = format!("{} {}", now.date.day, MONTHS[now.date.month as usize - 1]);
    c.styled(Rect::new(title.left, title.top + a, title.width(), b), &date, &small, color.secondary, Align::Start, Align::Center);
    c.hit(TODAY, title, Role::Button, "Return to today");
    let today = now.date.key();
    let mut events: Vec<_> = c
        .m
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["date"].as_str().is_some_and(|d| d >= today.as_str()))
        .collect();
    events.sort_by_key(|(_, e)| format!("{} {}", e["date"], e["time"]));
    if events.is_empty() {
        c.empty(rest, Some(Icon::Calendar), "Nothing planned", Some("Add an event with the plus."));
        return;
    }
    let row = c.px(30.0);
    let heading = c.f.line(TextRole::Caption) + c.px(8.0);
    let mut height = 0.0;
    let mut last = "";
    for (_, e) in &events {
        let d = e["date"].as_str().unwrap_or("");
        if d != last {
            height += heading;
            last = d;
        }
        height += row;
    }
    c.scroll(rest, height, |c, top| {
        let mut y = top;
        let mut last = "";
        for (index, e) in &events {
            let d = e["date"].as_str().unwrap_or("");
            if d != last {
                let label = match Date::parse(d).map(|d| d.ordinal() - now.date.ordinal()) {
                    Some(0) => "TODAY".to_owned(),
                    Some(1) => "TOMORROW".to_owned(),
                    _ => Date::parse(d)
                        .map(|d| time::date_label(d, false).to_uppercase())
                        .unwrap_or_default(),
                };
                c.text(
                    Rect::new(rest.left + c.px(6.0), y + c.px(4.0), rest.width(), heading - c.px(4.0)),
                    &label,
                    TextRole::Caption,
                    color.tertiary,
                    Align::Start,
                );
                y += heading;
                last = d;
            }
            event_row(c, Rect::new(rest.left, y, rest.width(), row), *index, e, None);
            y += row;
        }
    });
}

/// The small calendar: the weekday in the accent over today's date, large; the month and the
/// next event under it (or beside it in a wide tile).
fn tile(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let now = c.now;
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let wide = w >= h * 1.5;
    let weekday = if w <= 170.0 {
        WEEKDAYS[now.date.weekday() as usize].to_uppercase()
    } else {
        super::clock::long_weekday(now.date).to_uppercase()
    };
    let size = if wide {
        (h * 0.7).min(w * 0.24).clamp(40.0, 80.0)
    } else {
        (h * 0.46).min(w * 0.4).clamp(34.0, 72.0)
    }
    // The numeral's line always fits the body (short wide tiles on a fine grid).
    .min(r.height() / s * 0.78)
    .max(20.0);
    let number = c.display(size, 500);
    let number = mypaper_ui::TextStyle {
        tracking: -0.04 * number.size,
        ..number
    };
    let caps = c.f.font(TextRole::Subheadline);
    let next = c
        .m
        .events
        .iter()
        .filter(|e| e["date"].as_str().is_some_and(|d| d >= now.date.key().as_str()))
        .min_by_key(|e| format!("{} {}", e["date"], e["time"]));
    c.hit(TODAY, r, Role::Button, "Return to today");
    let month = MONTHS[now.date.month as usize - 1];
    let day = now.date.day.to_string();
    if wide {
        let num_w = c.f.text.measure(&day, &number).0;
        let (num_box, words) = r.take_left(num_w + 2.0, c.px(14.0));
        c.styled(num_box, &day, &number, color.label, Align::Start, Align::Center);
        let lines = [c.f.line(TextRole::Subheadline), c.f.line(TextRole::Headline), c.f.line(TextRole::Footnote)];
        let shown = if next.is_some() && h >= 80.0 { 3 } else { 2 };
        let total: f32 = lines[..shown].iter().sum();
        let mut y = words.top + ((words.height() - total) / 2.0).round();
        c.styled(Rect::new(words.left, y, words.width(), lines[0]), &weekday, &caps, color.accent, Align::Start, Align::Center);
        y += lines[0];
        c.text(Rect::new(words.left, y, words.width(), lines[1]), month, TextRole::Headline, color.secondary, Align::Start);
        y += lines[1];
        if shown == 3 && let Some(e) = next {
            next_event(c, Rect::new(words.left, y, words.width(), lines[2]), e);
        }
        return;
    }
    let cap_h = c.f.line(TextRole::Subheadline);
    let num_h = (number.size * 1.05).round();
    let month_h = c.f.line(TextRole::Headline);
    c.styled(Rect::new(r.left, r.top, r.width(), cap_h), &weekday, &caps, color.accent, Align::Start, Align::Center);
    c.styled(Rect::new(r.left, r.top + cap_h, r.width(), num_h), &day, &number, color.label, Align::Start, Align::Center);
    let below = r.top + cap_h + num_h;
    if below + month_h <= r.bottom + c.px(2.0) {
        c.text(Rect::new(r.left, below, r.width(), month_h), month, TextRole::Headline, color.secondary, Align::Start);
    }
    let foot = c.f.line(TextRole::Footnote);
    if let Some(e) = next
        && below + month_h + foot <= r.bottom
    {
        next_event(c, Rect::new(r.left, r.bottom - foot, r.width(), foot), e);
    }
}

/// "● 09:30 Dentist" in a line.
fn next_event(c: &mut Ctx, r: Rect, e: &Value) {
    let color = c.color();
    let d = c.px(7.0);
    let (dot, rest) = r.take_left(d, c.px(6.0));
    c.f.fill(dot.place(d, d, Align::Center, Align::Center), d / 2.0, source_color(e));
    let label = match e["time"].as_str().filter(|t| !t.is_empty()) {
        Some(t) => format!("{t} {}", e["title"].as_str().unwrap_or("Event")),
        None => e["title"].as_str().unwrap_or("Event").to_owned(),
    };
    c.text(rest, &label, TextRole::Footnote, color.secondary, Align::Start);
}

/// Adding or editing an event: the title field, the date and time, a colour, Save and Cancel.
fn editor(c: &mut Ctx, r: Rect) {
    let Some(draft) = &c.m.event_draft else { return };
    let color = c.color();
    let control = c.px(c.f.theme.metrics.control);
    let gap = c.px(8.0);
    let (title, rest) = r.take_top(c.f.line(TextRole::Headline), gap);
    c.text(title, "Event", TextRole::Headline, color.label, Align::Start);
    let (hint_box, _) = title.take_right(title.width() * 0.6, 0.0);
    c.text(hint_box, "Enter saves · Esc cancels", TextRole::Footnote, color.tertiary, Align::End);
    let (row, rest) = rest.take_top(control, gap);
    let part = match draft.part {
        crate::widgets::model::EventPart::Title => "Event title",
        crate::widgets::model::EventPart::Date => "Date (YYYY-MM-DD)",
        crate::widgets::model::EventPart::Time => "Time (HH:MM), empty for all day",
    };
    field(c, row, part, Some(Icon::Edit));
    let (row, rest) = rest.take_top(control, gap);
    let (date, time_box) = row.take_left(row.width() * 0.58, gap);
    c.f.button(EVENT_DATE, date, &draft.date, Some(Icon::Calendar), ButtonKind::Gray, true);
    let time_label = if draft.time.is_empty() { "All day" } else { draft.time.as_str() };
    c.f.button(EVENT_TIME, time_box, time_label, Some(Icon::Clock), ButtonKind::Gray, true);
    let (row, rest) = rest.take_top(control, gap);
    let title_label = format!("Title: {}", draft.title);
    c.f.button(FIELD, row, &title_label, None, ButtonKind::Plain, true);
    let (swatches, rest) = rest.take_top(c.px(24.0), gap);
    let d = c.px(22.0);
    for i in 0..EVENT_COLORS.len() {
        let at = Rect::new(swatches.left + i as f32 * (d + c.px(8.0)), swatches.top, d, d);
        let id = EVENT_COLOR + i as u32;
        c.f.fill(at, d / 2.0, event_color(i));
        c.f.wash(id, at, d / 2.0, true);
        c.f.focus_ring(id, at, d / 2.0);
        if draft.color == i {
            c.icon(at, Icon::Check, 13.0, Rgba::rgb8([255; 3], 1.0));
        }
        c.f.hit(id, at, Role::RadioButton, &format!("Event colour {}", i + 1), true, Some(draft.color == i));
    }
    let (buttons, _) = rest.take_bottom(control, 0.0);
    let (save, cancel) = buttons.take_left((buttons.width() - gap) / 2.0, gap);
    c.f.button(PRIMARY, save, "Save", None, ButtonKind::Filled, true);
    c.f.button(CANCEL, cancel, "Cancel", None, ButtonKind::Gray, true);
}
