//! All native widget views. Time and data are explicit inputs; views never install a timer or
//! fetch anything. Faces are layouts of the same widget, with a single media implementation.
use super::{
    model::{Model, Widget},
    time::Stamp,
};
use mypaper_ui::calendar::{Date, MONTHS, MonthGrid, WEEKDAYS};
use mypaper_ui::{
    DisplayList, Hit, HitId, Image, Op, Point, Rect, Rgba, Role, Text, TextStyle, Tokens,
};
use serde_json::Value;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

pub const BODY: HitId = 1;
pub const PRIMARY: HitId = 2;
pub const PREVIOUS: HitId = 3;
pub const NEXT: HitId = 4;
pub const RESET: HitId = 5;
pub const FIELD: HitId = 6;
pub const REMOVE: HitId = 7;
pub const DUPLICATE: HitId = 8;
pub const RESIZE: HitId = 9;
pub const ITEM: HitId = 100;
pub const DELETE_TASK: HitId = 1000;
pub const EDIT_TASK: HitId = 2000;
pub const MODE: HitId = 3000;
pub const NOTE_TASK: HitId = 4000;

/// A complete widget content frame in device pixels. Every interior pixel stays hit-testable,
/// even when the engine supplies the visible glass fill.
pub struct Frame {
    pub size: (u32, u32),
    pub scale: f32,
    pub now: Stamp,
    pub instant: Instant,
    pub editing: bool,
}

pub fn view(model: &Model, tokens: &Tokens, frame: Frame, text: &mut Text) -> DisplayList {
    let Frame {
        size,
        scale,
        now,
        instant,
        editing,
    } = frame;
    let rect = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let mut widget_tokens = tokens.clone();
    let font = model.str("font", "");
    if model.kind == Widget::Clock && !font.trim().is_empty() {
        widget_tokens.font.family = font.into();
    }
    let mut c = Draw {
        list: DisplayList::default(),
        text,
        tokens: &widget_tokens,
        scale,
        rect,
        pad: 16.0 * scale,
    };
    let radius = model
        .placed
        .appearance
        .get("radius")
        .and_then(Value::as_f64)
        .unwrap_or(22.0) as f32
        * scale;
    c.fill(rect, radius, Rgba::HIT);
    let surface = model
        .placed
        .appearance
        .get("surface")
        .and_then(Value::as_str)
        .unwrap_or("glass");
    if matches!(surface, "solid" | "tint") {
        c.fill(rect, radius, tokens.material.tint);
    }
    c.hit(BODY, rect, Role::Pane, &model.placed.widget);
    match model.kind {
        Widget::Clock => clock(&mut c, model, now),
        Widget::Calendar => calendar(&mut c, model, now),
        Widget::Media => media(&mut c, model, now),
        Widget::QuickToggles => quick(&mut c, model),
        Widget::Launcher => launcher(&mut c, model),
        Widget::Weather => weather(&mut c, model),
        Widget::System => system(&mut c, model),
        Widget::Timer => timer(&mut c, model, now),
        Widget::Countdown => countdown(&mut c, model, now),
        Widget::Photo => photo(&mut c, model, instant),
        Widget::Notes => notes(&mut c, model),
        Widget::Todo => todo(&mut c, model),
        Widget::Rss => rss(&mut c, model, now),
        Widget::Crypto => crypto(&mut c, model),
        Widget::Heatmap => heatmap(&mut c, model, now),
        Widget::Quote => quote(&mut c, model, now),
    }
    if model
        .placed
        .appearance
        .get("border")
        .and_then(Value::as_bool)
        .unwrap_or(true)
        || editing
    {
        c.stroke(
            rect.inflate(-0.5),
            radius,
            if editing {
                Rgba::rgb8([125, 192, 255], 0.95)
            } else {
                tokens.colors.line
            },
            if editing { 2.0 } else { 1.0 },
        );
    }
    if editing {
        let s = c.scale;
        c.button(
            DUPLICATE,
            Rect::new(rect.width() - 80.0 * s, 8.0 * s, 32.0 * s, 26.0 * s),
            "+",
            "Duplicate widget",
        );
        c.button(
            REMOVE,
            Rect::new(rect.width() - 42.0 * s, 8.0 * s, 32.0 * s, 26.0 * s),
            "×",
            "Remove widget",
        );
        let handle = Rect::new(
            rect.width() - 24.0 * s,
            rect.height() - 24.0 * s,
            24.0 * s,
            24.0 * s,
        );
        c.line(
            Point::new(handle.left + 6.0 * s, handle.bottom - 5.0 * s),
            Point::new(handle.right - 5.0 * s, handle.top + 6.0 * s),
            tokens.colors.text,
            2.0,
        );
        c.hit(RESIZE, handle, Role::Button, "Resize widget");
    }
    c.list
}

