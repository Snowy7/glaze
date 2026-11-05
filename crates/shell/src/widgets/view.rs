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
pub const RESIZE_EDGE: HitId = 5000;
pub const TODAY: HitId = 10;
pub const RETRY: HitId = 11;
pub const PRIVACY: HitId = 12;
pub const CANCEL: HitId = 13;
pub const EVENT_DATE: HitId = 14;
pub const EVENT_TIME: HitId = 15;
pub const EVENT_COLOR: HitId = 20;

pub fn resize_edges(hit: HitId) -> Option<[bool; 4]> {
    if hit == RESIZE {
        return Some([false, false, true, true]);
    }
    let edges = [
        [true, false, false, false],
        [false, true, false, false],
        [false, false, true, false],
        [false, false, false, true],
        [true, true, false, false],
        [false, true, true, false],
        [true, false, false, true],
    ];
    hit.checked_sub(RESIZE_EDGE)
        .and_then(|n| edges.get(n as usize))
        .copied()
}

pub fn plate_radius(model: &Model, rect: Rect, scale: f32) -> f32 {
    if model.placed.widget == "media-pill" {
        match model.str("variant", "pill").as_str() {
            "pill" | "mini" => rect.height() / 2.0,
            _ => 14.0 * scale,
        }
    } else {
        model
            .placed
            .appearance
            .get("radius")
            .and_then(Value::as_f64)
            .unwrap_or(22.0) as f32
            * scale
    }
}

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
        model,
        instant,
    };
    let radius = plate_radius(model, rect, scale);
    c.fill(rect, radius, Rgba::HIT);
    let surface = model
        .placed
        .appearance
        .get("surface")
        .and_then(Value::as_str)
        .unwrap_or("glass");
    if matches!(surface, "solid" | "tint") {
        c.fill(
            rect,
            radius,
            if surface == "solid" {
                tokens.material.tint.with_alpha(1.0)
            } else {
                tokens.material.tint
            },
        );
    }
    let design = model
        .placed
        .appearance
        .get("style")
        .and_then(Value::as_str)
        .unwrap_or("minimal");
    let border = model
        .placed
        .appearance
        .get("border")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if surface != "none" && border {
        match design {
            "depth" => c.stroke(
                rect.inflate(-2.0 * scale),
                radius,
                Rgba::new(0.0, 0.0, 0.0, 0.25),
                3.0,
            ),
            "paper" => {
                c.stroke(
                    rect.inflate(-4.0 * scale),
                    radius,
                    tokens.colors.text_dim.with_alpha(0.16),
                    1.0,
                );
            }
            "aero" => c.stroke(
                rect.inflate(-3.0 * scale),
                radius,
                Rgba::rgb8([150, 210, 255], 0.4),
                2.0,
            ),
            "liquid" => c.stroke(
                rect.inflate(-2.0 * scale),
                radius,
                Rgba::new(1.0, 1.0, 1.0, 0.16),
                1.0,
            ),
            _ => {}
        }
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
    if (surface != "none" && border) || editing {
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
    if editing {
        let handle = 10.0 * scale;
        for (i, (x, y, w, h)) in [
            (0.0, handle, handle, rect.height() - 2.0 * handle),
            (handle, 0.0, rect.width() - 2.0 * handle, handle),
            (
                rect.right - handle,
                handle,
                handle,
                rect.height() - 2.0 * handle,
            ),
            (
                handle,
                rect.bottom - handle,
                rect.width() - 2.0 * handle,
                handle,
            ),
            (0.0, 0.0, handle, handle),
            (rect.right - handle, 0.0, handle, handle),
            (0.0, rect.bottom - handle, handle, handle),
        ]
        .into_iter()
        .enumerate()
        {
            c.hit(
                RESIZE_EDGE + i as u32,
                Rect::new(x, y, w, h),
                Role::Button,
                "Resize edge",
            );
        }
    }
    c.list.focus = model.focus;
    if let Some(hit) = model
        .focus
        .and_then(|id| c.list.hits.iter().find(|h| h.id == id))
        .cloned()
    {
        c.stroke(
            hit.rect.inflate(-1.0),
            4.0 * scale,
            c.tokens.colors.text,
            1.5,
        );
    }
    if let Some(hit) = model
        .pressed
        .and_then(|id| c.list.hits.iter().find(|h| h.id == id))
        .cloned()
    {
        c.fill(
            hit.rect,
            6.0 * scale,
            c.tokens.colors.hover.with_alpha(0.22),
        );
    }
    if let Some(message) = model.notice.as_ref().or(model.tooltip.as_ref()) {
        let height = (rect.height() * 0.48).clamp(24.0 * scale, 96.0 * scale);
        let box_r = Rect::new(
            8.0 * scale,
            rect.bottom - height - 8.0 * scale,
            rect.width() - 16.0 * scale,
            height,
        );
        c.fill(box_r, 8.0 * scale, Rgba::rgb8([25, 29, 35], 0.98));
        c.wrapped(
            box_r.inflate(-6.0 * scale),
            message,
            10.0,
            Rgba::rgb8([245; 3], 1.0),
            400,
        );
    }
    if model
        .placed
        .appearance
        .get("shadow")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        c.list.ops = text_shadows(c.list.ops, scale);
    }
    c.list
}

fn text_shadows(ops: Vec<Op>, scale: f32) -> Vec<Op> {
    let mut out = Vec::with_capacity(ops.len() * 2);
    for op in ops {
        match &op {
            Op::Text {
                rect,
                text,
                style,
                color,
            } => out.push(Op::Text {
                rect: rect.offset(0.0, scale),
                text: text.clone(),
                style: style.clone(),
                color: Rgba::new(0.0, 0.0, 0.0, color.a * 0.35),
            }),
            Op::Emphasis {
                rect,
                text,
                style,
                color,
                italic,
                strike,
            } => out.push(Op::Emphasis {
                rect: rect.offset(0.0, scale),
                text: text.clone(),
                style: style.clone(),
                color: Rgba::new(0.0, 0.0, 0.0, color.a * 0.35),
                italic: *italic,
                strike: *strike,
            }),
            Op::Clipped { rect, ops } => {
                out.push(Op::Clipped {
                    rect: *rect,
                    ops: text_shadows(ops.to_vec(), scale).into(),
                });
                continue;
            }
            _ => {}
        }
        out.push(op);
    }
    out
}

struct Draw<'a> {
    list: DisplayList,
    text: &'a mut Text,
    tokens: &'a Tokens,
    scale: f32,
    rect: Rect,
    pad: f32,
    model: &'a Model,
    instant: Instant,
}
impl Draw<'_> {
    fn scroll_start(&mut self, clip: Rect, height: f32) -> ((usize, usize), f32) {
        let max = ((height - clip.height()) / self.scale).max(0.0);
        self.model.scroll_max.set(max);
        self.model.scroll_page.set(clip.height() / self.scale);
        (
            (self.list.ops.len(), self.list.hits.len()),
            self.model.scroll.clamp(0.0, max) * self.scale,
        )
    }
    fn scroll_end(&mut self, mark: (usize, usize), clip: Rect) {
        let ops = self.list.ops.split_off(mark.0);
        self.list.ops.push(Op::Clipped {
            rect: clip,
            ops: ops.into(),
        });
        let hits = self.list.hits.split_off(mark.1);
        self.list
            .hits
            .extend(hits.into_iter().filter_map(|mut hit| {
                hit.rect = Rect {
                    left: hit.rect.left.max(clip.left),
                    top: hit.rect.top.max(clip.top),
                    right: hit.rect.right.min(clip.right),
                    bottom: hit.rect.bottom.min(clip.bottom),
                };
                (!hit.rect.is_empty()).then_some(hit)
            }));
        let max = self.model.scroll_max.get();
        if max > 0.0 {
            let page = self.model.scroll_page.get();
            let height = (clip.height() * page / (page + max)).max(12.0 * self.scale);
            let top = clip.top + self.model.scroll.min(max) / max * (clip.height() - height);
            self.fill(
                Rect::new(clip.right - 2.0 * self.scale, top, 2.0 * self.scale, height),
                self.scale,
                self.tokens.colors.text_dim.with_alpha(0.4),
            );
        }
    }
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
        let (width, height) = self.text.measure(&value, &style);
        let rect = Rect::new(rect.left, rect.top, width.min(rect.width()), height);
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
        for (i, line) in wrap_lines(self.text, value, &style, rect.width())
            .into_iter()
            .enumerate()
        {
            let y = rect.top + i as f32 * height;
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
    let color = c.tokens.colors.text;
    let variant = m.str("variant", "desk");
    match variant.as_str() {
        "analog" => {
            let footer = if m.flag("showDate", true) {
                24.0 * s
            } else {
                0.0
            };
            let radius = (r.width().min(r.height() - footer) * 0.45).max(12.0 * s);
            let center = Point::new(
                r.left + r.width() / 2.0,
                r.top + (r.height() - footer) / 2.0,
            );
            dial(c, center, radius, now, m.flag("showSeconds", false), true);
            if footer > 0.0 {
                c.centered(
                    Rect::new(r.left, r.bottom - footer, r.width(), footer),
                    &super::time::date_label(now.date, false),
                    11.0,
                    color,
                    500,
                );
            }
        }
        "world" => {
            let zones = m.list("zones");
            if zones.is_empty() {
                c.empty("World clocks", "Choose cities in Studio");
                return;
            }
            let cols = if r.width() > 420.0 * s { 2 } else { 1 };
            let rows = zones.len().div_ceil(cols);
            let h = (r.height() / rows as f32).max(64.0 * s);
            let w = r.width() / cols as f32;
            let (mark, offset) = c.scroll_start(r, rows as f32 * h);
            let utc = now.zone("UTC");
            for (i, zone) in zones.iter().enumerate() {
                let x = r.left + (i % cols) as f32 * w;
                let y = r.top + (i / cols) as f32 * h - offset;
                let tz = zone["tz"].as_str().unwrap_or("UTC");
                let label = zone["label"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(tz);
                let Some(time) = now.zone(tz) else {
                    c.label(Rect::new(x, y, w, 22.0 * s), label, 13.0, color, 600);
                    c.label(
                        Rect::new(x, y + 23.0 * s, w, h - 23.0 * s),
                        &format!("Unknown time zone: {tz}"),
                        10.0,
                        c.tokens.colors.text_dim,
                        400,
                    );
                    continue;
                };
                let radius = 20.0 * s;
                dial(
                    c,
                    Point::new(x + radius, y + radius + 4.0 * s),
                    radius,
                    time,
                    false,
                    false,
                );
                let text_x = x + radius * 2.0 + 10.0 * s;
                c.label(
                    Rect::new(text_x, y, w - (text_x - x), 20.0 * s),
                    label,
                    12.0,
                    color,
                    600,
                );
                c.label(
                    Rect::new(text_x, y + 20.0 * s, w - (text_x - x), 24.0 * s),
                    &time.clock(m.flag("format24", true), false),
                    17.0,
                    color,
                    500,
                );
                let delta = time.date.ordinal() - now.date.ordinal();
                let day = match delta {
                    -1 => "Yesterday",
                    0 => "Today",
                    1 => "Tomorrow",
                    _ => "",
                };
                let minutes = utc.map_or(0, |utc| {
                    (time.date.ordinal() - utc.date.ordinal()) * 1440
                        + i64::from(time.hour) * 60
                        + i64::from(time.minute)
                        - i64::from(utc.hour) * 60
                        - i64::from(utc.minute)
                });
                c.label(
                    Rect::new(text_x, y + 44.0 * s, w - (text_x - x), 18.0 * s),
                    &format!(
                        "{} · {day} · UTC{}{hours:02}:{mins:02}",
                        if (6..18).contains(&time.hour) {
                            "Day"
                        } else {
                            "Night"
                        },
                        if minutes < 0 { "−" } else { "+" },
                        hours = minutes.abs() / 60,
                        mins = minutes.abs() % 60
                    ),
                    9.0,
                    c.tokens.colors.text_dim,
                    400,
                );
            }
            c.scroll_end(mark, r);
        }
        "words" => {
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
                .unwrap_or("");
            let greeting = super::time::greeting(now.hour);
            let message = if name.is_empty() {
                greeting.to_owned()
            } else {
                format!("{greeting}, {name}")
            };
            let footer = if m.flag("showDate", true) {
                44.0 * s
            } else {
                24.0 * s
            };
            c.wrapped(
                Rect::new(r.left, r.top, r.width(), r.height() - footer),
                &message,
                (r.height() / s * 0.24).clamp(14.0, 28.0),
                color,
                500,
            );
            c.label(
                Rect::new(r.left, r.bottom - footer, r.width(), 24.0 * s),
                &now.clock(m.flag("format24", true), false),
                15.0,
                color,
                500,
            );
            if m.flag("showDate", true) {
                c.label(
                    Rect::new(r.left, r.bottom - 20.0 * s, r.width(), 20.0 * s),
                    &super::time::date_label(now.date, true),
                    11.0,
                    c.tokens.colors.text_dim,
                    400,
                );
            }
        }
        _ => {
            let value = now.clock(m.flag("format24", true), m.flag("showSeconds", false));
            let size = (r.width() / s / (value.chars().count() as f32 * 0.62))
                .min(r.height() / s * 0.52)
                .clamp(12.0, 76.0);
            let box_r = Rect::new(
                r.left,
                r.top + r.height() * 0.10,
                r.width(),
                r.height() * 0.63,
            );
            if variant == "flip" {
                let width = r.width() / value.chars().count() as f32;
                let phase = m.flip_at.map(|at| {
                    (c.instant.saturating_duration_since(at).as_secs_f32() / 0.28).clamp(0.0, 1.0)
                });
                let previous = m
                    .previous_clock
                    .map(|old| old.clock(m.flag("format24", true), m.flag("showSeconds", false)));
                for (i, ch) in value.chars().enumerate() {
                    let rect = Rect::new(
                        r.left + i as f32 * width + 2.0 * s,
                        box_r.top,
                        width - 4.0 * s,
                        box_r.height(),
                    );
                    let old = previous.as_ref().and_then(|old| old.chars().nth(i));
                    if ch.is_ascii_digit() {
                        c.fill(rect, 8.0 * s, Rgba::new(0.1, 0.12, 0.15, 0.4));
                        if old != Some(ch)
                            && let Some(phase) = phase
                        {
                            let fold = (phase * std::f32::consts::PI).sin() * rect.height() * 0.46;
                            c.fill(
                                Rect::new(
                                    rect.left,
                                    rect.top + rect.height() / 2.0 - fold,
                                    rect.width(),
                                    fold,
                                ),
                                3.0 * s,
                                Rgba::new(0.0, 0.0, 0.0, 0.32),
                            );
                        }
                        c.line(
                            Point::new(rect.left, rect.top + rect.height() / 2.0),
                            Point::new(rect.right, rect.top + rect.height() / 2.0),
                            c.tokens.colors.line,
                            1.0,
                        );
                    }
                    let shown = if phase.is_some_and(|phase| phase < 0.5) {
                        old.unwrap_or(ch)
                    } else {
                        ch
                    };
                    c.centered(rect, &shown.to_string(), size, color, 500);
                }
            } else {
                c.centered(box_r, &value, size, color, 400);
            }
            if m.flag("showDate", true) {
                c.centered(
                    Rect::new(r.left, r.top + r.height() * 0.76, r.width(), 24.0 * s),
                    &super::time::date_label(now.date, true),
                    12.0,
                    c.tokens.colors.text_dim,
                    500,
                );
            }
        }
    }
}
fn dial(c: &mut Draw<'_>, center: Point, radius: f32, time: Stamp, seconds: bool, numerals: bool) {
    let color = c.tokens.colors.text;
    c.ring(center, radius, 0.0, color, 1.0);
    if numerals {
        for number in 1..=12 {
            let angle = number as f32 * TAU / 12.0 - TAU / 4.0;
            let length = radius * 0.78;
            c.centered(
                Rect::new(
                    center.x + length * angle.cos() - 10.0 * c.scale,
                    center.y + length * angle.sin() - 9.0 * c.scale,
                    20.0 * c.scale,
                    18.0 * c.scale,
                ),
                &number.to_string(),
                9.0,
                color,
                500,
            );
        }
    }
    for (part, length, width) in [
        (
            (time.hour % 12) as f32 / 12.0 + time.minute as f32 / 720.0,
            0.52,
            3.0,
        ),
        (time.minute as f32 / 60.0, 0.70, 1.8),
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
    if seconds {
        let a = time.second as f32 * TAU / 60.0 - TAU / 4.0;
        c.line(
            center,
            Point::new(
                center.x + radius * 0.82 * a.cos(),
                center.y + radius * 0.82 * a.sin(),
            ),
            Rgba::rgb8([255, 155, 120], 1.0),
            1.0,
        );
    }
}
fn calendar(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "month");
    if m.event_draft.is_some() {
        event_editor(c, m);
        return;
    }
    c.label(
        Rect::new(r.left, r.top, r.width() - 70.0 * s, 28.0 * s),
        &format!("{} {}", MONTHS[m.month.month as usize - 1], m.month.year),
        16.0,
        c.tokens.colors.text,
        600,
    );
    c.hit(
        TODAY,
        Rect::new(r.left, r.top, r.width() - 70.0 * s, 28.0 * s),
        Role::Button,
        "Return to today",
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
    let day_events: Vec<_> = m
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["date"].as_str() == Some(m.selected.key().as_str()))
        .collect();
    let day_height = if variant != "agenda" && !day_events.is_empty() {
        (r.height() * 0.28).max(38.0 * s)
    } else {
        0.0
    };
    let grid = Rect::new(
        r.left,
        r.top + 32.0 * s,
        r.width(),
        (footer.top - r.top - 38.0 * s - day_height).max(24.0 * s),
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
        let (mark, offset) = c.scroll_start(grid, events.len() as f32 * 44.0 * s);
        for (row, (index, e)) in events.iter().enumerate() {
            let y = grid.top + row as f32 * 44.0 * s - offset;
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
        c.scroll_end(mark, grid);
    } else if variant == "strip" || grid.height() < 100.0 * s {
        let first = m.selected.add_days(-i64::from(
            (m.selected.weekday() + 7 - super::calendar_start(m)) % 7,
        ));
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
                    event_color(e["color"].as_u64().unwrap_or(0) as usize),
                );
            }
        }
    }
    if day_height > 0.0 {
        let viewport = Rect::new(
            r.left,
            footer.top - day_height,
            r.width(),
            day_height - 3.0 * s,
        );
        let (mark, offset) = c.scroll_start(viewport, day_events.len() as f32 * 32.0 * s);
        for (row, (index, event)) in day_events.iter().enumerate() {
            let y = viewport.top + row as f32 * 32.0 * s - offset;
            let color = event_color(event["color"].as_u64().unwrap_or(0) as usize);
            c.fill(
                Rect::new(r.left, y + 7.0 * s, 5.0 * s, 5.0 * s),
                2.0 * s,
                color,
            );
            c.label(
                Rect::new(r.left + 12.0 * s, y, r.width() - 42.0 * s, 24.0 * s),
                &format!(
                    "{} {}",
                    event["time"].as_str().unwrap_or(""),
                    event["title"].as_str().unwrap_or("Event")
                ),
                11.0,
                c.tokens.colors.text,
                400,
            );
            c.hit(
                EDIT_TASK + *index as u32,
                Rect::new(r.left, y, r.width() - 28.0 * s, 28.0 * s),
                Role::Button,
                "Edit event",
            );
            c.button(
                DELETE_TASK + *index as u32,
                Rect::new(r.right - 24.0 * s, y, 24.0 * s, 24.0 * s),
                "×",
                "Delete event",
            );
        }
        c.scroll_end(mark, viewport);
    }
    c.button(PRIMARY, footer, "+ Add event", "Add event for selected day");
}
fn media(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let title = m.data["title"].as_str().unwrap_or("");
    if title.is_empty() {
        c.empty("Nothing playing", "Play music in any media app");
        return;
    }
    let variant = m.str("variant", "card");
    let pill = m.placed.widget == "media-pill";
    let mini = pill && variant == "mini";
    let compact = pill || variant == "strip" || r.height() < 110.0 * s;
    let accent = if m.flag("tint", true) {
        m.artwork_tint.unwrap_or(c.tokens.colors.text)
    } else {
        c.tokens.colors.text
    };
    if variant == "blur"
        && !pill
        && let Some(image) = &m.artwork_backdrop
    {
        c.image(r, image, 0.42, false);
        c.fill(r, 12.0 * s, Rgba::new(0.0, 0.0, 0.0, 0.24));
    } else if m.flag("tint", true) && m.artwork_tint.is_some() {
        c.fill(r, 12.0 * s, accent.with_alpha(0.10));
    }
    if compact {
        let art = (if mini { 36.0 * s } else { 48.0 * s }).min(r.height());
        let artwork = Rect::new(r.left, r.top + (r.height() - art) / 2.0, art, art);
        if let Some(image) = &m.artwork {
            c.image(artwork, image, 1.0, false);
        } else {
            c.centered(artwork, "♫", 24.0, c.tokens.colors.text_dim, 400);
        }
        let controls = (if mini { 78.0 } else { 96.0 }) * s;
        let x = artwork.right + 8.0 * s;
        let width = (r.right - controls - x - 5.0 * s).max(1.0);
        let y = r.top + (r.height() - 42.0 * s) / 2.0;
        c.label(
            Rect::new(x, y, width, 22.0 * s),
            title,
            if mini { 12.0 } else { 14.0 },
            c.tokens.colors.text,
            600,
        );
        c.label(
            Rect::new(x, y + 23.0 * s, width, 18.0 * s),
            m.data["artist"].as_str().unwrap_or(""),
            10.0,
            c.tokens.colors.text_dim,
            400,
        );
        for (i, (hit, glyph, label)) in [
            (PREVIOUS, "‹", "Previous track"),
            (
                PRIMARY,
                if m.data["playing"].as_bool() == Some(true) {
                    "Ⅱ"
                } else {
                    "▶"
                },
                "Play or pause",
            ),
            (NEXT, "›", "Next track"),
        ]
        .into_iter()
        .enumerate()
        {
            c.button(
                hit,
                Rect::new(
                    r.right - controls + i as f32 * controls / 3.0,
                    r.top + (r.height() - 24.0 * s) / 2.0,
                    controls / 3.0 - 2.0 * s,
                    24.0 * s,
                ),
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
            c.fill(
                Rect::new(r.left, r.bottom - 2.0 * s, r.width(), 2.0 * s),
                s,
                c.tokens.colors.line,
            );
            c.fill(
                Rect::new(r.left, r.bottom - 2.0 * s, r.width() * progress, 2.0 * s),
                s,
                accent.with_alpha(0.8),
            );
        }
        return;
    }
    let art = if compact {
        r.height().min(if mini { 36.0 * s } else { 52.0 * s })
    } else {
        (r.height() - 78.0 * s).min(r.width() * 0.34).max(40.0 * s)
    };
    if let Some(image) = &m.artwork {
        c.image(Rect::new(r.left, r.top, art, art), image, 1.0, false);
    } else {
        c.fill(
            Rect::new(r.left, r.top, art, art),
            8.0 * s,
            c.tokens.colors.hover,
        );
        c.centered(
            Rect::new(r.left, r.top, art, art),
            "♫",
            26.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
    let x = r.left + art + 10.0 * s;
    let w = (r.right - x).max(1.0);
    c.label(
        Rect::new(x, r.top, w, 24.0 * s),
        title,
        if mini { 12.0 } else { 16.0 },
        c.tokens.colors.text,
        600,
    );
    c.label(
        Rect::new(x, r.top + 24.0 * s, w, 20.0 * s),
        m.data["artist"].as_str().unwrap_or(""),
        11.0,
        c.tokens.colors.text_dim,
        400,
    );
    if !compact {
        let album = m.data["album"].as_str().unwrap_or("");
        let app = m.data["app"].as_str().unwrap_or("");
        c.label(
            Rect::new(x, r.top + 46.0 * s, w, 18.0 * s),
            &format!("{album} · {app}"),
            10.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
    let y = r.bottom - if compact { 26.0 * s } else { 30.0 * s };
    let center = if compact {
        x + w / 2.0
    } else {
        r.left + r.width() / 2.0
    };
    let spacing = if mini { 30.0 } else { 42.0 };
    for (hit, dx, glyph, label) in [
        (PREVIOUS, -spacing, "‹", "Previous track"),
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
        (NEXT, spacing, "›", "Next track"),
    ] {
        c.button(
            hit,
            Rect::new(center + (dx - 13.0) * s, y, 26.0 * s, 24.0 * s),
            glyph,
            label,
        );
    }
    let position = m.data["position"].as_f64();
    let duration = m.data["duration"].as_f64();
    let advance = if m.data["playing"].as_bool() == Some(true) {
        (now.ms - m.data["_at"].as_i64().unwrap_or(now.ms)).max(0) as f64 / 1000.0
    } else {
        0.0
    };
    if m.flag("progress", true)
        && let (Some(position), Some(duration)) = (position, duration)
        && duration > 0.0
    {
        let progress = ((position + advance) / duration).clamp(0.0, 1.0) as f32;
        let track = Rect::new(
            r.left,
            if compact {
                r.bottom - 1.0 * s
            } else {
                y - 24.0 * s
            },
            r.width(),
            2.0 * s,
        );
        c.fill(track, s, c.tokens.colors.line);
        c.fill(
            Rect::new(
                track.left,
                track.top,
                track.width() * progress,
                track.height(),
            ),
            s,
            accent.with_alpha(0.8),
        );
        if !compact {
            c.label(
                Rect::new(r.left, track.top + 3.0 * s, r.width(), 16.0 * s),
                &format!(
                    "{} / {}",
                    super::media_time((position + advance).min(duration)),
                    super::media_time(duration)
                ),
                9.0,
                c.tokens.colors.text_dim,
                400,
            );
        }
    } else if !compact {
        c.label(
            Rect::new(r.left, y - 20.0 * s, r.width(), 18.0 * s),
            "Live",
            10.0,
            c.tokens.colors.text_dim,
            400,
        );
    }
    if variant == "viz" && !pill && !m.spectrum.is_empty() {
        let width = r.width() / m.spectrum.len() as f32;
        let base = y - 32.0 * s;
        let mode = m.str("viz", "bars");
        let levels: Vec<_> = m
            .spectrum
            .iter()
            .map(|v| (*v * m.num("sensitivity", 1.2) as f32).clamp(0.0, 1.0) * 32.0 * s)
            .collect();
        if mode == "wave" {
            for (i, pair) in levels.windows(2).enumerate() {
                c.line(
                    Point::new(r.left + i as f32 * width, base - pair[0]),
                    Point::new(r.left + (i + 1) as f32 * width, base - pair[1]),
                    accent,
                    1.8,
                );
            }
        } else {
            for (i, level) in levels.iter().enumerate() {
                c.fill(
                    Rect::new(
                        r.left + i as f32 * width,
                        base - level,
                        width.max(s) - s,
                        *level,
                    ),
                    s,
                    accent.with_alpha(0.7),
                );
                if mode == "mirror" {
                    c.fill(
                        Rect::new(
                            r.left + i as f32 * width,
                            base + 2.0 * s,
                            width.max(s) - s,
                            *level * 0.65,
                        ),
                        s,
                        accent.with_alpha(0.35),
                    );
                }
            }
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
    for (n, (index, (key, label, glyph, _))) in items.iter().enumerate() {
        let mut label = (*label).to_owned();
        let mut glyph = *glyph;
        if *key == "wifi"
            && let Some(network) = m.stats.as_ref().and_then(|s| s.network.as_ref())
        {
            use mypaper_widgets::stats::LinkKind;
            label = match network.kind {
                LinkKind::Wifi => format!(
                    "{} · {}%",
                    network.name.as_deref().unwrap_or("Wi-Fi"),
                    network.signal.unwrap_or(0)
                ),
                LinkKind::Ethernet => "Ethernet".into(),
                LinkKind::None => "Wi-Fi off".into(),
            };
            glyph = match network.kind {
                LinkKind::Ethernet => "\u{E839}",
                LinkKind::None => "\u{EB55}",
                LinkKind::Wifi => {
                    if network.signal.unwrap_or(0) > 66 {
                        "\u{E701}"
                    } else if network.signal.unwrap_or(0) > 33 {
                        "\u{E874}"
                    } else {
                        "\u{E872}"
                    }
                }
            };
        }
        if *key == "battery" {
            label = m.stats.as_ref().and_then(|s| s.battery).map_or_else(
                || "No battery".into(),
                |b| {
                    format!(
                        "{}%{}",
                        b.percent,
                        if b.charging { " · Charging" } else { "" }
                    )
                },
            );
        }
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
            text: Arc::from(glyph),
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
                &label,
                10.0,
                c.tokens.colors.text,
                500,
            );
        }
        c.hit(ITEM + *index as u32, rect, Role::Button, &label);
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
    let (w, h) = (
        r.width() / cols as f32,
        (r.height() / rows as f32).max(if variant == "list" {
            52.0 * s
        } else {
            (m.num("iconSize", 48.0) as f32 + 30.0) * s
        }),
    );
    let (mark, offset) = c.scroll_start(r, rows as f32 * h);
    for (i, app) in apps.iter().enumerate() {
        let cell = Rect::new(
            r.left + (i % cols) as f32 * w,
            r.top + (i / cols) as f32 * h - offset,
            w,
            h,
        );
        let label = app["label"].as_str().unwrap_or("App");
        let zoom = if variant == "dock" && m.hover == Some(ITEM + i as u32) {
            1.2
        } else {
            1.0
        };
        let size = (m.num("iconSize", 48.0) as f32 * s)
            .min(h - 18.0 * s)
            .min(w - 8.0 * s)
            .max(16.0 * s)
            * zoom;
        let icon = if cols == 1 {
            Rect::new(cell.left, cell.top + 4.0 * s, size, size)
        } else {
            Rect::new(cell.left + (w - size) / 2.0, cell.top + 4.0 * s, size, size)
        };
        let bounce = if m.action_hit == Some(ITEM + i as u32) {
            m.action_at.map_or(0.0, |at| {
                (c.instant.saturating_duration_since(at).as_secs_f32() / 0.65
                    * std::f32::consts::PI
                    * 3.0)
                    .sin()
                    .abs()
                    * 7.0
                    * s
            })
        } else {
            0.0
        };
        let icon = icon.offset(0.0, -bounce);
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
    c.scroll_end(mark, r);
}
fn weather(c: &mut Draw<'_>, m: &Model) {
    let r = c.body();
    let s = c.scale;
    let current = &m.data["current"];
    if !current.is_object() {
        let message = m.error.as_deref().unwrap_or("Finding your local forecast…");
        c.empty(
            if m.error.is_some() {
                "Weather unavailable"
            } else {
                "Weather"
            },
            message,
        );
        if m.error.is_some() {
            c.button(
                RETRY,
                Rect::new(r.left, r.bottom - 30.0 * s, r.width() * 0.45, 28.0 * s),
                "Retry",
                "Retry weather",
            );
            c.button(
                PRIVACY,
                Rect::new(
                    r.left + r.width() * 0.5,
                    r.bottom - 30.0 * s,
                    r.width() * 0.5,
                    28.0 * s,
                ),
                "Location",
                "Open Windows location privacy settings",
            );
        }
        return;
    }
    let compact = m.str("variant", "card") == "compact" || r.height() < 115.0 * s;
    c.title(
        current["location"]["name"]
            .as_str()
            .unwrap_or("Local weather"),
    );
    c.label(
        Rect::new(r.left, r.top + 22.0 * s, r.width() * 0.68, 60.0 * s),
        &format!(
            "{:.0}°{}",
            current["temperature"].as_f64().unwrap_or(0.0),
            if m.str("units", "metric") == "imperial" {
                "F"
            } else {
                "C"
            }
        ),
        if compact { 36.0 } else { 48.0 },
        c.tokens.colors.text,
        400,
    );
    c.centered(
        Rect::new(r.right - 60.0 * s, r.top + 22.0 * s, 60.0 * s, 56.0 * s),
        weather_glyph(current),
        36.0,
        c.tokens.colors.text,
        400,
    );
    c.label(
        Rect::new(
            r.left,
            r.top + if compact { 64.0 * s } else { 80.0 * s },
            r.width(),
            22.0 * s,
        ),
        current["condition"]
            .as_str()
            .unwrap_or("Current conditions"),
        12.0,
        c.tokens.colors.text_dim,
        400,
    );
    c.hit(
        PRIMARY,
        Rect::new(
            r.left,
            r.top,
            r.width(),
            if compact { r.height() } else { 104.0 * s },
        ),
        Role::Button,
        "Open weather forecast",
    );
    if !compact {
        let days = m.data["forecast"]["days"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let hourly = m.data["forecast"]["hourly"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let viewport = Rect::new(
            r.left,
            r.top + 108.0 * s,
            r.width(),
            (r.height() - 128.0 * s).max(20.0 * s),
        );
        let cols = (r.width() / (54.0 * s)).floor().max(1.0) as usize;
        let hour_rows = hourly.len().div_ceil(cols);
        let height = hour_rows as f32 * 64.0 * s + days.len() as f32 * 40.0 * s + 22.0 * s;
        let (mark, offset) = c.scroll_start(viewport, height);
        for (i, hour) in hourly.iter().enumerate() {
            let x = r.left + (i % cols) as f32 * r.width() / cols as f32;
            let y = viewport.top + (i / cols) as f32 * 64.0 * s - offset;
            let w = r.width() / cols as f32;
            c.centered(
                Rect::new(x, y, w, 16.0 * s),
                hour["time"]
                    .as_str()
                    .and_then(|t| t.get(11..16))
                    .unwrap_or(""),
                9.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.centered(
                Rect::new(x, y + 15.0 * s, w, 22.0 * s),
                &format!(
                    "{} {:.0}°",
                    weather_glyph(hour),
                    hour["temperature"].as_f64().unwrap_or(0.0)
                ),
                12.0,
                c.tokens.colors.text,
                500,
            );
            c.centered(
                Rect::new(x, y + 38.0 * s, w, 17.0 * s),
                &chance(hour),
                9.0,
                c.tokens.colors.text_dim,
                400,
            );
        }
        let y = viewport.top + hour_rows as f32 * 64.0 * s - offset;
        c.label(
            Rect::new(r.left, y, r.width(), 22.0 * s),
            "7 DAY FORECAST",
            10.0,
            c.tokens.colors.text_dim,
            600,
        );
        let low = days
            .iter()
            .filter_map(|d| d["min"].as_f64())
            .reduce(f64::min)
            .unwrap_or(0.0);
        let high = days
            .iter()
            .filter_map(|d| d["max"].as_f64())
            .reduce(f64::max)
            .unwrap_or(1.0);
        for (i, day) in days.iter().take(7).enumerate() {
            let y = y + 22.0 * s + i as f32 * 40.0 * s;
            let label = day["date"]
                .as_str()
                .and_then(Date::parse)
                .map(|d| super::time::date_label(d, false))
                .unwrap_or_default();
            c.label(
                Rect::new(r.left, y, r.width() * 0.34, 20.0 * s),
                &label,
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.label(
                Rect::new(r.left + r.width() * 0.34, y, r.width() * 0.22, 20.0 * s),
                &format!("{} {}", weather_glyph(day), chance(day)),
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            let min = day["min"].as_f64().unwrap_or(0.0);
            let max = day["max"].as_f64().unwrap_or(0.0);
            c.label(
                Rect::new(r.left + r.width() * 0.58, y, r.width() * 0.42, 20.0 * s),
                &format!("{min:.0}° / {max:.0}°"),
                11.0,
                c.tokens.colors.text,
                500,
            );
            let bar = Rect::new(
                r.left + r.width() * 0.34,
                y + 25.0 * s,
                r.width() * 0.64,
                3.0 * s,
            );
            c.fill(bar, s, c.tokens.colors.line);
            c.fill(
                Rect::new(
                    bar.left + ((min - low) / (high - low).max(1.0)) as f32 * bar.width(),
                    bar.top,
                    ((max - min) / (high - low).max(1.0)) as f32 * bar.width(),
                    bar.height(),
                ),
                s,
                Rgba::rgb8([160, 200, 250], 0.8),
            );
        }
        c.scroll_end(mark, viewport);
    }
    if let Some(error) = &m.error {
        c.label(
            Rect::new(r.left, r.bottom - 18.0 * s, r.width() - 54.0 * s, 18.0 * s),
            &format!("Stale · {error}"),
            9.0,
            c.tokens.colors.text_dim,
            400,
        );
        c.button(
            RETRY,
            Rect::new(r.right - 50.0 * s, r.bottom - 22.0 * s, 50.0 * s, 22.0 * s),
            "Retry",
            "Retry weather",
        );
    }
}
fn chance(value: &Value) -> String {
    value["precipitation_probability"]
        .as_f64()
        .map_or_else(|| "—".into(), |n| format!("{n:.0}%"))
}
fn weather_glyph(v: &Value) -> &'static str {
    let code = v["code"]
        .as_u64()
        .or_else(|| v["condition"]["code"].as_u64())
        .unwrap_or(0);
    let night = v["is_day"].as_bool() == Some(false)
        || v["icon"].as_str().is_some_and(|s| s.ends_with("night"));
    match code {
        0 | 1 if night => "☾",
        0 | 1 => "☀",
        2 | 3 => "☁",
        45 | 48 => "≋",
        71..=77 | 85 | 86 => "❄",
        95..=99 => "ϟ",
        _ => "☂",
    }
}
fn system(c: &mut Draw<'_>, m: &Model) {
    if m.stats.is_none() {
        c.empty("System", "Collecting the first sample…");
        return;
    }
    let r = c.body();
    let s = c.scale;
    let metrics = super::metrics::visible(m);
    if metrics.is_empty() {
        c.empty("No metrics selected", "Choose metrics in Studio");
        return;
    }
    let rings = m.str("variant", "rings") == "rings";
    let spark = m.str("variant", "rings") == "spark";
    let cols = if rings {
        (r.width() / (92.0 * s)).floor().max(1.0) as usize
    } else {
        1
    };
    let rows = metrics.len().div_ceil(cols);
    let h = (r.height() / rows as f32).max(if rings { 92.0 * s } else { 62.0 * s });
    let w = r.width() / cols as f32;
    let (mark, offset) = c.scroll_start(r, rows as f32 * h);
    for (i, metric) in metrics.iter().enumerate() {
        let x = r.left + (i % cols) as f32 * w;
        let y = r.top + (i / cols) as f32 * h - offset;
        let color = if !m.flag("vary", true) {
            c.tokens.colors.text
        } else if metric.id == "battery" && metric.detail == "Low" {
            Rgba::rgb8([255, 150, 135], 1.0)
        } else {
            Rgba::rgb8(
                [
                    [130, 190, 255],
                    [126, 211, 172],
                    [216, 172, 255],
                    [247, 200, 133],
                ][i % 4],
                0.9,
            )
        };
        if rings {
            let radius = (w.min(h - 34.0 * s) * 0.33).max(10.0 * s);
            let cy = y + (h - 30.0 * s) / 2.0;
            c.ring(
                Point::new(x + w / 2.0, cy),
                radius,
                metric.percent as f32 / 100.0,
                color,
                3.0,
            );
            c.centered(
                Rect::new(x, cy - 10.0 * s, w, 20.0 * s),
                if metric.id == "net" {
                    metric
                        .value
                        .split_once(' ')
                        .map_or(metric.value.as_str(), |(down, _)| down)
                } else {
                    &metric.value
                },
                14.0,
                c.tokens.colors.text,
                500,
            );
            c.centered(
                Rect::new(x, y + h - 32.0 * s, w, 16.0 * s),
                &metric.label,
                10.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.centered(
                Rect::new(x, y + h - 16.0 * s, w, 16.0 * s),
                if metric.id == "net" {
                    metric
                        .value
                        .split_once(' ')
                        .map_or(metric.detail.as_str(), |(_, up)| up)
                } else {
                    &metric.detail
                },
                9.0,
                c.tokens.colors.text_dim,
                400,
            );
        } else {
            c.label(
                Rect::new(x, y, w * 0.42, 20.0 * s),
                &metric.label,
                11.0,
                c.tokens.colors.text_dim,
                400,
            );
            c.label(
                Rect::new(x + w * 0.44, y, w * 0.56, 20.0 * s),
                &metric.value,
                13.0,
                c.tokens.colors.text,
                500,
            );
            c.label(
                Rect::new(x, y + 20.0 * s, w, 16.0 * s),
                &metric.detail,
                9.0,
                c.tokens.colors.text_dim,
                400,
            );
            let plot = Rect::new(x, y + 40.0 * s, w, h - 46.0 * s);
            if spark {
                if let Some(history) = m.history.get(&metric.id) {
                    let step = plot.width() / (history.len().max(2) - 1) as f32;
                    let axis = if metric.id == "net" {
                        history
                            .iter()
                            .copied()
                            .reduce(f64::max)
                            .unwrap_or(0.0)
                            .max(262_144.0)
                    } else {
                        100.0
                    };
                    let points: Vec<_> = history.iter().collect();
                    for (j, pair) in points.windows(2).enumerate() {
                        c.line(
                            Point::new(
                                plot.left + j as f32 * step,
                                plot.bottom - *pair[0] as f32 / axis as f32 * plot.height(),
                            ),
                            Point::new(
                                plot.left + (j + 1) as f32 * step,
                                plot.bottom - *pair[1] as f32 / axis as f32 * plot.height(),
                            ),
                            color,
                            1.3,
                        );
                    }
                }
            } else {
                let bar = Rect::new(plot.left, plot.top, plot.width(), 3.0 * s);
                c.fill(bar, s, c.tokens.colors.line);
                c.fill(
                    Rect::new(
                        bar.left,
                        bar.top,
                        bar.width() * metric.percent as f32 / 100.0,
                        bar.height(),
                    ),
                    s,
                    color,
                );
            }
        }
    }
    c.scroll_end(mark, r);
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
    let after = left <= 0;
    let up = m.str("afterwards", "countUp") == "countUp";
    let quantum = if m.flag("showSeconds", false) {
        1000
    } else {
        60_000
    };
    let seconds = if after {
        (-left) / quantum * quantum / 1000
    } else {
        (left + quantum - 1) / quantum * quantum / 1000
    };
    let units = [
        (seconds / 86400, "days"),
        (seconds / 3600 % 24, "hours"),
        (seconds / 60 % 60, "minutes"),
        (seconds % 60, "seconds"),
    ];
    let end = if m.flag("showSeconds", false) { 4 } else { 3 };
    let leading = units[..end]
        .iter()
        .position(|(n, _)| *n > 0)
        .unwrap_or(end - 1);
    let progress =
        ((now.ms - start) as f64 / (target - start).max(1) as f64).clamp(0.0, 1.0) as f32;
    let ring = m.str("style", "digits") == "ring";
    if ring {
        c.ring(
            Point::new(r.left + r.width() / 2.0, r.top + r.height() / 2.0),
            r.width().min(r.height()) * 0.43,
            progress,
            c.tokens.colors.text,
            3.0,
        );
    }
    let title = m.str("title", "");
    c.centered(
        Rect::new(r.left, r.top, r.width(), 20.0 * s),
        &if after && up && !title.is_empty() {
            format!("Since {title}")
        } else {
            title
        },
        11.0,
        c.tokens.colors.text_dim,
        500,
    );
    let stopped = after && (!up || Stamp::at(target).date == now.date);
    let hero = if stopped {
        if Stamp::at(target).date == now.date {
            "Today".into()
        } else {
            "Done".into()
        }
    } else {
        units[leading].0.to_string()
    };
    let size = ((r.height() / s - 40.0) * 0.58)
        .clamp(14.0, 56.0)
        .min(r.width() / s / (hero.len() as f32 * 0.65));
    c.centered(
        Rect::new(
            r.left,
            r.top + 20.0 * s,
            r.width(),
            (r.height() - 52.0 * s).max(18.0 * s),
        ),
        &hero,
        size,
        c.tokens.colors.text,
        400,
    );
    let secondary = units[leading + 1..end]
        .iter()
        .map(|(n, label)| format!("{n} {label}"))
        .collect::<Vec<_>>()
        .join(" · ");
    let detail = if stopped {
        "".into()
    } else {
        format!(
            "{}{}{}",
            units[leading].1,
            if after { " ago" } else { " left" },
            if secondary.is_empty() {
                String::new()
            } else {
                format!(" · {secondary}")
            }
        )
    };
    c.centered(
        Rect::new(r.left, r.bottom - 36.0 * s, r.width(), 18.0 * s),
        &detail,
        10.0,
        c.tokens.colors.text_dim,
        400,
    );
    c.centered(
        Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s),
        &super::time::date_label(Stamp::at(target).date, false),
        9.0,
        c.tokens.colors.text_dim,
        400,
    );
    if !after && !ring {
        c.fill(
            Rect::new(r.left, r.bottom - 1.0 * s, r.width() * progress, 2.0 * s),
            s,
            c.tokens.colors.text_dim,
        );
    }
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
    use super::markdown::{self, Emphasis};
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
    // Lay out complete blocks first, so the viewport knows the full scroll range.
    let mut runs = Vec::new();
    let mut blocks = Vec::new();
    let mut y = 0.0;
    for (index, raw) in m.note.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() {
            y += 8.0 * s;
            continue;
        }
        let top = y;
        let rule = (line.len() >= 3 && line.chars().all(|ch| ch == '-'))
            || (line.len() >= 3 && line.chars().all(|ch| ch == '*'));
        if rule {
            blocks.push((index, top, 12.0 * s, None, false, true));
            y += 12.0 * s;
            continue;
        }
        let heading = line.chars().take_while(|&ch| ch == '#').count();
        let heading = if (1..=3).contains(&heading) && line.as_bytes().get(heading) == Some(&b' ') {
            heading
        } else {
            0
        };
        let task = markdown::task_box(line);
        let bullet = line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ");
        let quote = line.starts_with('>');
        let value = if heading > 0 {
            &line[heading + 1..]
        } else if let Some(at) = task {
            line[at + 2..].trim_start()
        } else if bullet {
            line.trim_start()[2..].trim_start()
        } else if quote {
            line[1..].trim_start()
        } else {
            line
        };
        let size = (if heading > 0 {
            24.0 - heading as f32 * 3.0
        } else {
            14.0
        }) * m.num("fontScale", 1.0) as f32;
        let inset = if task.is_some() || bullet {
            24.0 * s
        } else if quote {
            12.0 * s
        } else {
            0.0
        };
        let mut x = inset;
        let line_height = size * s * 1.45;
        for span in markdown::inline(value) {
            let mut style = c.tokens.font.scaled(size / 14.0 * s);
            if heading > 0 || span.emphasis == Emphasis::Bold {
                style.weight = 650;
            }
            if span.emphasis == Emphasis::Code {
                style.family = "Cascadia Mono".into();
                style.size *= 0.92;
            }
            for word in span.text.split_inclusive(char::is_whitespace) {
                let width = c.text.measure(word, &style).0;
                if x > inset && x + width > r.width() {
                    y += line_height;
                    x = inset;
                }
                if width > r.width() - inset {
                    for ch in word.chars() {
                        let text = ch.to_string();
                        let width = c.text.measure(&text, &style).0;
                        if x > inset && x + width > r.width() {
                            y += line_height;
                            x = inset;
                        }
                        runs.push((x, y, text, style.clone(), span.emphasis));
                        x += width;
                    }
                } else {
                    runs.push((x, y, word.to_owned(), style.clone(), span.emphasis));
                    x += width;
                }
            }
        }
        y += line_height + 3.0 * s;
        blocks.push((
            index,
            top,
            y - top,
            task.map(|at| line.as_bytes()[at] != b' '),
            quote,
            false,
        ));
        if bullet && task.is_none() {
            runs.push((
                0.0,
                top,
                "•".into(),
                c.tokens.font.scaled(size / 14.0 * s),
                Emphasis::Plain,
            ));
        }
    }
    let (mark, offset) = c.scroll_start(r, y);
    for (index, top, height, task, quote, rule) in blocks {
        let y = r.top + top - offset;
        if quote {
            c.fill(
                Rect::new(r.left, y, 3.0 * s, height),
                s,
                c.tokens.colors.text_dim.with_alpha(0.45),
            );
        }
        if rule {
            c.line(
                Point::new(r.left, y + 5.0 * s),
                Point::new(r.right, y + 5.0 * s),
                c.tokens.colors.line,
                1.0,
            );
        }
        if let Some(checked) = task {
            let rect = Rect::new(r.left, y + 3.0 * s, 15.0 * s, 15.0 * s);
            c.stroke(rect, 3.0 * s, c.tokens.colors.text_dim, 1.0);
            if checked {
                c.centered(rect, "✓", 11.0, c.tokens.colors.text, 600);
            }
            c.hit(
                NOTE_TASK + index as u32,
                Rect::new(r.left, y, 22.0 * s, height),
                Role::CheckBox,
                "Toggle note task",
            );
            c.list.hits.last_mut().unwrap().checked = Some(checked);
        }
    }
    for (x, y, text, style, emphasis) in runs {
        let width = c.text.measure(&text, &style).0;
        let rect = Rect::new(r.left + x, r.top + y - offset, width, style.size * 1.45);
        if !rect.intersects(&r) {
            continue;
        }
        if emphasis == Emphasis::Code {
            c.fill(rect, 3.0 * s, c.tokens.colors.hover);
        }
        c.list.ops.push(Op::Emphasis {
            rect,
            text: text.into(),
            style,
            color: c.tokens.colors.text,
            italic: emphasis == Emphasis::Italic,
            strike: emphasis == Emphasis::Strike,
        });
    }
    c.scroll_end(mark, r);
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
    let compact = r.height() < 150.0 * s;
    let hide_done = m.flag("hideDone", false) || compact;
    let viewport = Rect::new(
        r.left,
        list_top,
        r.width(),
        (list_bottom - list_top).max(1.0),
    );
    let height = m.tasks.iter().filter(|t| !hide_done || !t.done).count() as f32 * 32.0 * s;
    let (mark, offset) = c.scroll_start(viewport, height);
    let mut y = list_top - offset;
    for (i, task) in m
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| !hide_done || !t.done)
    {
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
    c.scroll_end(mark, viewport);
    if !m.tasks.is_empty() {
        let bar = Rect::new(r.left, r.top + 22.0 * s, r.width(), 2.0 * s);
        c.fill(bar, s, c.tokens.colors.line);
        c.fill(
            Rect::new(
                bar.left,
                bar.top,
                bar.width() * done as f32 / m.tasks.len() as f32,
                bar.height(),
            ),
            s,
            c.tokens.colors.text_dim,
        );
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
    let viewport = Rect::new(r.left, r.top + 24.0 * s, r.width(), r.height() - 44.0 * s);
    let (mark, offset) = c.scroll_start(
        viewport,
        items.len().min(m.num("maxItems", 12.0) as usize) as f32 * h,
    );
    for (i, item) in items
        .iter()
        .take(m.num("maxItems", 12.0) as usize)
        .enumerate()
    {
        let y = viewport.top + i as f32 * h - offset;
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
            detail.push_str(&format!("  {}", relative_age(at, now)));
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
    c.scroll_end(mark, viewport);
    let failures = m.data["errors"].as_array().map_or(0, Vec::len);
    let footer = if let Some(error) = &m.error {
        format!("Offline · cached headlines · {error}")
    } else if failures > 0 {
        format!("{failures} feed(s) unavailable · showing available headlines")
    } else {
        "".into()
    };
    c.label(
        Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s),
        &footer,
        9.0,
        c.tokens.colors.text_dim,
        400,
    );
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
    let viewport = Rect::new(r.left, r.top + 24.0 * s, r.width(), r.height() - 46.0 * s);
    let h = (viewport.height() / coins.len().max(1) as f32).max(42.0 * s);
    let (mark, offset) = c.scroll_start(viewport, coins.len() as f32 * h);
    for (i, coin) in coins.iter().enumerate() {
        let y = viewport.top + i as f32 * h - offset;
        c.label(
            Rect::new(r.left, y, r.width() * 0.45, h),
            coin["name"].as_str().unwrap_or("Coin"),
            12.0,
            c.tokens.colors.text,
            500,
        );
        c.label(
            Rect::new(r.left + r.width() * 0.45, y, r.width() * 0.55, 22.0 * s),
            &price_label(
                coin["current_price"]
                    .as_f64()
                    .or_else(|| coin["price"].as_f64())
                    .unwrap_or(0.0),
                &m.str("currency", "usd"),
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
    c.scroll_end(mark, viewport);
    let at = m.data["at"].as_i64();
    let footer = format!(
        "{}{}",
        if m.error.is_some() {
            "Offline · cached · "
        } else {
            "Updated "
        },
        at.map_or_else(
            || "waiting for refresh".into(),
            |at| super::time::Stamp::at(at).clock(true, false)
        )
    );
    c.label(
        Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s),
        &footer,
        9.0,
        c.tokens.colors.text_dim,
        400,
    );
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
    let labels = m.flag("labels", true);
    let gutter = if labels { 28.0 * s } else { 0.0 };
    let plot = Rect::new(
        r.left + gutter,
        r.top + if labels { 42.0 * s } else { 26.0 * s },
        r.width() - gutter,
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
        if labels {
            if row == 0 && (date.day <= 7 || col == 0) {
                c.label(
                    Rect::new(
                        plot.left + col as f32 * cell,
                        plot.top - 16.0 * s,
                        45.0 * s,
                        16.0 * s,
                    ),
                    MONTHS[date.month as usize - 1],
                    9.0,
                    c.tokens.colors.text_dim,
                    400,
                );
            }
            if col == 0 {
                c.label(
                    Rect::new(r.left, plot.top + row as f32 * cell, gutter, cell),
                    WEEKDAYS[date.weekday() as usize],
                    8.0,
                    c.tokens.colors.text_dim,
                    400,
                );
            }
        }
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
    let content = q["text"].as_str().unwrap_or("");
    let height = (r.height() - 24.0 * s).max(1.0);
    let mut size = if r.height() > 120.0 * s { 20.0 } else { 14.0 };
    while size > 6.0 {
        let style = c.tokens.font.scaled(size / 14.0 * s);
        let lines = wrap_lines(c.text, content, &style, r.width()).len().max(1) as f32;
        if lines * size * s * 1.45 <= height {
            break;
        }
        size -= 0.5;
    }
    c.wrapped(
        Rect::new(r.left, r.top, r.width(), height),
        content,
        size,
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

fn relative_age(at: i64, now: Stamp) -> String {
    let seconds = (now.ms / 1000 - at).max(0);
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        86400..604800 => format!("{}d", seconds / 86400),
        _ => super::time::date_label(Stamp::at(at * 1000).date, false),
    }
}

pub fn price_label(price: f64, currency: &str) -> String {
    let decimals = if price > 0.0 && price < 1.0 {
        ((-price.log10()).ceil() as usize + 2).clamp(2, 10)
    } else if currency == "jpy" {
        0
    } else {
        2
    };
    let symbol = match currency {
        "usd" => "$",
        "eur" => "€",
        "gbp" => "£",
        "jpy" => "¥",
        _ => currency,
    };
    if price != 0.0 && price.abs() < 0.000_000_000_1 {
        format!("{symbol}{price:.2e}")
    } else {
        format!("{symbol}{price:.decimals$}")
    }
}

pub const EVENT_COLORS: [[u8; 3]; 6] = [
    [125, 192, 255],
    [137, 218, 175],
    [255, 175, 135],
    [210, 170, 255],
    [255, 150, 175],
    [245, 215, 120],
];
fn event_color(index: usize) -> Rgba {
    Rgba::rgb8(EVENT_COLORS[index % EVENT_COLORS.len()], 1.0)
}
fn event_editor(c: &mut Draw<'_>, m: &Model) {
    let Some(draft) = &m.event_draft else { return };
    let r = c.body();
    let s = c.scale;
    c.title("EVENT · Enter saves · Esc cancels");
    let field = Rect::new(r.left, r.top + 28.0 * s, r.width(), 30.0 * s);
    c.field(m, field, "Event title, date or optional HH:MM");
    c.button(
        EVENT_DATE,
        Rect::new(r.left, r.top + 66.0 * s, r.width() * 0.58, 28.0 * s),
        &draft.date,
        "Edit event date (YYYY-MM-DD)",
    );
    c.button(
        EVENT_TIME,
        Rect::new(
            r.left + r.width() * 0.61,
            r.top + 66.0 * s,
            r.width() * 0.39,
            28.0 * s,
        ),
        if draft.time.is_empty() {
            "All day"
        } else {
            &draft.time
        },
        "Edit optional event time (HH:MM)",
    );
    c.button(
        FIELD,
        Rect::new(r.left, r.top + 102.0 * s, r.width(), 24.0 * s),
        &format!("Title: {}", draft.title),
        "Edit event title",
    );
    for (i, _) in EVENT_COLORS.iter().enumerate() {
        let rect = Rect::new(
            r.left + i as f32 * 27.0 * s,
            r.top + 136.0 * s,
            20.0 * s,
            20.0 * s,
        );
        c.fill(rect, 10.0 * s, event_color(i));
        if draft.color == i {
            c.stroke(rect.inflate(2.0 * s), 12.0 * s, c.tokens.colors.text, 1.0);
        }
        c.hit(
            EVENT_COLOR + i as u32,
            rect,
            Role::RadioButton,
            &format!("Event color {}", i + 1),
        );
    }
    c.button(
        PRIMARY,
        Rect::new(r.left, r.bottom - 30.0 * s, r.width() * 0.46, 28.0 * s),
        "Save",
        "Save event",
    );
    c.button(
        CANCEL,
        Rect::new(
            r.left + r.width() * 0.52,
            r.bottom - 30.0 * s,
            r.width() * 0.48,
            28.0 * s,
        ),
        "Cancel",
        "Cancel event",
    );
}

fn wrap_lines(text: &mut Text, value: &str, style: &TextStyle, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in value.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && text.measure(&candidate, style).0 > width {
                out.push(std::mem::take(&mut line));
            }
            if text.measure(word, style).0 > width {
                for ch in word.chars() {
                    let candidate = format!("{line}{ch}");
                    if !line.is_empty() && text.measure(&candidate, style).0 > width {
                        out.push(std::mem::take(&mut line));
                    }
                    line.push(ch);
                }
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        out.push(line);
    }
    out
}