struct Draw<'a> {
    list: DisplayList,
    text: &'a mut Text,
    tokens: &'a Tokens,
    scale: f32,
    rect: Rect,
    pad: f32,
}
impl Draw<'_> {
    fn body(&self) -> Rect {
        self.rect.inflate(-self.pad)
    }
    fn fill(&mut self, rect: Rect, radius: f32, color: Rgba) {
        self.list.ops.push(Op::Fill {
            rect,
            radius,
            color,
        });
    }
    fn stroke(&mut self, rect: Rect, radius: f32, color: Rgba, width: f32) {
        self.list.ops.push(Op::Stroke {
            rect,
            radius,
            width: width * self.scale,
            color,
        });
    }
    fn line(&mut self, from: Point, to: Point, color: Rgba, width: f32) {
        self.list.ops.push(Op::Line {
            from,
            to,
            width: width * self.scale,
            color,
        });
    }
    fn hit(&mut self, id: HitId, rect: Rect, role: Role, label: &str) {
        self.list.hits.push(Hit {
            id,
            rect,
            role,
            label: label.into(),
            enabled: true,
            checked: None,
        });
    }
    fn label(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = TextStyle {
            size: size * self.scale,
            weight,
            ..self.tokens.font.clone()
        };
        let mut value = value.to_owned();
        if self.text.measure(&value, &style).0 > rect.width() {
            while !value.is_empty()
                && self.text.measure(&format!("{value}…"), &style).0 > rect.width()
            {
                value.pop();
            }
            value.push('…');
        }
        self.list.ops.push(Op::Text {
            rect,
            text: value.into(),
            style,
            color,
        });
    }
    fn centered(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = TextStyle {
            size: size * self.scale,
            weight,
            ..self.tokens.font.clone()
        };
        let (w, h) = self.text.measure(value, &style);
        let rect = Rect::new(
            rect.left + ((rect.width() - w) / 2.0).max(0.0),
            rect.top + ((rect.height() - h) / 2.0).max(0.0),
            rect.width(),
            h,
        );
        self.label(rect, value, size, color, weight);
    }
    fn wrapped(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = TextStyle {
            size: size * self.scale,
            weight,
            ..self.tokens.font.clone()
        };
        let height = size * self.scale * 1.45;
        let mut line = String::new();
        let mut y = rect.top;
        for ch in value.chars() {
            let candidate = format!("{line}{ch}");
            if ch == '\n' || self.text.measure(&candidate, &style).0 > rect.width() {
                if y + height > rect.bottom {
                    break;
                }
                self.label(
                    Rect::new(rect.left, y, rect.width(), height),
                    &line,
                    size,
                    color,
                    weight,
                );
                y += height;
                line.clear();
                if ch != '\n' {
                    line.push(ch);
                }
            } else {
                line.push(ch);
            }
        }
        if !line.is_empty() && y + height <= rect.bottom {
            self.label(
                Rect::new(rect.left, y, rect.width(), height),
                &line,
                size,
                color,
                weight,
            );
        }
    }
    fn title(&mut self, title: &str) {
        let r = self.body();
        self.label(
            Rect::new(r.left, r.top, r.width(), 22.0 * self.scale),
            title,
            12.0,
            self.tokens.colors.text_dim,
            600,
        );
    }
    fn button(&mut self, id: HitId, rect: Rect, glyph: &str, label: &str) {
        self.fill(rect, 8.0 * self.scale, self.tokens.colors.hover);
        self.centered(rect, glyph, 16.0, self.tokens.colors.text, 500);
        self.hit(id, rect, Role::Button, label);
    }
    fn empty(&mut self, title: &str, hint: &str) {
        let r = self.body();
        let cy = r.top + r.height() * 0.38;
        self.centered(
            Rect::new(r.left, cy, r.width(), 28.0 * self.scale),
            title,
            16.0,
            self.tokens.colors.text,
            500,
        );
        self.wrapped(
            Rect::new(
                r.left,
                cy + 32.0 * self.scale,
                r.width(),
                r.bottom - cy - 32.0 * self.scale,
            ),
            hint,
            11.0,
            self.tokens.colors.text_dim,
            400,
        );
    }
    fn ring(&mut self, center: Point, radius: f32, progress: f32, color: Rgba, width: f32) {
        self.list.ops.push(Op::Arc {
            center,
            radius,
            sweep: TAU,
            width: width * self.scale,
            color: self.tokens.colors.line,
        });
        if progress > 0.0 {
            self.list.ops.push(Op::Arc {
                center,
                radius,
                sweep: progress.clamp(0.0, 1.0) * TAU,
                width: width * self.scale,
                color,
            });
        }
    }
    fn image(&mut self, rect: Rect, image: &Arc<Image>, opacity: f32, contain: bool) {
        let ratio = if contain {
            (rect.width() / image.width as f32).min(rect.height() / image.height as f32)
        } else {
            (rect.width() / image.width as f32).max(rect.height() / image.height as f32)
        };
        // Fit keeps the full image; cover takes a centred source crop without stretching it.
        let (w, h) = (image.width as f32 * ratio, image.height as f32 * ratio);
        let rect = if contain {
            Rect::new(
                rect.left + (rect.width() - w) / 2.0,
                rect.top + (rect.height() - h) / 2.0,
                w,
                h,
            )
        } else {
            rect
        };
        let source = (!contain).then(|| {
            let width = rect.width() / ratio;
            let height = rect.height() / ratio;
            Rect::new(
                (image.width as f32 - width) / 2.0,
                (image.height as f32 - height) / 2.0,
                width,
                height,
            )
        });
        self.list.ops.push(Op::Image {
            rect,
            image: image.clone(),
            opacity,
            source,
        });
    }
    fn field(&mut self, m: &Model, rect: Rect, hint: &str) {
        self.fill(
            rect,
            7.0 * self.scale,
            self.tokens.colors.hover.with_alpha(0.10),
        );
        if let Some(field) = &m.field {
            field.set_rect(rect);
            self.list.ops.push(Op::TextField {
                rect,
                field: field.clone(),
                revision: field.revision(),
            });
        } else {
            self.label(
                rect.inflate(-6.0 * self.scale),
                hint,
                13.0,
                self.tokens.colors.text_dim,
                400,
            );
        }
        self.hit(FIELD, rect, Role::TextInput, hint);
    }
}

fn clock(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "desk");
    let color = c.tokens.colors.text;
    match variant.as_str() {
        "analog" => {
            let radius = (r.width().min(r.height()) * 0.43).max(12.0);
            let center = Point::new(r.left + r.width() / 2.0, r.top + r.height() / 2.0);
            c.ring(center, radius, 0.0, color, 1.0);
            for i in 0..12 {
                let angle = i as f32 * TAU / 12.0 - TAU / 4.0;
                c.line(
                    Point::new(
                        center.x + (radius - 7.0 * s) * angle.cos(),
                        center.y + (radius - 7.0 * s) * angle.sin(),
                    ),
                    Point::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    ),
                    color,
                    1.5,
                );
            }
            for (part, length, width) in [
                (
                    (now.hour % 12) as f32 / 12.0 + now.minute as f32 / 720.0,
                    0.52,
                    3.5,
                ),
                (now.minute as f32 / 60.0, 0.78, 2.0),
            ] {
                let angle = part * TAU - TAU / 4.0;
                c.line(
                    center,
                    Point::new(
                        center.x + radius * length * angle.cos(),
                        center.y + radius * length * angle.sin(),
                    ),
                    color,
                    width,
                );
            }
            if m.flag("showSeconds", false) {
                let a = now.second as f32 * TAU / 60.0 - TAU / 4.0;
                c.line(
                    center,
                    Point::new(
                        center.x + radius * 0.83 * a.cos(),
                        center.y + radius * 0.83 * a.sin(),
                    ),
                    Rgba::rgb8([255, 155, 120], 1.0),
                    1.0,
                );
            }
        }
        "world" => {
            c.title("WORLD CLOCK");
            let zones = m.list("zones");
            let h = (r.height() - 24.0 * s) / zones.len().max(1) as f32;
            for (i, z) in zones.iter().enumerate() {
                let y = r.top + 24.0 * s + i as f32 * h;
                let tz = z["tz"].as_str().unwrap_or("UTC");
                let time = now.zone(tz);
                c.label(
                    Rect::new(r.left, y, r.width() * 0.52, h),
                    z["label"].as_str().unwrap_or(tz),
                    13.0,
                    color,
                    500,
                );
                c.label(
                    Rect::new(r.left + r.width() * 0.54, y, r.width() * 0.46, h),
                    &time.map_or_else(|| "—".into(), |t| t.clock(m.flag("format24", true), false)),
                    18.0,
                    color,
                    500,
                );
            }
        }
        "words" => {
            let greeting = match now.hour {
                0..=4 => "Good night",
                5..=11 => "Good morning",
                12..=17 => "Good afternoon",
                _ => "Good evening",
            };
            c.wrapped(
                r,
                &format!("{greeting}{}", {
                    let name = m.str("name", "");
                    if name.is_empty() {
                        mypaper_widgets::userinfo::cached()
                            .map(|u| format!(", {}", u.name))
                            .unwrap_or_default()
                    } else {
                        format!(", {name}")
                    }
                }),
                26.0,
                color,
                500,
            );
        }
        _ => {
            let value = now.clock(m.flag("format24", true), m.flag("showSeconds", false));
            let size = (r.width() / s / (value.len() as f32 * 0.62))
                .min(r.height() / s * 0.52)
                .clamp(16.0, 76.0);
            let box_r = Rect::new(
                r.left,
                r.top + r.height() * 0.10,
                r.width(),
                r.height() * 0.63,
            );
            if variant == "flip" {
                let n = value.chars().count() as f32;
                let w = r.width() / n;
                for (i, ch) in value.chars().enumerate() {
                    let rect = Rect::new(
                        r.left + i as f32 * w + 2.0 * s,
                        box_r.top,
                        w - 4.0 * s,
                        box_r.height(),
                    );
                    if ch.is_ascii_digit() {
                        c.fill(rect, 8.0 * s, Rgba::new(0.1, 0.12, 0.15, 0.4));
                        c.line(
                            Point::new(rect.left, rect.top + rect.height() / 2.0),
                            Point::new(rect.right, rect.top + rect.height() / 2.0),
                            c.tokens.colors.line,
                            1.0,
                        );
                    }
                    c.centered(rect, &ch.to_string(), size, color, 500);
                }
            } else {
                c.centered(box_r, &value, size, color, 400);
            }
            if m.flag("showDate", true) {
                c.centered(
                    Rect::new(r.left, r.top + r.height() * 0.76, r.width(), 24.0 * s),
                    &format!(
                        "{}, {} {}",
                        WEEKDAYS[now.date.weekday() as usize],
                        MONTHS[now.date.month as usize - 1],
                        now.date.day
                    ),
                    12.0,
                    c.tokens.colors.text_dim,
                    500,
                );
            }
        }
    }
}
fn calendar(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "month");
    c.label(
        Rect::new(r.left, r.top, r.width() - 70.0 * s, 28.0 * s),
        &format!("{} {}", MONTHS[m.month.month as usize - 1], m.month.year),
        16.0,
        c.tokens.colors.text,
        600,
    );
    c.button(
        PREVIOUS,
        Rect::new(r.right - 60.0 * s, r.top, 26.0 * s, 24.0 * s),
        "‹",
        "Previous month",
    );
    c.button(
        NEXT,
        Rect::new(r.right - 28.0 * s, r.top, 26.0 * s, 24.0 * s),
        "›",
        "Next month",
    );
    let footer = Rect::new(r.left, r.bottom - 30.0 * s, r.width(), 30.0 * s);
    let grid = Rect::new(
        r.left,
        r.top + 32.0 * s,
        r.width(),
        (footer.top - r.top - 38.0 * s).max(24.0 * s),
    );
    if variant == "agenda" {
        let mut events: Vec<_> = m
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e["date"]
                    .as_str()
                    .is_some_and(|date| date >= now.date.key().as_str())
            })
            .collect();
        events.sort_by_key(|(_, e)| format!("{} {}", e["date"], e["time"]));
        if events.is_empty() {
            c.centered(
                grid,
                "No upcoming events",
                14.0,
                c.tokens.colors.text_dim,
                400,
            );
        }
        for (row, (index, e)) in events
            .iter()
            .take((grid.height() / (44.0 * s)) as usize)
            .enumerate()
        {
            let y = grid.top + row as f32 * 44.0 * s;
            let rect = Rect::new(r.left, y, r.width() - 28.0 * s, 40.0 * s);
            c.label(
                Rect::new(rect.left, y, rect.width(), 20.0 * s),
                e["title"].as_str().unwrap_or("Event"),
                13.0,
                c.tokens.colors.text,
                500,
            );
            c.label(
                Rect::new(rect.left, y + 21.0 * s, rect.width(), 18.0 * s),
                &format!(
                    "{}  {}",
                    e["date"].as_str().unwrap_or(""),
                    e["time"].as_str().unwrap_or("")
                ),
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.hit(EDIT_TASK + *index as u32, rect, Role::Button, "Edit event");
            c.button(
                DELETE_TASK + *index as u32,
                Rect::new(r.right - 24.0 * s, y, 24.0 * s, 24.0 * s),
                "×",
                "Delete event",
            );
        }
    } else if variant == "strip" || grid.height() < 100.0 * s {
        let first = m.selected.add_days(-i64::from(m.selected.weekday()));
        for i in 0..7 {
            let d = first.add_days(i);
            let cell = Rect::new(
                grid.left + i as f32 * grid.width() / 7.0,
                grid.top,
                grid.width() / 7.0,
                grid.height(),
            );
            if d == m.selected {
                c.fill(cell.inflate(-2.0 * s), 10.0 * s, c.tokens.colors.hover);
            }
            c.centered(
                Rect::new(cell.left, cell.top, cell.width(), 22.0 * s),
                WEEKDAYS[d.weekday() as usize],
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.centered(
                Rect::new(
                    cell.left,
                    cell.top + 22.0 * s,
                    cell.width(),
                    (cell.height() - 22.0 * s).max(20.0 * s),
                ),
                &d.day.to_string(),
                20.0,
                c.tokens.colors.text,
                500,
            );
            c.hit(ITEM + d.day, cell, Role::Button, &d.key());
        }
    } else {
        let start = match m.str("weekStart", "auto").as_str() {
            "sunday" => 0,
            "saturday" => 6,
            "monday" => 1,
            _ => super::time::week_start(),
        };
        let month = MonthGrid::new(m.month, start);
        let show_weeks = m.flag("weekNumbers", false);
        let inset = if show_weeks { 20.0 * s } else { 0.0 };
        let grid_rect = Rect::new(
            grid.left + inset,
            grid.top,
            grid.width() - inset,
            grid.height(),
        );
        if show_weeks {
            let h = grid.height() / (month.cells.len() / 7 + 1) as f32;
            for (row, cells) in month.cells.chunks(7).enumerate() {
                if let Some(day) = cells.iter().flatten().next() {
                    c.centered(
                        Rect::new(grid.left, grid.top + (row + 1) as f32 * h, inset, h),
                        &day.iso_week().to_string(),
                        9.0,
                        c.tokens.colors.text_dim,
                        400,
                    );
                }
            }
        }
        month.view(
            &mut c.list,
            grid_rect,
            now.date,
            c.tokens.colors.text,
            c.tokens.colors.hover,
            ITEM,
            &c.tokens.font.scaled(s * 0.86),
        );
        if let Some(hit) = c.list.hits.iter().find(|h| {
            h.id == ITEM + m.selected.day
                && m.selected.month == m.month.month
                && m.selected.year == m.month.year
        }) {
            let rect = hit.rect;
            c.stroke(
                rect.inflate(-2.0 * s),
                6.0 * s,
                c.tokens.colors.text_dim,
                1.0,
            );
        }
        for e in &m.events {
            if let Some(date) = e["date"].as_str().and_then(Date::parse)
                && let Some(hit) = c.list.hits.iter().find(|h| {
                    h.id == ITEM + date.day
                        && date.month == m.month.month
                        && date.year == m.month.year
                })
            {
                let r = hit.rect;
                c.fill(
                    Rect::new(
                        r.left + r.width() / 2.0 - 2.0 * s,
                        r.bottom - 5.0 * s,
                        4.0 * s,
                        4.0 * s,
                    ),
                    2.0 * s,
                    c.tokens.colors.text,
                );
            }
        }
    }
    if m.field.is_some() {
        c.field(m, footer, "Event title · Enter to save");
    } else {
        let title = m
            .events
            .iter()
            .find(|e| e["date"].as_str() == Some(m.selected.key().as_str()))
            .and_then(|e| e["title"].as_str())
            .unwrap_or("Add event");
        c.button(
            PRIMARY,
            Rect::new(footer.right - 28.0 * s, footer.top, 28.0 * s, 28.0 * s),
            "+",
            "Add event for selected day",
        );
        c.label(
            Rect::new(
                footer.left,
                footer.top + 5.0 * s,
                footer.width() - 34.0 * s,
                22.0 * s,
            ),
            title,
            11.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
}
fn media(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let title = m.data["title"].as_str().unwrap_or("");
    if title.is_empty() {
        c.empty("Nothing playing", "Play music in any media app");
        return;
    }
    let compact = r.height() < 110.0 * s
        || m.placed.widget == "media-pill"
        || m.str("variant", "card") == "strip";
    let art = if compact {
        r.height().min(60.0 * s)
    } else {
        (r.height() - 48.0 * s).min(r.width() * 0.38)
    };
    if let Some(image) = &m.artwork {
        c.image(Rect::new(r.left, r.top, art, art), image, 1.0, false);
    } else {
        c.fill(
            Rect::new(r.left, r.top, art, art),
            10.0 * s,
            c.tokens.colors.hover,
        );
        c.centered(
            Rect::new(r.left, r.top, art, art),
            "♫",
            28.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
    let x = r.left + art + 12.0 * s;
    let w = (r.right - x).max(1.0);
    c.label(
        Rect::new(x, r.top, w, 26.0 * s),
        title,
        16.0,
        c.tokens.colors.text,
        600,
    );
    c.label(
        Rect::new(x, r.top + 27.0 * s, w, 22.0 * s),
        m.data["artist"].as_str().unwrap_or(""),
        12.0,
        c.tokens.colors.text_dim,
        400,
    );
    let y = if compact {
        r.bottom - 28.0 * s
    } else {
        r.bottom - 32.0 * s
    };
    let cx = if compact {
        x + w / 2.0
    } else {
        r.left + r.width() / 2.0
    };
    for (id, dx, glyph, label) in [
        (PREVIOUS, -46.0, "‹", "Previous track"),
        (
            PRIMARY,
            0.0,
            if m.data["playing"].as_bool() == Some(true) {
                "Ⅱ"
            } else {
                "▶"
            },
            "Play or pause",
        ),
        (NEXT, 46.0, "›", "Next track"),
    ] {
        c.button(
            id,
            Rect::new(cx + (dx - 16.0) * s, y, 32.0 * s, 28.0 * s),
            glyph,
            label,
        );
    }
    if m.flag("progress", true)
        && let (Some(position), Some(duration)) =
            (m.data["position"].as_f64(), m.data["duration"].as_f64())
        && duration > 0.0
    {
        let advance = if m.data["playing"].as_bool() == Some(true) {
            (now.ms - m.data["_at"].as_i64().unwrap_or(now.ms)).max(0) as f64 / 1000.0
        } else {
            0.0
        };
        let progress = ((position + advance) / duration).clamp(0.0, 1.0) as f32;
        let y = if compact {
            r.bottom - 2.0 * s
        } else {
            y - 10.0 * s
        };
        let track = Rect::new(r.left, y, r.width(), 2.0 * s);
        c.fill(track, s, c.tokens.colors.line);
        c.fill(
            Rect::new(
                track.left,
                track.top,
                track.width() * progress,
                track.height(),
            ),
            s,
            c.tokens.colors.text.with_alpha(0.7),
        );
    }
    if m.str("variant", "card") == "viz" && !m.spectrum.is_empty() {
        let width = r.width() / m.spectrum.len() as f32;
        for (i, v) in m.spectrum.iter().enumerate() {
            let h = (*v * m.num("sensitivity", 1.2) as f32 * 32.0 * s).min(32.0 * s);
            c.fill(
                Rect::new(
                    r.left + i as f32 * width,
                    y - h - 6.0 * s,
                    (width - 2.0 * s).max(1.0),
                    h,
                ),
                2.0 * s,
                c.tokens.colors.text.with_alpha(0.65),
            );
        }
    }
}
pub const TOGGLES: [(&str, &str, &str, &str); 8] = [
    ("wifi", "Wi-Fi", "\u{E701}", "ms-settings:network-wifi"),
    (
        "bluetooth",
        "Bluetooth",
        "\u{E702}",
        "ms-settings:bluetooth",
    ),
    ("focus", "Focus", "\u{E708}", "ms-settings:quiethours"),
    (
        "airplane",
        "Airplane mode",
        "\u{E709}",
        "ms-settings:network-airplanemode",
    ),
    (
        "nightlight",
        "Night light",
        "\u{E706}",
        "ms-settings:nightlight",
    ),
    ("sound", "Sound", "\u{E767}", "ms-settings:sound"),
    ("display", "Display", "\u{E7F4}", "ms-settings:display"),
    ("battery", "Battery", "\u{E850}", "ms-settings:batterysaver"),
];
fn quick(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "grid");
    let items: Vec<_> = TOGGLES
        .iter()
        .enumerate()
        .filter(|(_, t)| m.flag(t.0, true))
        .collect();
    let cols = if variant == "list" {
        1
    } else if variant == "row" {
        items.len().max(1)
    } else {
        (r.width() / (76.0 * s)).floor().max(2.0) as usize
    };
    let rows = items.len().div_ceil(cols).max(1);
    let w = r.width() / cols as f32;
    let h = r.height() / rows as f32;
    for (n, (index, (_, label, glyph, _))) in items.iter().enumerate() {
        let rect = Rect::new(
            r.left + (n % cols) as f32 * w,
            r.top + (n / cols) as f32 * h,
            w - 6.0 * s,
            h - 6.0 * s,
        );
        c.fill(rect, 12.0 * s, c.tokens.colors.hover.with_alpha(0.14));
        let gr = Rect::new(
            rect.left,
            rect.top,
            if cols == 1 { 42.0 * s } else { rect.width() },
            rect.height() * 0.56,
        );
        let style = TextStyle::new(&c.tokens.icons, 21.0 * s);
        c.list.ops.push(Op::Text {
            rect: Rect::new(
                gr.left + gr.width() / 2.0 - 11.0 * s,
                gr.top + 6.0 * s,
                24.0 * s,
                28.0 * s,
            ),
            text: Arc::from(*glyph),
            style,
            color: c.tokens.colors.text,
        });
        if rect.height() > 48.0 * s || cols == 1 {
            c.centered(
                if cols == 1 {
                    Rect::new(
                        rect.left + 42.0 * s,
                        rect.top,
                        rect.width() - 42.0 * s,
                        rect.height(),
                    )
                } else {
                    Rect::new(
                        rect.left,
                        rect.top + rect.height() * 0.57,
                        rect.width(),
                        rect.height() * 0.4,
                    )
                },
                label,
                10.0,
                c.tokens.colors.text,
                500,
            );
        }
        c.hit(ITEM + *index as u32, rect, Role::Button, label);
    }
}
fn launcher(c: &mut Draw<'_>, m: &Model) {
    let mut r = c.body();
    let s = c.scale;
    if m.flag("search", true) {
        c.field(
            m,
            Rect::new(r.left, r.top, r.width(), 32.0 * s),
            "Search or enter an address",
        );
        r.top += 42.0 * s;
    }
    let variant = m.str("variant", "dock");
    let apps = m.list("shortcuts");
    let cols = if variant == "list" {
        1
    } else if variant == "dock" {
        apps.len().max(1)
    } else {
        (r.width() / (70.0 * s)).floor().max(1.0) as usize
    };
    let rows = apps.len().div_ceil(cols).max(1);
    let (w, h) = (r.width() / cols as f32, r.height() / rows as f32);
    for (i, app) in apps.iter().enumerate() {
        let cell = Rect::new(
            r.left + (i % cols) as f32 * w,
            r.top + (i / cols) as f32 * h,
            w,
            h,
        );
        let label = app["label"].as_str().unwrap_or("App");
        let size = (m.num("iconSize", 48.0) as f32 * s)
            .min(h - 18.0 * s)
            .min(w - 8.0 * s)
            .max(16.0 * s);
        let icon = if cols == 1 {
            Rect::new(cell.left, cell.top + 4.0 * s, size, size)
        } else {
            Rect::new(cell.left + (w - size) / 2.0, cell.top + 4.0 * s, size, size)
        };
        if let Some(image) = m.icons.get(&i) {
            c.image(icon, image, 1.0, true);
        } else {
            c.fill(icon, 10.0 * s, c.tokens.colors.hover);
            c.centered(
                icon,
                &label.chars().next().unwrap_or('•').to_string(),
                20.0,
                c.tokens.colors.text,
                500,
            );
        }
        if m.flag("labels", true) {
            if cols == 1 {
                c.label(
                    Rect::new(
                        icon.right + 10.0 * s,
                        cell.top + 10.0 * s,
                        cell.right - icon.right - 10.0 * s,
                        24.0 * s,
                    ),
                    label,
                    13.0,
                    c.tokens.colors.text,
                    500,
                );
            } else {
                c.centered(
                    Rect::new(cell.left, icon.bottom + 5.0 * s, w, 18.0 * s),
                    label,
                    10.0,
                    c.tokens.colors.text,
                    400,
                );
            }
        }
        c.hit(ITEM + i as u32, cell, Role::Button, label);
    }
}
fn weather(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    let current = &m.data["current"];
    if !current.is_object() {
        c.empty(
            if m.error.is_some() {
                "Weather unavailable"
            } else {
                "Weather"
            },
            m.error.as_deref().unwrap_or("Finding your local forecast…"),
        );
        return;
    }
    let name = current["location"]["name"]
        .as_str()
        .unwrap_or("Local weather");
    c.title(name);
    let temp = current["temperature"]
        .as_f64()
        .or_else(|| current["temperature_2m"].as_f64())
        .unwrap_or(0.0);
    let condition = current["condition"]["text"]
        .as_str()
        .or_else(|| current["condition"].as_str())
        .unwrap_or("Current conditions");
    let compact = m.str("variant", "card") == "compact" || r.height() < 115.0 * s;
    c.label(
        Rect::new(r.left, r.top + 22.0 * s, r.width() * 0.62, 65.0 * s),
        &format!("{temp:.0}°"),
        if compact { 38.0 } else { 52.0 },
        c.tokens.colors.text,
        400,
    );
    c.label(
        Rect::new(
            r.left,
            r.top + if compact { 66.0 * s } else { 84.0 * s },
            r.width(),
            24.0 * s,
        ),
        condition,
        12.0,
        c.tokens.colors.text_dim,
        400,
    );
    c.centered(
        Rect::new(r.right - 65.0 * s, r.top + 18.0 * s, 65.0 * s, 60.0 * s),
        weather_glyph(current),
        36.0,
        c.tokens.colors.text,
        400,
    );
    if !compact {
        let days = m.data["forecast"]["days"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let count = days.len().min(if m.str("variant", "card") == "forecast" {
            6
        } else {
            3
        });
        let width = r.width() / count.max(1) as f32;
        for (i, d) in days.iter().take(count).enumerate() {
            let x = r.left + i as f32 * width;
            let y = r.bottom - 48.0 * s;
            c.label(
                Rect::new(x, y, width, 18.0 * s),
                d["date"]
                    .as_str()
                    .map(|d| d.get(5..).unwrap_or(d))
                    .unwrap_or(""),
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.label(
                Rect::new(x, y + 20.0 * s, width, 24.0 * s),
                &format!(
                    "{:.0}° / {:.0}°",
                    d["max"].as_f64().unwrap_or(0.0),
                    d["min"].as_f64().unwrap_or(0.0)
                ),
                12.0,
                c.tokens.colors.text,
                500,
            );
        }
    }
    c.hit(PRIMARY, r, Role::Button, "Open weather forecast");
}
fn weather_glyph(v: &Value) -> &'static str {
    let code = v["code"]
        .as_u64()
        .or_else(|| v["condition"]["code"].as_u64())
        .unwrap_or(0);
    match code {
        0 | 1 => "☀",
        2 | 3 => "☁",
        45 | 48 => "≋",
        71..=77 | 85 | 86 => "❄",
        95..=99 => "ϟ",
        _ => "☂",
    }
}
fn system(c: &mut Draw<'_>, m: &Model) {
    let Some(stats) = &m.stats else {
        c.empty("System", "Collecting the first sample…");
        return;
    };
    let r = c.body();
    let s = c.scale;
    let mut values = Vec::new();
    if m.flag("cpu", true) {
        values.push(("CPU".into(), stats.cpu, format!("{:.0}%", stats.cpu)));
    }
    if m.flag("ram", true) {
        let usage = stats.memory.used as f64 / stats.memory.total.max(1) as f64 * 100.0;
        values.push(("Memory".into(), usage, format!("{usage:.0}%")));
    }
    if m.flag("gpu", true)
        && let Some(gpu) = stats.gpu
    {
        values.push(("GPU".into(), gpu, format!("{gpu:.0}%")));
    }
    if m.flag("net", true)
        && let Some(net) = &stats.net
    {
        values.push((
            "Network".into(),
            0.0,
            format!("↓{} ↑{}", rate(net.down), rate(net.up)),
        ));
    }
    if m.flag("disk", false)
        && let Some(disks) = &stats.disks
    {
        for disk in disks {
            let filter = m.str("drives", "");
            if !filter.is_empty() && !filter.to_uppercase().contains(&disk.name[..1]) {
                continue;
            }
            let pct = disk.used as f64 / disk.total.max(1) as f64 * 100.0;
            values.push((disk.name.clone(), pct, format!("{pct:.0}%")));
        }
    }
    if m.flag("battery", true)
        && let Some(b) = &stats.battery
    {
        values.push((
            "Battery".into(),
            b.percent as f64,
            format!("{}%{}", b.percent, if b.charging { " +" } else { "" }),
        ));
    }
    let fallback = c.tokens.colors.text;
    let metric_color = |index: usize| {
        if m.flag("vary", true) {
            Rgba::rgb8(
                [
                    [130, 190, 255],
                    [126, 211, 172],
                    [216, 172, 255],
                    [247, 200, 133],
                ][index % 4],
                0.85,
            )
        } else {
            fallback.with_alpha(0.8)
        }
    };
    let variant = m.str("variant", "rings");
    if variant == "rings" {
        let cols = if values.len() <= 3 {
            values.len().max(1)
        } else {
            3
        };
        let rows = values.len().div_ceil(cols).max(1);
        let w = r.width() / cols as f32;
        let h = r.height() / rows as f32;
        for (i, (label, pct, value)) in values.iter().enumerate() {
            let x = r.left + (i % cols) as f32 * w;
            let y = r.top + (i / cols) as f32 * h;
            let radius = (w.min(h - 20.0 * s) * 0.35).max(8.0 * s);
            let cy = y + (h - 20.0 * s) / 2.0;
            c.ring(
                Point::new(x + w / 2.0, cy),
                radius,
                pct.round() as f32 / 100.0,
                metric_color(i),
                3.0,
            );
            c.centered(
                Rect::new(x, cy - 12.0 * s, w, 24.0 * s),
                value,
                if value.len() > 6 { 8.0 } else { 14.0 },
                c.tokens.colors.text,
                500,
            );
            c.centered(
                Rect::new(x, y + h - 22.0 * s, w, 20.0 * s),
                label,
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
        }
    } else {
        let h = r.height() / values.len().max(1) as f32;
        for (i, (label, pct, value)) in values.iter().enumerate() {
            let y = r.top + i as f32 * h;
            c.label(
                Rect::new(r.left, y, r.width() * 0.42, 20.0 * s),
                label,
                11.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.label(
                Rect::new(r.left + r.width() * 0.45, y, r.width() * 0.55, 20.0 * s),
                value,
                13.0,
                c.tokens.colors.text,
                500,
            );
            if variant == "spark" && i < 3 {
                let points: Vec<_> = m.history.iter().map(|row| row[i]).collect();
                let step = r.width() / points.len().max(2) as f32;
                for (j, pair) in points.windows(2).enumerate() {
                    c.line(
                        Point::new(
                            r.left + j as f32 * step,
                            y + h - 5.0 * s - pair[0] as f32 / 100.0 * (h - 25.0 * s).max(1.0),
                        ),
                        Point::new(
                            r.left + (j + 1) as f32 * step,
                            y + h - 5.0 * s - pair[1] as f32 / 100.0 * (h - 25.0 * s).max(1.0),
                        ),
                        c.tokens.colors.text,
                        1.3,
                    );
                }
            } else {
                let bar = Rect::new(r.left, y + h - 10.0 * s, r.width(), 3.0 * s);
                c.fill(bar, 2.0 * s, c.tokens.colors.line);
                c.fill(
                    Rect::new(
                        bar.left,
                        bar.top,
                        bar.width() * (*pct as f32 / 100.0).clamp(0.0, 1.0),
                        bar.height(),
                    ),
                    2.0 * s,
                    metric_color(i),
                );
            }
        }
    }
}
fn rate(bytes: f64) -> String {
    if bytes >= 1_000_000.0 {
        format!("{:.1}M", bytes / 1_000_000.0)
    } else {
        format!("{:.0}K", bytes / 1000.0)
    }
}
fn timer(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let left = m.timer.left(now.ms);
    let seconds = (left + 999) / 1000;
    let value = if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    };
    let modes = r.width() > 150.0 * s && r.height() > 150.0 * s;
    if modes {
        for (i, (mode, label)) in [
            ("focus", "Focus"),
            ("short", "Short"),
            ("long", "Long"),
            ("custom", "Custom"),
        ]
        .iter()
        .enumerate()
        {
            let rect = Rect::new(
                r.left + i as f32 * r.width() / 4.0,
                r.top,
                r.width() / 4.0 - 3.0 * s,
                22.0 * s,
            );
            c.button(MODE + i as u32, rect, label, &format!("{label} timer"));
            if m.timer.mode == *mode {
                c.stroke(rect, 6.0 * s, c.tokens.colors.text_dim, 1.0);
            }
        }
    }
    let r = if modes {
        Rect::new(r.left, r.top + 28.0 * s, r.width(), r.height() - 28.0 * s)
    } else {
        r
    };
    let wide = r.width() > r.height() * 1.35;
    let radius = (if wide {
        (r.height() / 2.0 - 5.0 * s).min(r.width() * 0.30)
    } else {
        r.width().min(r.height() - 40.0 * s) / 2.0 - 4.0 * s
    })
    .max(12.0 * s);
    let center = Point::new(
        if wide {
            r.left + radius + 4.0 * s
        } else {
            r.left + r.width() / 2.0
        },
        r.top + radius + 4.0 * s,
    );
    c.ring(
        center,
        radius,
        1.0 - left as f32 / m.timer.total as f32,
        c.tokens.colors.text,
        3.5,
    );
    c.centered(
        Rect::new(
            center.x - radius,
            center.y - 20.0 * s,
            radius * 2.0,
            32.0 * s,
        ),
        &value,
        (radius / s * 0.5).min(30.0),
        c.tokens.colors.text,
        500,
    );
    c.centered(
        Rect::new(
            center.x - radius,
            center.y + 12.0 * s,
            radius * 2.0,
            20.0 * s,
        ),
        if left == 0 { "Done" } else { &m.timer.mode },
        10.0,
        c.tokens.colors.text_dim,
        500,
    );
    let y = r.bottom - 28.0 * s;
    let x = if wide {
        center.x + radius + 12.0 * s
    } else {
        r.left + (r.width() - 106.0 * s) / 2.0
    };
    c.button(
        RESET,
        Rect::new(x, y, 28.0 * s, 26.0 * s),
        "↺",
        "Reset timer",
    );
    c.button(
        PRIMARY,
        Rect::new(x + 36.0 * s, y, 32.0 * s, 26.0 * s),
        if m.timer.ends_at.is_some() {
            "Ⅱ"
        } else {
            "▶"
        },
        "Start or pause timer",
    );
    c.button(
        NEXT,
        Rect::new(x + 76.0 * s, y, 28.0 * s, 26.0 * s),
        "›",
        "Skip to next session",
    );
}
fn countdown(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let (target, start) = m.countdown(now);
    let left = target - now.ms;
    let title = m.str("title", "New Year");
    let days = if left > 0 {
        (left + 86_399_999) / 86_400_000
    } else {
        (-left) / 86_400_000
    };
    let target_date = Date::parse(&m.str("date", "")).unwrap_or(Date {
        year: now.date.year + 1,
        month: 1,
        day: 1,
    });
    let arrived =
        left <= 0 && (now.date == target_date || m.str("afterwards", "countUp") == "stop");
    if m.str("style", "digits") == "ring" {
        c.ring(
            Point::new(r.left + r.width() / 2.0, r.top + r.height() / 2.0),
            r.height().min(r.width()) * 0.43,
            ((now.ms - start) as f64 / (target - start).max(1) as f64) as f32,
            c.tokens.colors.text,
            3.0,
        );
    }
    c.centered(
        Rect::new(r.left, r.top, r.width(), 22.0 * s),
        &title,
        12.0,
        c.tokens.colors.text_dim,
        500,
    );
    c.centered(
        Rect::new(r.left, r.top + 20.0 * s, r.width(), r.height() - 40.0 * s),
        &if arrived {
            "Today".to_owned()
        } else {
            days.to_string()
        },
        38.0,
        c.tokens.colors.text,
        400,
    );
    let detail = if arrived {
        "The day is here".into()
    } else if left < 0 {
        "days since".into()
    } else {
        let seconds = (left
            + if m.flag("showSeconds", false) {
                999
            } else {
                59_999
            })
            / if m.flag("showSeconds", false) {
                1000
            } else {
                60_000
            }
            * if m.flag("showSeconds", false) { 1 } else { 60 };
        if m.flag("showSeconds", false) {
            format!(
                "days · {:02}:{:02}:{:02}",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            )
        } else {
            format!(
                "days · {:02}h {:02}m",
                seconds / 3600 % 24,
                seconds / 60 % 60
            )
        }
    };
    c.centered(
        Rect::new(r.left, r.bottom - 22.0 * s, r.width(), 22.0 * s),
        &detail,
        11.0,
        c.tokens.colors.text_dim,
        400,
    );
}
fn photo(c: &mut Draw<'_>, m: &Model, instant: Instant) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "bleed");
    let area = if variant == "polaroid" {
        Rect::new(r.left, r.top, r.width(), (r.height() - 32.0 * s).max(1.0))
    } else {
        r
    };
    if variant == "matte" || variant == "polaroid" {
        c.fill(
            r.inflate(8.0 * s),
            8.0 * s,
            Rgba::new(0.95, 0.94, 0.91, 0.95),
        );
    }
    if let Some(image) = &m.photo.current {
        let fraction = m.photo.fade.map_or(1.0, |start| {
            (instant.saturating_duration_since(start).as_secs_f32() / 0.35).clamp(0.0, 1.0)
        });
        if let Some(previous) = &m.photo.previous {
            c.image(area, previous, 1.0, m.str("fit", "cover") == "contain");
        }
        c.image(area, image, fraction, m.str("fit", "cover") == "contain");
        if variant == "polaroid" {
            c.centered(
                Rect::new(r.left, area.bottom + 8.0 * s, r.width(), 24.0 * s),
                &m.str("caption", ""),
                12.0,
                Rgba::new(0.16, 0.16, 0.17, 1.0),
                400,
            );
        }
    } else {
        c.empty(
            "Your photos",
            m.error.as_deref().unwrap_or("Choose images in Studio"),
        );
    }
    c.hit(PRIMARY, r, Role::Button, "Next photo");
}
fn notes(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    if m.field.is_some() {
        c.field(m, r, "Note");
        return;
    }
    c.hit(FIELD, r, Role::Button, "Edit note");
    if m.note.trim().is_empty() {
        c.empty("Click to write", "# a heading, - [ ] a task");
        return;
    }
    let mut y = r.top;
    for (index, line) in m.note.lines().enumerate() {
        if y >= r.bottom {
            break;
        }
        if line.trim().is_empty() {
            y += 8.0 * s;
            continue;
        }
        let heading = line.chars().take_while(|c| *c == '#').count().min(3);
        let is_heading = heading > 0 && line.as_bytes().get(heading) == Some(&b' ');
        let task =
            line.starts_with("- [ ] ") || line.starts_with("- [x] ") || line.starts_with("- [X] ");
        let mut value = if is_heading {
            line[heading + 1..].to_owned()
        } else if task {
            line[6..].to_owned()
        } else if let Some(bullet) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            format!("• {bullet}")
        } else {
            line.strip_prefix("> ").unwrap_or(line).to_owned()
        };
        value = value.replace("**", "").replace('`', "");
        let size = (if is_heading {
            24.0 - heading as f32 * 3.0
        } else {
            14.0
        }) * m.num("fontScale", 1.0) as f32;
        let style = c.tokens.font.scaled(size / 14.0 * s);
        let x = r.left + if task { 24.0 * s } else { 0.0 };
        let width = r.right - x;
        let lines = (c.text.measure(&value, &style).0 / width).ceil().max(1.0);
        let height = (lines * size * s * 1.45).min(r.bottom - y);
        c.wrapped(
            Rect::new(x, y, width, height),
            &value,
            size,
            c.tokens.colors.text,
            if is_heading { 600 } else { 400 },
        );
        if task {
            let box_r = Rect::new(r.left, y + 3.0 * s, 15.0 * s, 15.0 * s);
            c.stroke(box_r, 3.0 * s, c.tokens.colors.text_dim, 1.0);
            if !line.starts_with("- [ ]") {
                c.centered(box_r, "✓", 11.0, c.tokens.colors.text, 600);
            }
            c.hit(
                NOTE_TASK + index as u32,
                Rect::new(r.left, y, r.width(), height),
                Role::CheckBox,
                &value,
            );
        }
        y += height + 3.0 * s;
    }
}
fn todo(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    let done = m.tasks.iter().filter(|t| t.done).count();
    c.title(&m.str("title", "To do"));
    c.label(
        Rect::new(r.right - 60.0 * s, r.top, 60.0 * s, 20.0 * s),
        &format!("{} left", m.tasks.len() - done),
        10.0,
        c.tokens.colors.text_dim,
        400,
    );
    let top = m.str("addPosition", "bottom") == "top";
    let field = Rect::new(
        r.left,
        if top {
            r.top + 26.0 * s
        } else {
            r.bottom - 28.0 * s
        },
        r.width(),
        28.0 * s,
    );
    let list_top = r.top + if top { 62.0 * s } else { 26.0 * s };
    let list_bottom = if top { r.bottom } else { field.top - 6.0 * s };
    let mut y = list_top;
    for (i, task) in m
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| !m.flag("hideDone", false) || !t.done)
    {
        if y + 30.0 * s > list_bottom {
            break;
        }
        let row = Rect::new(r.left, y, r.width(), 30.0 * s);
        let check = Rect::new(row.left, row.top + 5.0 * s, 16.0 * s, 16.0 * s);
        c.stroke(check, 4.0 * s, c.tokens.colors.text_dim, 1.0);
        if task.done {
            c.centered(check, "✓", 12.0, c.tokens.colors.text, 600);
        }
        c.hit(ITEM + i as u32, check, Role::CheckBox, &task.text);
        if let Some(hit) = c.list.hits.last_mut() {
            hit.checked = Some(task.done);
        }
        if m.task_edit == Some(i) {
            c.field(
                m,
                Rect::new(
                    row.left + 24.0 * s,
                    row.top,
                    row.width() - 46.0 * s,
                    28.0 * s,
                ),
                "Edit task",
            );
        } else {
            c.label(
                Rect::new(
                    row.left + 24.0 * s,
                    row.top + 3.0 * s,
                    row.width() - 46.0 * s,
                    24.0 * s,
                ),
                &task.text,
                12.0,
                if task.done {
                    c.tokens.colors.text_dim
                } else {
                    c.tokens.colors.text
                },
                400,
            );
            c.hit(
                EDIT_TASK + i as u32,
                Rect::new(
                    row.left + 22.0 * s,
                    row.top,
                    row.width() - 44.0 * s,
                    row.height(),
                ),
                Role::Button,
                &format!("Edit {}", task.text),
            );
        }
        c.button(
            DELETE_TASK + i as u32,
            Rect::new(row.right - 20.0 * s, row.top + 3.0 * s, 20.0 * s, 22.0 * s),
            "×",
            "Delete task",
        );
        y += 32.0 * s;
    }
    if m.tasks.is_empty() {
        c.centered(
            Rect::new(
                r.left,
                list_top,
                r.width(),
                (list_bottom - list_top).max(1.0),
            ),
            "Nothing to do",
            14.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
    if m.task_edit.is_none() {
        c.field(m, field, "Add a task");
    }
}
fn rss(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    c.title("HEADLINES");
    let items = m.data["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        c.empty(
            "Headlines",
            m.error.as_deref().unwrap_or("Fetching your feeds…"),
        );
        return;
    }
    let cards = m.str("variant", "list") == "cards";
    let h = if cards { 74.0 * s } else { 56.0 * s };
    for (i, item) in items
        .iter()
        .take(m.num("maxItems", 12.0) as usize)
        .enumerate()
    {
        let y = r.top + 24.0 * s + i as f32 * h;
        if y + h > r.bottom {
            break;
        }
        let row = Rect::new(r.left, y, r.width(), h - 6.0 * s);
        if cards {
            c.fill(row, 8.0 * s, c.tokens.colors.hover.with_alpha(0.12));
        }
        c.wrapped(
            Rect::new(
                row.left + 4.0 * s,
                row.top,
                row.width() - 8.0 * s,
                h - 23.0 * s,
            ),
            item["title"].as_str().unwrap_or("Headline"),
            12.0,
            c.tokens.colors.text,
            500,
        );
        let mut detail = if m.flag("showSource", true) {
            item["source"].as_str().unwrap_or("").into()
        } else {
            String::new()
        };
        if m.flag("showTime", true)
            && let Some(at) = item["published"].as_i64()
        {
            let minutes = ((now.ms / 1000 - at) / 60).max(0);
            detail.push_str(&format!(
                "  {}",
                if minutes < 60 {
                    format!("{minutes}m")
                } else {
                    format!("{}h", minutes / 60)
                }
            ));
        }
        c.label(
            Rect::new(
                row.left + 4.0 * s,
                row.bottom - 17.0 * s,
                row.width() - 8.0 * s,
                17.0 * s,
            ),
            &detail,
            9.0,
            c.tokens.colors.text_dim,
            400,
        );
        c.hit(
            ITEM + i as u32,
            row,
            Role::Link,
            item["title"].as_str().unwrap_or("Open headline"),
        );
    }
}
fn crypto(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    c.title("MARKETS");
    let coins = m.data["coins"]
        .as_array()
        .or_else(|| m.data["items"].as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    if coins.is_empty() {
        c.empty("Markets", m.error.as_deref().unwrap_or("Fetching prices…"));
        return;
    }
    let h = (r.height() - 24.0 * s) / coins.len().max(1) as f32;
    for (i, coin) in coins.iter().enumerate() {
        let y = r.top + 24.0 * s + i as f32 * h;
        c.label(
            Rect::new(r.left, y, r.width() * 0.45, h),
            coin["name"].as_str().unwrap_or("Coin"),
            12.0,
            c.tokens.colors.text,
            500,
        );
        c.label(
            Rect::new(r.left + r.width() * 0.45, y, r.width() * 0.55, 22.0 * s),
            &format!(
                "{} {:.2}",
                m.str("currency", "usd").to_uppercase(),
                coin["current_price"]
                    .as_f64()
                    .or_else(|| coin["price"].as_f64())
                    .unwrap_or(0.0)
            ),
            12.0,
            c.tokens.colors.text,
            500,
        );
        if m.flag("showChange", true) {
            let change = coin["price_change_percentage_24h"]
                .as_f64()
                .or_else(|| coin["change"].as_f64())
                .unwrap_or(0.0);
            c.label(
                Rect::new(
                    r.left + r.width() * 0.45,
                    y + 21.0 * s,
                    r.width() * 0.55,
                    18.0 * s,
                ),
                &format!("{change:+.2}%"),
                10.0,
                if change >= 0.0 {
                    Rgba::rgb8([137, 218, 175], 1.0)
                } else {
                    Rgba::rgb8([255, 156, 146], 1.0)
                },
                400,
            );
        }
        c.hit(
            ITEM + i as u32,
            Rect::new(r.left, y, r.width(), h),
            Role::Link,
            coin["name"].as_str().unwrap_or("Market"),
        );
    }
}
fn heatmap(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let total: u32 = m.counts.values().sum();
    let variant = m.str("variant", "weeks");
    c.title("ACTIVITY");
    if variant == "streak" {
        let mut day = if m.counts.contains_key(&now.date.key()) {
            now.date
        } else {
            now.date.add_days(-1)
        };
        let mut streak = 0;
        while m.counts.get(&day.key()).copied().unwrap_or(0) > 0 {
            streak += 1;
            day = day.add_days(-1);
        }
        c.centered(
            Rect::new(r.left, r.top + 20.0 * s, r.width(), r.height() - 50.0 * s),
            &streak.to_string(),
            48.0,
            c.tokens.colors.text,
            400,
        );
        c.centered(
            Rect::new(r.left, r.bottom - 28.0 * s, r.width(), 24.0 * s),
            &format!("day streak · {total} completed"),
            11.0,
            c.tokens.colors.text_dim,
            400,
        );
        return;
    }
    let plot = Rect::new(
        r.left,
        r.top + 26.0 * s,
        r.width(),
        r.height()
            - if m.flag("legend", true) {
                50.0 * s
            } else {
                26.0 * s
            },
    );
    let week_start = if m.str("weekStart", "monday") == "sunday" {
        0
    } else {
        1
    };
    let (dates, cols, rows) = if variant == "month" {
        let grid = MonthGrid::new(now.date, week_start);
        let rows = grid.cells.len() / 7;
        (grid.cells, 7, rows)
    } else {
        let cols = (plot.width() / (12.0 * s)).floor().max(4.0) as usize;
        let first = now.date.add_days(
            -i64::from((now.date.weekday() + 7 - week_start) % 7) - (cols as i64 - 1) * 7,
        );
        (
            (0..cols * 7)
                .map(|i| Some(first.add_days((i / 7 * 7 + i % 7) as i64)))
                .collect(),
            cols,
            7,
        )
    };
    let cell = (plot.width() / cols as f32).min(plot.height() / rows as f32);
    let mut busy: Vec<_> = m.counts.values().filter(|&&v| v > 0).copied().collect();
    busy.sort_unstable();
    let max = busy.get(busy.len() * 85 / 100).copied().unwrap_or(1).max(1);
    for (i, date) in dates.into_iter().enumerate() {
        let Some(date) = date else {
            continue;
        };
        let (col, row) = if variant == "month" {
            (i % 7, i / 7)
        } else {
            (i / 7, i % 7)
        };
        let n = m.counts.get(&date.key()).copied().unwrap_or(0);
        let level = if max <= 4 {
            n.min(4)
        } else {
            (n * 4).div_ceil(max).min(4)
        };
        let rect = Rect::new(
            plot.left + col as f32 * cell,
            plot.top + row as f32 * cell,
            (cell - 3.0 * s).max(2.0),
            (cell - 3.0 * s).max(2.0),
        );
        c.fill(
            rect,
            2.0 * s,
            if date > now.date {
                c.tokens.colors.line.with_alpha(0.04)
            } else {
                Rgba::rgb8(
                    [138, 219, 178],
                    if level == 0 {
                        0.08
                    } else {
                        0.2 + level as f32 * 0.18
                    },
                )
            },
        );
        c.hit(
            ITEM + i as u32,
            rect,
            Role::Button,
            &format!("{}: {n} completed", date.key()),
        );
    }
    if m.flag("legend", true) {
        c.label(
            Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s),
            &format!("{total} completed  ·  Less ▪ ▪ ▪ More"),
            10.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
}
fn quote(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let builtins: Vec<Value> =
        serde_json::from_str(include_str!("quotes.json")).unwrap_or_default();
    let source = m.str("source", "builtin");
    let mut quotes = if source == "custom" {
        Vec::new()
    } else {
        builtins
    };
    if source != "builtin" {
        quotes.extend_from_slice(m.list("custom"));
    }
    quotes.retain(|q| q["text"].as_str().is_some_and(|t| !t.trim().is_empty()));
    if quotes.is_empty() {
        c.empty("Your words", "Add quotes in Studio");
        return;
    }
    let period = match m.str("mode", "daily").as_str() {
        "click" => 0,
        "hourly" => now.date.ordinal() * 24 + i64::from(now.hour),
        _ => now.date.ordinal(),
    };
    let index = (period.rem_euclid(quotes.len() as i64) as usize + m.quote_offset) % quotes.len();
    let q = &quotes[index];
    c.wrapped(
        Rect::new(r.left, r.top, r.width(), r.height() - 24.0 * s),
        q["text"].as_str().unwrap_or(""),
        if r.height() > 120.0 * s { 20.0 } else { 14.0 },
        c.tokens.colors.text,
        400,
    );
    c.label(
        Rect::new(r.left, r.bottom - 20.0 * s, r.width(), 20.0 * s),
        q["author"].as_str().unwrap_or(""),
        11.0,
        c.tokens.colors.text_dim,
        500,
    );
    c.hit(PRIMARY, r, Role::Button, "Next quote");
}
