//! All native widget views. Time and data are explicit inputs; views never install a timer or
//! fetch anything. Faces are layouts of the same widget, with a single media implementation.
use super::{
    model::{Model, Widget},
    time::Stamp,
};
use mypaper_ui::calendar::{Date, MONTHS, MonthGrid, WEEKDAYS};
use mypaper_ui::icons::{self, Icon};
use mypaper_ui::interact::State;
use mypaper_ui::{
    DisplayList, Hit, HitId, Image, Ink, Interaction, Op, Point, Rect, Rgba, Role, Text, TextStyle,
    Tokens, Typeface,
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
    // The default family is the Segoe UI Variable pair (text and display cuts); a theme's or
    // the clock's own font is used at every size.
    let face = if widget_tokens.font.family.as_ref() == DEFAULT_FAMILY {
        Typeface::system(text)
    } else {
        Typeface::single(&widget_tokens.font.family)
    };
    let icon_family = icons::system_family(text);
    let mut c = Draw {
        list: DisplayList::default(),
        text,
        tokens: &widget_tokens,
        ink: Ink::of(widget_tokens.colors.text),
        face,
        icons: icon_family,
        stateful: Vec::new(),
        scale,
        rect,
        // The old kit's clamp(12px, 9cqmin, 22px): tighter in small cards, roomier in big ones.
        pad: (rect.width().min(rect.height()) / scale * 0.09).clamp(12.0, 20.0) * scale,
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
        for (i, (id, icon, label)) in [
            (DUPLICATE, Icon::Copy, "Duplicate widget"),
            (REMOVE, Icon::Close, "Remove widget"),
        ]
        .into_iter()
        .enumerate()
        {
            let at = Rect::new(
                rect.width() - (8.0 + 30.0 * (2 - i) as f32) * s,
                8.0 * s,
                26.0 * s,
                26.0 * s,
            );
            // Over any content: a solid chip, so the control reads on every widget.
            c.fill(at, 13.0 * s, Rgba::rgb8([28, 30, 36], 0.86));
            c.icon_button(id, at, icon, label);
        }
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
        .filter(|&id| id != BODY)
        .and_then(|id| c.list.hits.iter().find(|h| h.id == id))
        .cloned()
    {
        c.stroke(hit.rect.inflate(-1.0), 6.0 * scale, c.ink.secondary, 1.5);
    }
    // Rows, cells and tiles that do not draw their own state get the shared hover and press
    // washes over them; the card body never does (pressing a card's empty space is no button).
    // (A press shows only while the pointer is on the pressed item, so the hovered one is all.)
    if let Some(id) = model
        .hover
        .filter(|id| *id != BODY && !c.stateful.contains(id))
        && let Some(hit) = c.list.hits.iter().find(|h| h.id == id).cloned()
    {
        let wash = c.state(id).wash(c.tokens.colors.text);
        c.fill(hit.rect, 8.0 * scale, wash);
    }
    if let Some(message) = &model.notice {
        // A compact pill along the bottom edge, up to two lines; the card stays readable.
        let style = c.style(12.0, 400);
        let inner = rect.width() - 40.0 * scale;
        let lines = c.text.lines(message, &style, inner, 2);
        let height = (lines.len().max(1) as f32 * style.size * 1.35 + 14.0 * scale).round();
        let pill = Rect::new(
            12.0 * scale,
            rect.bottom - height - 10.0 * scale,
            rect.width() - 24.0 * scale,
            height,
        );
        c.fill(pill, 10.0 * scale, Rgba::rgb8([24, 26, 31], 0.94));
        for (i, line) in lines.iter().enumerate() {
            c.label(
                Rect::new(
                    pill.left + 8.0 * scale,
                    pill.top + 7.0 * scale + i as f32 * style.size * 1.35,
                    inner,
                    style.size * 1.35,
                ),
                line,
                12.0,
                Rgba::rgb8([245; 3], 1.0),
                400,
            );
        }
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

/// The widgets' default family: the Segoe UI Variable pair through [`Typeface::system`].
pub const DEFAULT_FAMILY: &str = "Segoe UI Variable";

/// One widget's display list, being built: shared paint helpers over the widget's tokens.
struct Draw<'a> {
    list: DisplayList,
    text: &'a mut Text,
    tokens: &'a Tokens,
    /// Contrast steps of the text colour: labels, details, hints.
    ink: Ink,
    face: Typeface,
    /// The system icon font (Segoe Fluent Icons, or MDL2 on Windows 10).
    icons: Arc<str>,
    /// Controls that drew their own hover and press state (the shared wash skips them).
    stateful: Vec<HitId>,
    scale: f32,
    rect: Rect,
    pad: f32,
    model: &'a Model,
    instant: Instant,
}
impl Draw<'_> {
    /// How control `id` looks now, from the model's pointer state.
    fn state(&self, id: HitId) -> State {
        Interaction {
            hover: self.model.hover,
            pressed: self.model.pressed,
            ..Default::default()
        }
        .state(id, true)
    }
    /// A glyph-only round control (skip, close, chevrons): no fill at rest, a wash on hover
    /// and a deeper one while pressed, the glyph sinking a little.
    fn icon_button(&mut self, id: HitId, rect: Rect, icon: Icon, label: &str) {
        let state = self.state(id);
        let radius = rect.width().min(rect.height()) / 2.0;
        self.fill(rect, radius, state.wash(self.tokens.colors.text));
        let size = (rect.width().min(rect.height()) * 0.5)
            .clamp(10.0 * self.scale, 22.0 * self.scale)
            * state.press_scale();
        let op = icons::centered(
            self.text,
            &self.icons,
            icon,
            size.round(),
            rect,
            self.ink.primary,
        );
        self.list.ops.push(op);
        self.hit(id, rect, Role::Button, label);
        self.stateful.push(id);
    }
    /// The main control of a widget (play, start): a filled disc in `fill` with the glyph in
    /// `on`, lighter on hover and deeper while pressed.
    fn primary(&mut self, id: HitId, rect: Rect, icon: Icon, label: &str, fill: Rgba, on: Rgba) {
        let state = self.state(id);
        let radius = rect.width().min(rect.height()) / 2.0;
        self.fill(rect, radius, fill);
        self.fill(
            rect,
            radius,
            match state {
                State::Hover => Rgba::new(1.0, 1.0, 1.0, 0.14),
                State::Pressed => Rgba::new(0.0, 0.0, 0.0, 0.18),
                _ => Rgba::TRANSPARENT,
            },
        );
        let size = (rect.width().min(rect.height()) * 0.42).round() * state.press_scale();
        let op = icons::centered(self.text, &self.icons, icon, size, rect, on);
        self.list.ops.push(op);
        self.hit(id, rect, Role::Button, label);
        self.stateful.push(id);
    }
    /// A glyph without a control (a status icon), centred in `rect`.
    fn glyph(&mut self, rect: Rect, icon: Icon, size: f32, color: Rgba) {
        let op = icons::centered(
            self.text,
            &self.icons,
            icon,
            (size * self.scale).round(),
            rect,
            color,
        );
        self.list.ops.push(op);
    }
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
                self.ink.secondary.with_alpha(0.4),
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
    /// The ramp's style at `size` design px (never below the 10.5 px of the old kit's caps).
    fn style(&self, size: f32, weight: u16) -> TextStyle {
        self.face.sized(size.max(10.5), weight, self.scale)
    }
    fn label(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = self.style(size, weight);
        let value = self.text.fit(value, &style, rect.width().max(1.0));
        let (width, height) = self.text.measure(&value, &style);
        let rect = Rect::new(
            rect.left.round(),
            rect.top.round(),
            width.min(rect.width()),
            height,
        );
        self.list.ops.push(Op::Text {
            rect,
            text: value,
            style,
            color,
        });
    }
    /// A label right-aligned in `rect` (values in a column).
    fn trailing(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = self.style(size, weight);
        let w = self.text.measure(value, &style).0.min(rect.width());
        self.label(
            Rect::new(rect.right - w, rect.top, w + 1.0, rect.height()),
            value,
            size,
            color,
            weight,
        );
    }
    fn centered(&mut self, rect: Rect, value: &str, size: f32, color: Rgba, weight: u16) {
        let style = self.style(size, weight);
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
        let style = self.style(size, weight);
        let height = style.size * 1.4;
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
    /// A card's title line: semibold, in the secondary step.
    fn title(&mut self, title: &str) {
        let r = self.body();
        self.label(
            Rect::new(r.left, r.top, r.width(), 20.0 * self.scale),
            title,
            13.0,
            self.ink.secondary,
            600,
        );
    }
    /// A text control (Retry, Save, timer modes): a quiet chip, brighter on hover, deeper
    /// while pressed.
    fn button(&mut self, id: HitId, rect: Rect, text: &str, label: &str) {
        let state = self.state(id);
        let ink = self.tokens.colors.text;
        let fill = match state {
            State::Hover => ink.with_alpha(0.18),
            State::Pressed => ink.with_alpha(0.26),
            _ => ink.with_alpha(0.10),
        };
        self.fill(rect, (rect.height() / 2.0).min(10.0 * self.scale), fill);
        let size = (rect.height() / self.scale * 0.48).clamp(11.0, 14.0);
        self.centered(rect, text, size, self.ink.primary, 500);
        self.hit(id, rect, Role::Button, label);
        self.stateful.push(id);
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
            self.ink.secondary,
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
                self.ink.secondary,
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
                        c.ink.secondary,
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
                    c.ink.secondary,
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
                    c.ink.secondary,
                    400,
                );
            }
        }
        _ => {
            let format24 = m.flag("format24", true);
            let value = now.clock(format24, m.flag("showSeconds", false));
            // "9:41:07 AM": the day period is set smaller, after the digits.
            let (digits, period) = match value.rsplit_once(' ') {
                Some((digits, period)) if !format24 => (digits.to_owned(), Some(period.to_owned())),
                _ => (value.clone(), None),
            };
            let show_date = m.flag("showDate", true);
            let weight = if variant == "flip" { 500 } else { 300 };
            // The largest size whose line fits the width, capped by the height.
            let width_at = |c: &mut Draw<'_>, size: f32| {
                let digits_w = c.text.measure(&digits, &c.style(size, weight)).0;
                let period_w = period.as_ref().map_or(0.0, |p| {
                    c.text.measure(p, &c.style(size * 0.36, 500)).0 + size * 0.14 * s
                });
                digits_w + period_w
            };
            let mut size =
                (r.height() / s * if show_date { 0.56 } else { 0.78 }).clamp(14.0, 132.0);
            while size > 14.0 && width_at(c, size) > r.width() {
                size -= 2.0;
            }
            let date_size = (size * 0.2).clamp(12.0, 20.0);
            let line = size * 1.2 * s;
            let date_line = if show_date { date_size * 1.45 * s } else { 0.0 };
            let top = r.top + ((r.height() - line - date_line) / 2.0).max(0.0);
            let line_w = width_at(c, size);
            let left = r.left + ((r.width() - line_w) / 2.0).max(0.0);
            if variant == "flip" {
                let count = digits.chars().count() as f32;
                let width = line_w / count.max(1.0);
                let phase = m.flip_at.map(|at| {
                    (c.instant.saturating_duration_since(at).as_secs_f32() / 0.28).clamp(0.0, 1.0)
                });
                let previous = m
                    .previous_clock
                    .map(|old| old.clock(format24, m.flag("showSeconds", false)));
                for (i, ch) in digits.chars().enumerate() {
                    let rect = Rect::new(
                        left + i as f32 * width + 1.0 * s,
                        top,
                        width - 2.0 * s,
                        line,
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
                    c.centered(rect, &shown.to_string(), size * 0.86, color, weight);
                }
            } else {
                c.label(
                    Rect::new(left, top, line_w + 2.0, line),
                    &digits,
                    size,
                    color,
                    weight,
                );
            }
            if let Some(period) = &period {
                let x = left + line_w - c.text.measure(period, &c.style(size * 0.36, 500)).0;
                // On the digits' baseline: the small cut sits low in the same line box.
                c.label(
                    Rect::new(x, top + size * 0.62 * s, r.right - x, size * 0.5 * s),
                    period,
                    size * 0.36,
                    c.ink.secondary,
                    500,
                );
            }
            if show_date {
                // The locale's long date, else "Wednesday, 4 November", else "Wed 4 Nov".
                let weekday = WEEKDAYS[now.date.weekday() as usize];
                let month = MONTHS[now.date.month as usize - 1];
                let date = [
                    super::time::date_label(now.date, true),
                    format!("{}, {} {month}", long_weekday(now.date), now.date.day),
                    format!("{weekday} {} {}", now.date.day, &month[..3]),
                ]
                .into_iter()
                .find(|d| c.text.measure(d, &c.style(date_size, 500)).0 <= r.width())
                .unwrap_or_else(|| now.date.day.to_string());
                c.centered(
                    Rect::new(r.left, top + line, r.width(), date_line),
                    &date,
                    date_size,
                    c.ink.secondary,
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
/// The calendar. By the card's size: a **tile** (today's weekday and date, as macOS's small
/// calendar), a **week strip** (short cards), or the **month** with its header controls and
/// the selected day's events; `agenda` lists what is coming.
fn calendar(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let variant = m.str("variant", "month");
    if m.event_draft.is_some() {
        event_editor(c, m);
        return;
    }
    // The one accent of the widget: today.
    let today_ink = Rgba::rgb8([255, 106, 98], 1.0);
    if variant != "agenda" && r.width() < 150.0 * s {
        let weekday = WEEKDAYS[now.date.weekday() as usize].to_uppercase();
        c.label(
            Rect::new(r.left, r.top, r.width(), 16.0 * s),
            &weekday,
            12.0,
            today_ink,
            700,
        );
        let size = ((r.height() - 34.0 * s) / s * 0.9).clamp(22.0, 64.0);
        c.label(
            Rect::new(r.left, r.top + 15.0 * s, r.width(), size * 1.3 * s),
            &now.date.day.to_string(),
            size,
            c.ink.primary,
            300,
        );
        c.label(
            Rect::new(r.left, r.bottom - 16.0 * s, r.width(), 16.0 * s),
            MONTHS[now.date.month as usize - 1],
            12.0,
            c.ink.secondary,
            500,
        );
        c.hit(TODAY, r, Role::Button, "Return to today");
        return;
    }
    let header = 26.0 * s;
    let button = 26.0 * s;
    // The title: "November 2026", else "November", else "Nov" — never an ellipsis.
    let room = r.width() - 3.0 * button - 6.0 * s;
    let full = MONTHS[m.month.month as usize - 1];
    let year = m.month.year.to_string();
    let month_name = if c.text.measure(full, &c.style(15.0, 650)).0 <= room {
        full
    } else {
        &full[..3]
    };
    let (name_w, _) = c.text.measure(month_name, &c.style(15.0, 650));
    c.label(
        Rect::new(r.left, r.top + 2.0 * s, room, header),
        month_name,
        15.0,
        c.ink.primary,
        650,
    );
    if name_w + 6.0 * s + c.text.measure(&year, &c.style(15.0, 400)).0 <= room {
        c.label(
            Rect::new(
                r.left + name_w + 6.0 * s,
                r.top + 2.0 * s,
                room - name_w,
                header,
            ),
            &year,
            15.0,
            c.ink.secondary,
            400,
        );
    }
    c.hit(
        TODAY,
        Rect::new(r.left, r.top, room, header),
        Role::Button,
        "Return to today",
    );
    for (i, (id, icon, label)) in [
        (PRIMARY, Icon::Add, "Add event for selected day"),
        (PREVIOUS, Icon::ChevronLeft, "Previous month"),
        (NEXT, Icon::ChevronRight, "Next month"),
    ]
    .into_iter()
    .enumerate()
    {
        c.icon_button(
            id,
            Rect::new(r.right - (3 - i) as f32 * button, r.top, button, button),
            icon,
            label,
        );
    }
    let body = Rect::new(
        r.left,
        r.top + header + 8.0 * s,
        r.width(),
        r.bottom - r.top - header - 8.0 * s,
    );
    if variant == "agenda" {
        agenda(c, m, now, body);
        return;
    }
    let start = match m.str("weekStart", "auto").as_str() {
        "sunday" => 0,
        "saturday" => 6,
        "monday" => 1,
        _ => super::time::week_start(),
    };
    let day_events: Vec<_> = m
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["date"].as_str() == Some(m.selected.key().as_str()))
        .collect();
    let strip = variant == "strip" || body.height() < 120.0 * s;
    let wide = body.width() / 7.0 > 34.0 * s;
    let weekday = |d: u32| -> String {
        let name = WEEKDAYS[d as usize % 7];
        if wide {
            name.to_owned()
        } else {
            name.chars().next().unwrap_or(' ').to_string()
        }
    };
    // A day: its number centred in `cell`, today on a filled disc, the selected day ringed,
    // and a dot per event colour under it.
    let day = |c: &mut Draw<'_>, cell: Rect, d: Date, size: f32| {
        let is_today = d == now.date;
        let disc = (cell.width().min(cell.height()) * 0.86).min(34.0 * s);
        let circle = Rect::new(
            cell.left + (cell.width() - disc) / 2.0,
            cell.top + (cell.height() - disc) / 2.0,
            disc,
            disc,
        );
        if is_today {
            c.fill(circle, disc / 2.0, today_ink);
        } else if d == m.selected {
            c.stroke(circle.inflate(-0.75 * s), disc / 2.0, c.ink.tertiary, 1.5);
        }
        c.stateful.push(ITEM + d.day);
        let state = c.state(ITEM + d.day);
        if !is_today && state != State::Rest {
            c.fill(circle, disc / 2.0, state.wash(c.tokens.colors.text));
        }
        c.centered(
            circle,
            &d.day.to_string(),
            size,
            if is_today {
                Rgba::rgb8([255; 3], 1.0)
            } else {
                c.ink.primary
            },
            if is_today { 650 } else { 500 },
        );
        let colors: Vec<_> = m
            .events
            .iter()
            .filter(|e| e["date"].as_str() == Some(d.key().as_str()))
            .map(|e| event_color(e["color"].as_u64().unwrap_or(0) as usize))
            .take(3)
            .collect();
        for (i, color) in colors.iter().enumerate() {
            let x = cell.left
                + cell.width() / 2.0
                + (i as f32 - (colors.len() as f32 - 1.0) / 2.0) * 6.0 * s;
            c.fill(
                Rect::new(
                    x - 2.0 * s,
                    (circle.bottom + 1.0 * s).min(cell.bottom - 4.0 * s),
                    4.0 * s,
                    4.0 * s,
                ),
                2.0 * s,
                *color,
            );
        }
        c.hit(ITEM + d.day, cell, Role::Button, &d.key());
    };
    if strip {
        let first = m
            .selected
            .add_days(-i64::from((m.selected.weekday() + 7 - start) % 7));
        let w = body.width() / 7.0;
        for i in 0..7 {
            let d = first.add_days(i);
            let x = body.left + i as f32 * w;
            c.centered(
                Rect::new(x, body.top, w, 16.0 * s),
                &weekday(d.weekday()),
                11.0,
                c.ink.tertiary,
                600,
            );
            day(
                c,
                Rect::new(
                    x,
                    body.top + 18.0 * s,
                    w,
                    (body.height() - 18.0 * s).max(20.0 * s),
                ),
                d,
                15.0,
            );
        }
        return;
    }
    let month = MonthGrid::new(m.month, start);
    let rows = month.cells.len() / 7;
    let events_height = if day_events.is_empty() {
        0.0
    } else {
        (day_events.len().min(3) as f32 * 26.0 * s + 8.0 * s).min(body.height() * 0.3)
    };
    let grid = Rect::new(
        body.left,
        body.top,
        body.width(),
        body.height() - events_height,
    );
    let w = grid.width() / 7.0;
    let head = 18.0 * s;
    let h = ((grid.height() - head) / rows as f32).min(w * 1.1);
    for col in 0..7u32 {
        c.centered(
            Rect::new(grid.left + col as f32 * w, grid.top, w, head),
            &weekday(col + start),
            11.0,
            c.ink.tertiary,
            600,
        );
    }
    let size = (h / s * 0.42).clamp(11.0, 16.0);
    for (index, date) in month.cells.iter().enumerate() {
        let Some(date) = date else { continue };
        let cell = Rect::new(
            grid.left + (index % 7) as f32 * w,
            grid.top + head + (index / 7) as f32 * h,
            w,
            h,
        );
        day(c, cell, *date, size);
    }
    if events_height > 0.0 {
        let viewport = Rect::new(
            r.left,
            grid.bottom + 6.0 * s,
            r.width(),
            events_height - 6.0 * s,
        );
        c.fill(
            Rect::new(r.left, grid.bottom + 2.0 * s, r.width(), 1.0),
            0.0,
            c.tokens.colors.line,
        );
        let (mark, offset) = c.scroll_start(viewport, day_events.len() as f32 * 26.0 * s);
        for (row, (index, event)) in day_events.iter().enumerate() {
            let y = viewport.top + row as f32 * 26.0 * s - offset;
            let color = event_color(event["color"].as_u64().unwrap_or(0) as usize);
            c.fill(
                Rect::new(r.left, y + 5.0 * s, 3.0 * s, 14.0 * s),
                1.5 * s,
                color,
            );
            let time = event["time"].as_str().unwrap_or("");
            c.label(
                Rect::new(r.left + 10.0 * s, y + 3.0 * s, 44.0 * s, 18.0 * s),
                time,
                12.0,
                c.ink.secondary,
                500,
            );
            c.label(
                Rect::new(
                    r.left + 56.0 * s,
                    y + 3.0 * s,
                    r.width() - 84.0 * s,
                    18.0 * s,
                ),
                event["title"].as_str().unwrap_or("Event"),
                13.0,
                c.ink.primary,
                500,
            );
            c.hit(
                EDIT_TASK + *index as u32,
                Rect::new(r.left, y, r.width() - 28.0 * s, 24.0 * s),
                Role::Button,
                "Edit event",
            );
            c.icon_button(
                DELETE_TASK + *index as u32,
                Rect::new(r.right - 22.0 * s, y + 1.0 * s, 22.0 * s, 22.0 * s),
                Icon::Close,
                "Delete event",
            );
        }
        c.scroll_end(mark, viewport);
    }
}

/// The calendar's agenda: upcoming events, a quiet line when there are none.
fn agenda(c: &mut Draw<'_>, m: &Model, now: Stamp, body: Rect) {
    let s = c.scale;
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
        c.centered(body, "No upcoming events", 13.0, c.ink.secondary, 500);
    }
    let (mark, offset) = c.scroll_start(body, events.len() as f32 * 44.0 * s);
    for (row, (index, e)) in events.iter().enumerate() {
        let y = body.top + row as f32 * 44.0 * s - offset;
        let color = event_color(e["color"].as_u64().unwrap_or(0) as usize);
        c.fill(
            Rect::new(body.left, y + 4.0 * s, 3.0 * s, 32.0 * s),
            1.5 * s,
            color,
        );
        let rect = Rect::new(body.left + 10.0 * s, y, body.width() - 38.0 * s, 40.0 * s);
        c.label(
            Rect::new(rect.left, y + 2.0 * s, rect.width(), 20.0 * s),
            e["title"].as_str().unwrap_or("Event"),
            13.0,
            c.ink.primary,
            600,
        );
        let date = e["date"]
            .as_str()
            .and_then(Date::parse)
            .map(|d| super::time::date_label(d, false))
            .unwrap_or_default();
        c.label(
            Rect::new(rect.left, y + 21.0 * s, rect.width(), 18.0 * s),
            &format!("{date}  {}", e["time"].as_str().unwrap_or("")),
            12.0,
            c.ink.secondary,
            400,
        );
        c.hit(EDIT_TASK + *index as u32, rect, Role::Button, "Edit event");
        c.icon_button(
            DELETE_TASK + *index as u32,
            Rect::new(body.right - 24.0 * s, y + 8.0 * s, 24.0 * s, 24.0 * s),
            Icon::Close,
            "Delete event",
        );
    }
    c.scroll_end(mark, body);
}
/// Now playing (and the media pill). The card's own box picks one of three layouts, as the
/// old widget's container queries did:
/// - **compact** (under ~130 px tall, the strip, the pill): cover | title and artist over a
///   hairline progress | transport;
/// - **row** (at least 3:2 and 130 px tall): the cover on the left, a column with the source,
///   title, artist, scrubber and transport;
/// - **column** (square and portrait): the cover on top, then the same column, centred.
///
/// Sizes follow the box between the old clamps, so a card is never mostly empty.
fn media(c: &mut Draw<'_>, m: &Model, now: Stamp) {
    let r = c.body();
    let s = c.scale;
    let title = m.data["title"].as_str().unwrap_or("");
    if title.is_empty() {
        // Nothing to control: a quiet note and one line, centred in big cards.
        if r.height() > 110.0 * s {
            let top = r.top + r.height() * 0.32;
            c.glyph(
                Rect::new(r.left, top, r.width(), 36.0 * s),
                Icon::Music,
                28.0,
                c.ink.tertiary,
            );
            c.centered(
                Rect::new(r.left, top + 42.0 * s, r.width(), 22.0 * s),
                "Nothing playing",
                15.0,
                c.ink.secondary,
                600,
            );
        } else {
            c.glyph(
                Rect::new(r.left, r.top, 36.0 * s, r.height()),
                Icon::Music,
                20.0,
                c.ink.tertiary,
            );
            c.label(
                Rect::new(
                    r.left + 44.0 * s,
                    r.top + (r.height() - 20.0 * s) / 2.0,
                    r.width() - 44.0 * s,
                    20.0 * s,
                ),
                "Nothing playing",
                14.0,
                c.ink.secondary,
                600,
            );
        }
        return;
    }
    let playing = m.data["playing"].as_bool() == Some(true);
    let artist = m.data["artist"].as_str().unwrap_or("");
    let variant = m.str("variant", "card");
    let pill = m.placed.widget == "media-pill";
    let mini = pill && variant == "mini";
    // The cover's colour, lifted so it reads as a highlight on dark glass.
    let tint = m.artwork_tint.filter(|_| m.flag("tint", true)).map(lift);
    let disc = tint.unwrap_or(c.ink.primary);
    let on_disc = if luminance(disc) > 0.55 {
        Rgba::rgb8([16, 16, 20], 1.0)
    } else {
        Rgba::rgb8([255; 3], 1.0)
    };
    if variant == "blur"
        && !pill
        && let Some(image) = &m.artwork_backdrop
    {
        let card = c.rect;
        let radius = plate_radius(m, card, s);
        c.image(card, image, 0.5, false);
        c.fill(card, radius, Rgba::new(0.0, 0.0, 0.0, 0.32));
    }
    let advance = if playing {
        (now.ms - m.data["_at"].as_i64().unwrap_or(now.ms)).max(0) as f64 / 1000.0
    } else {
        0.0
    };
    let timeline = (m.flag("progress", true))
        .then(|| {
            let duration = m.data["duration"].as_f64().filter(|d| *d > 0.0)?;
            let position = (m.data["position"].as_f64()? + advance).min(duration);
            Some((position, duration))
        })
        .flatten();
    let source = app_name(m.data["app"].as_str().unwrap_or("")).to_uppercase();
    let play_icon = if playing { Icon::Pause } else { Icon::Play };

    if !pill && r.width() < 150.0 * s {
        // A tile: the cover alone, the transport moved onto it (over artwork: a dark scrim).
        let art = r.width().min(r.height());
        let cover = Rect::new(
            r.left + (r.width() - art) / 2.0,
            r.top + (r.height() - art) / 2.0,
            art,
            art,
        );
        cover_art(c, m, cover);
        let size = (art * 0.42).clamp(26.0 * s, 44.0 * s);
        c.primary(
            PRIMARY,
            Rect::new(
                cover.left + (art - size) / 2.0,
                cover.top + (art - size) / 2.0,
                size,
                size,
            ),
            play_icon,
            "Play or pause",
            Rgba::new(0.0, 0.0, 0.0, 0.42),
            Rgba::rgb8([255; 3], 1.0),
        );
        return;
    }
    let compact = pill || variant == "strip" || r.height() < 106.0 * s;
    if compact {
        let art = (r.height()).min(if mini { 36.0 } else { 56.0 } * s);
        let cover = Rect::new(r.left, r.top + (r.height() - art) / 2.0, art, art);
        cover_art(c, m, cover);
        let disc_size = (if mini { 30.0 } else { 36.0 } * s).min(r.height());
        let skip = disc_size * 0.86;
        let room = r.width() - art;
        // Narrow: only play/pause; wider: previous, play/pause, next.
        let full = room > 230.0 * s;
        let controls = if full {
            skip * 2.0 + disc_size + 4.0 * s
        } else {
            disc_size
        };
        let cy = r.top + r.height() / 2.0;
        let mut x = r.right - controls;
        if full {
            c.icon_button(
                PREVIOUS,
                Rect::new(x, cy - skip / 2.0, skip, skip),
                Icon::Previous,
                "Previous track",
            );
            x += skip + 2.0 * s;
        }
        c.primary(
            PRIMARY,
            Rect::new(x, cy - disc_size / 2.0, disc_size, disc_size),
            play_icon,
            "Play or pause",
            disc,
            on_disc,
        );
        if full {
            c.icon_button(
                NEXT,
                Rect::new(x + disc_size + 2.0 * s, cy - skip / 2.0, skip, skip),
                Icon::Next,
                "Next track",
            );
        }
        let tx = cover.right + 12.0 * s;
        let tw = (r.right - controls - 10.0 * s - tx).max(1.0);
        let block = if mini { 34.0 } else { 40.0 } * s;
        let ty = cy - block / 2.0;
        c.label(
            Rect::new(tx, ty, tw, 20.0 * s),
            title,
            if mini { 13.0 } else { 14.0 },
            c.ink.primary,
            600,
        );
        c.label(
            Rect::new(tx, ty + if mini { 17.0 } else { 20.0 } * s, tw, 18.0 * s),
            artist,
            12.0,
            c.ink.secondary,
            400,
        );
        if let Some((position, duration)) = timeline {
            let track = Rect::new(tx, ty + block + 4.0 * s, tw, 2.0 * s);
            if track.bottom < r.bottom + 6.0 * s {
                progress(c, track, (position / duration) as f32, tint);
            }
        }
        return;
    }

    let row = r.width() >= r.height() * 1.3 && r.height() >= 106.0 * s;
    // Text and controls below or beside the cover, sized by the box (old: clamp() on cq units).
    let mut title_size = if row {
        (r.height() / s * 0.13).clamp(16.0, 26.0)
    } else {
        (r.height().min(r.width()) / s * 0.075).clamp(15.0, 24.0)
    };
    // A long title steps down (to 15 px) before it is cut with an ellipsis.
    let room = if row { r.width() * 0.5 } else { r.width() };
    while title_size > 15.0 && c.text.measure(title, &c.style(title_size, 650)).0 > room {
        title_size -= 1.0;
    }
    let artist_size = (title_size * 0.68).clamp(12.0, 16.0);
    let disc_size = ((title_size * 2.2).clamp(38.0, 50.0) * s).round();
    let skip = (disc_size * 0.82).round();
    let eyebrow = !source.is_empty() && r.height() >= 150.0 * s;
    let line = |size: f32| (size * 1.3 * s).round();
    let column_height = |with_eyebrow: bool| {
        (if with_eyebrow {
            line(10.5) + 4.0 * s
        } else {
            0.0
        }) + line(title_size)
            + line(artist_size)
            + if timeline.is_some() {
                26.0 * s
            } else {
                8.0 * s
            }
            + disc_size
    };
    let (cover, column) = if row {
        let art = r.height().min(r.width() * 0.44);
        let cover = Rect::new(r.left, r.top + (r.height() - art) / 2.0, art, art);
        let left = cover.right + (r.width() * 0.05).clamp(14.0 * s, 26.0 * s);
        let height = column_height(eyebrow).min(r.height());
        (
            cover,
            Rect::new(
                left,
                r.top + (r.height() - height) / 2.0,
                r.right - left,
                height,
            ),
        )
    } else {
        let below = column_height(false) + 12.0 * s;
        let art = (r.height() - below)
            .min(r.width())
            .clamp(48.0 * s, 360.0 * s);
        let total = art + below;
        let top = r.top + ((r.height() - total) / 2.0).max(0.0);
        (
            Rect::new(r.left + (r.width() - art) / 2.0, top, art, art),
            Rect::new(r.left, top + art + 12.0 * s, r.width(), below - 12.0 * s),
        )
    };
    cover_art(c, m, cover);
    let mut y = column.top;
    let text_line =
        |c: &mut Draw<'_>, y: f32, h: f32, value: &str, size: f32, color: Rgba, weight: u16| {
            let rect = Rect::new(column.left, y, column.width(), h);
            if row {
                c.label(rect, value, size, color, weight);
            } else {
                c.centered(rect, value, size, color, weight);
            }
        };
    if row && eyebrow {
        text_line(c, y, line(10.5), &source, 10.5, c.ink.tertiary, 650);
        y += line(10.5) + 4.0 * s;
    }
    text_line(
        c,
        y,
        line(title_size),
        title,
        title_size,
        c.ink.primary,
        650,
    );
    y += line(title_size);
    text_line(
        c,
        y,
        line(artist_size),
        artist,
        artist_size,
        c.ink.secondary,
        450,
    );
    y += line(artist_size);
    if let Some((position, duration)) = timeline {
        y += 8.0 * s;
        let times = column.width() > 200.0 * s;
        let style = c.style(10.5, 500);
        let elapsed = super::media_time(position);
        let left = format!("-{}", super::media_time(duration - position));
        let gutter = if times {
            c.text
                .measure(&left, &style)
                .0
                .max(c.text.measure(&elapsed, &style).0)
                + 8.0 * s
        } else {
            0.0
        };
        let track = Rect::new(
            column.left + gutter,
            y + 5.0 * s,
            column.width() - 2.0 * gutter,
            4.0 * s,
        );
        progress(c, track, (position / duration) as f32, tint);
        if times {
            c.label(
                Rect::new(column.left, y, gutter, 14.0 * s),
                &elapsed,
                10.5,
                c.ink.tertiary,
                500,
            );
            c.trailing(
                Rect::new(column.right - gutter, y, gutter, 14.0 * s),
                &left,
                10.5,
                c.ink.tertiary,
                500,
            );
        }
        y += 18.0 * s;
    } else {
        y += 8.0 * s;
    }
    let width = skip * 2.0 + disc_size + 16.0 * s;
    let x = if row {
        column.left - (skip - 18.0 * s) / 2.0
    } else {
        column.left + (column.width() - width) / 2.0
    };
    let cy = y + disc_size / 2.0;
    c.icon_button(
        PREVIOUS,
        Rect::new(x, cy - skip / 2.0, skip, skip),
        Icon::Previous,
        "Previous track",
    );
    c.primary(
        PRIMARY,
        Rect::new(x + skip + 8.0 * s, y, disc_size, disc_size),
        play_icon,
        "Play or pause",
        disc,
        on_disc,
    );
    c.icon_button(
        NEXT,
        Rect::new(x + skip + disc_size + 16.0 * s, cy - skip / 2.0, skip, skip),
        Icon::Next,
        "Next track",
    );
    if variant == "viz" && !pill && !m.spectrum.is_empty() {
        // A low band of bars along the card's bottom edge, under the transport.
        let band = Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s);
        let width = band.width() / m.spectrum.len() as f32;
        let gain = m.num("sensitivity", 1.2) as f32;
        let color = tint.unwrap_or(c.ink.secondary).with_alpha(0.55);
        for (i, level) in m.spectrum.iter().enumerate() {
            let h = (level * gain).clamp(0.04, 1.0) * band.height();
            c.fill(
                Rect::new(
                    band.left + i as f32 * width,
                    band.bottom - h,
                    (width - s).max(s),
                    h,
                ),
                s,
                color,
            );
        }
    }
}

/// The cover with its hairline edge (the artwork arrives with rounded corners, 12% of its
/// side: `rounded_square`); a quiet tile with a note without artwork.
fn cover_art(c: &mut Draw<'_>, m: &Model, rect: Rect) {
    let radius = rect.width() * 0.12;
    match &m.artwork {
        Some(image) => {
            c.image(rect, image, 1.0, false);
            c.stroke(
                rect.inflate(-0.5),
                radius,
                Rgba::new(1.0, 1.0, 1.0, 0.12),
                1.0,
            );
        }
        None => {
            c.fill(rect, radius, c.tokens.colors.text.with_alpha(0.10));
            let size = (rect.width() / c.scale * 0.36).clamp(14.0, 48.0);
            c.glyph(rect, Icon::Music, size, c.ink.tertiary);
        }
    }
}

/// A track with its filled part: the cover's tint, or the ink.
fn progress(c: &mut Draw<'_>, track: Rect, fraction: f32, tint: Option<Rgba>) {
    let radius = track.height() / 2.0;
    c.fill(track, radius, c.tokens.colors.text.with_alpha(0.18));
    let done = Rect::new(
        track.left,
        track.top,
        (track.width() * fraction.clamp(0.0, 1.0)).max(track.height()),
        track.height(),
    );
    c.fill(done, radius, tint.unwrap_or(c.ink.primary));
}

/// Relative luminance (sRGB weights), for picking a glyph colour on a fill.
fn luminance(c: Rgba) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// A cover's average colour as a highlight, as the old widget did: its hue and saturation at
/// 68% lightness (`hsl(var(--art-h) var(--art-s) 68%)`), so it reads on dark glass.
fn lift(c: Rgba) -> Rgba {
    let (max, min) = (c.r.max(c.g).max(c.b), c.r.min(c.g).min(c.b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let sat = if d < 1e-4 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let hue = if d < 1e-4 {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.0)
    } else if max == c.g {
        (c.b - c.r) / d + 2.0
    } else {
        (c.r - c.g) / d + 4.0
    };
    // Back from HSL at the fixed lightness.
    let (l, sat) = (0.68, sat.clamp(0.0, 0.8));
    let chroma = (1.0 - (2.0 * l - 1.0f32).abs()) * sat;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    Rgba::new(r + m, g + m, b + m, 1.0)
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
/// Weather. By the card's size: a **tile** (temperature and icon), a **row** for short cards
/// (icon, temperature, place and conditions in a line), the **regular** card (place,
/// temperature, icon, high and low), and below it, when there is room, the next hours and a
/// daily list with a range bar per day (as macOS's large weather widget).
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
            let w = (r.width() - 8.0 * s) / 2.0;
            c.button(
                RETRY,
                Rect::new(r.left, r.bottom - 28.0 * s, w, 28.0 * s),
                "Retry",
                "Retry weather",
            );
            c.button(
                PRIVACY,
                Rect::new(r.left + w + 8.0 * s, r.bottom - 28.0 * s, w, 28.0 * s),
                "Location",
                "Open Windows location privacy settings",
            );
        }
        return;
    }
    let unit = if m.str("units", "metric") == "imperial" {
        "F"
    } else {
        "C"
    };
    let temperature = format!("{:.0}°", current["temperature"].as_f64().unwrap_or(0.0));
    let place = current["location"]["name"]
        .as_str()
        .unwrap_or("Local weather");
    let condition = current["condition"].as_str().unwrap_or("");
    let days = m.data["forecast"]["days"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let range = days.first().map(|d| {
        format!(
            "H {:.0}°  L {:.0}°",
            d["max"].as_f64().unwrap_or(0.0),
            d["min"].as_f64().unwrap_or(0.0)
        )
    });
    c.hit(PRIMARY, r, Role::Button, "Open weather forecast");
    c.stateful.push(PRIMARY);
    let compact = m.str("variant", "card") == "compact" || r.height() < 110.0 * s;
    if r.width() < 150.0 * s {
        // Tile: place, icon, temperature.
        c.label(
            Rect::new(r.left, r.top, r.width(), 16.0 * s),
            place,
            12.0,
            c.ink.secondary,
            600,
        );
        let icon = (r.height() * 0.36).min(r.width() * 0.4);
        weather_icon(
            c,
            Rect::new(r.right - icon, r.top + 18.0 * s, icon, icon),
            current,
        );
        let size = ((r.height() - 20.0 * s) / s * 0.55).clamp(20.0, 44.0);
        c.label(
            Rect::new(
                r.left,
                r.bottom - size * 1.25 * s,
                r.width(),
                size * 1.3 * s,
            ),
            &temperature,
            size,
            c.ink.primary,
            300,
        );
        return;
    }
    if compact {
        // One line: icon, temperature, then place over conditions.
        let icon = r.height().min(52.0 * s);
        weather_icon(
            c,
            Rect::new(r.left, r.top + (r.height() - icon) / 2.0, icon, icon),
            current,
        );
        let size = (r.height() / s * 0.62).clamp(24.0, 44.0);
        let style = c.style(size, 300);
        let (tw, _) = c.text.measure(&temperature, &style);
        let tx = r.left + icon + 12.0 * s;
        c.label(
            Rect::new(
                tx,
                r.top + (r.height() - size * 1.3 * s) / 2.0,
                tw + 2.0,
                size * 1.3 * s,
            ),
            &temperature,
            size,
            c.ink.primary,
            300,
        );
        let x = tx + tw + 14.0 * s;
        let w = (r.right - x).max(1.0);
        let cy = r.top + r.height() / 2.0;
        c.label(
            Rect::new(x, cy - 19.0 * s, w, 18.0 * s),
            place,
            13.0,
            c.ink.primary,
            600,
        );
        c.label(
            Rect::new(x, cy + 1.0 * s, w, 18.0 * s),
            &range.clone().map_or(condition.to_owned(), |range| {
                format!("{condition} · {range}")
            }),
            12.0,
            c.ink.secondary,
            400,
        );
        return;
    }
    // Regular: the hero block, sized by the card.
    let hero = (r.height() * 0.5).clamp(84.0 * s, 150.0 * s);
    let block = Rect::new(r.left, r.top, r.width(), hero.min(r.height()));
    c.label(
        Rect::new(block.left, block.top, block.width() * 0.62, 18.0 * s),
        place,
        14.0,
        c.ink.primary,
        600,
    );
    let size = ((block.height() - 58.0 * s) / s).clamp(34.0, 72.0);
    c.label(
        Rect::new(
            block.left - 2.0 * s,
            block.top + 18.0 * s,
            block.width() * 0.62,
            size * 1.25 * s,
        ),
        &format!(
            "{temperature}{}",
            if block.width() > 260.0 * s { unit } else { "" }
        ),
        size,
        c.ink.primary,
        200,
    );
    let icon = (block.height() * 0.5)
        .min(block.width() * 0.3)
        .min(72.0 * s);
    weather_icon(
        c,
        Rect::new(block.right - icon, block.top + 4.0 * s, icon, icon),
        current,
    );
    let lines_top = block.bottom - 36.0 * s;
    c.trailing(
        Rect::new(block.left, lines_top, block.width(), 18.0 * s),
        condition,
        13.0,
        c.ink.primary,
        500,
    );
    if let Some(range) = &range {
        c.trailing(
            Rect::new(block.left, lines_top + 18.0 * s, block.width(), 18.0 * s),
            range,
            13.0,
            c.ink.secondary,
            500,
        );
    }
    let below = Rect::new(
        r.left,
        block.bottom + 10.0 * s,
        r.width(),
        r.bottom - block.bottom - 10.0 * s,
    );
    if below.height() < 44.0 * s || days.is_empty() {
        stale(c, m, r);
        return;
    }
    c.fill(
        Rect::new(r.left, block.bottom + 4.0 * s, r.width(), 1.0),
        0.0,
        c.tokens.colors.line,
    );
    let hourly = m.data["forecast"]["hourly"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut y = below.top;
    // The next hours, as columns, when there is room for them and the days.
    let cols = ((r.width() / (52.0 * s)).floor() as usize).min(hourly.len());
    if cols >= 4 && below.height() >= 150.0 * s {
        let w = r.width() / cols as f32;
        for (i, hour) in hourly.iter().take(cols).enumerate() {
            let x = r.left + i as f32 * w;
            c.centered(
                Rect::new(x, y, w, 16.0 * s),
                hour["time"]
                    .as_str()
                    .and_then(|t| t.get(11..13))
                    .unwrap_or(""),
                11.0,
                c.ink.tertiary,
                600,
            );
            weather_icon(
                c,
                Rect::new(x + (w - 24.0 * s) / 2.0, y + 18.0 * s, 24.0 * s, 24.0 * s),
                hour,
            );
            c.centered(
                Rect::new(x, y + 44.0 * s, w, 18.0 * s),
                &format!("{:.0}°", hour["temperature"].as_f64().unwrap_or(0.0)),
                13.0,
                c.ink.primary,
                500,
            );
        }
        y += 72.0 * s;
    }
    // Days: name, icon, low, a range bar against the week's span, high.
    let row = 28.0 * s;
    let fit = ((r.bottom - y) / row).floor().max(0.0) as usize;
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
    for (i, day) in days.iter().take(fit.min(7)).enumerate() {
        let y = y + i as f32 * row;
        let name = day["date"]
            .as_str()
            .and_then(Date::parse)
            .map(|d| {
                if i == 0 {
                    "Today".to_owned()
                } else {
                    WEEKDAYS[d.weekday() as usize].to_owned()
                }
            })
            .unwrap_or_default();
        c.label(
            Rect::new(r.left, y + 4.0 * s, 52.0 * s, 20.0 * s),
            &name,
            13.0,
            c.ink.primary,
            500,
        );
        weather_icon(
            c,
            Rect::new(r.left + 56.0 * s, y + 3.0 * s, 22.0 * s, 22.0 * s),
            day,
        );
        let min = day["min"].as_f64().unwrap_or(0.0);
        let max = day["max"].as_f64().unwrap_or(0.0);
        let bar_left = r.left + 124.0 * s;
        let bar = Rect::new(
            bar_left,
            y + 12.0 * s,
            r.right - 40.0 * s - bar_left,
            4.0 * s,
        );
        // Too narrow for a range bar: the low sits next to the high.
        let low_right = if bar.width() >= 36.0 * s {
            bar.left - 6.0 * s
        } else {
            r.right - 42.0 * s
        };
        c.trailing(
            Rect::new(low_right - 38.0 * s, y + 4.0 * s, 38.0 * s, 20.0 * s),
            &format!("{min:.0}°"),
            13.0,
            c.ink.secondary,
            500,
        );
        c.trailing(
            Rect::new(r.right - 36.0 * s, y + 4.0 * s, 36.0 * s, 20.0 * s),
            &format!("{max:.0}°"),
            13.0,
            c.ink.primary,
            500,
        );
        if bar.width() < 36.0 * s {
            continue;
        }
        c.fill(bar, 2.0 * s, c.tokens.colors.text.with_alpha(0.14));
        let span = (high - low).max(1.0);
        c.fill(
            Rect::new(
                bar.left + ((min - low) / span) as f32 * bar.width(),
                bar.top,
                (((max - min) / span) as f32 * bar.width()).max(bar.height()),
                bar.height(),
            ),
            2.0 * s,
            Rgba::rgb8([255, 196, 92], 0.95),
        );
    }
    stale(c, m, r);
}

/// "Stale" and a retry chip when the last refresh failed but older data is shown.
fn stale(c: &mut Draw<'_>, m: &Model, r: Rect) {
    let s = c.scale;
    if let Some(error) = &m.error {
        c.label(
            Rect::new(r.left, r.bottom - 20.0 * s, r.width() - 70.0 * s, 18.0 * s),
            &format!("Stale · {error}"),
            11.0,
            c.ink.tertiary,
            400,
        );
        c.button(
            RETRY,
            Rect::new(r.right - 62.0 * s, r.bottom - 24.0 * s, 62.0 * s, 24.0 * s),
            "Retry",
            "Retry weather",
        );
    }
}

/// What the sky looks like, from an Open-Meteo code (or a condition with one).
#[derive(Clone, Copy, PartialEq)]
enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Storm,
}

fn sky(v: &Value) -> (Sky, bool) {
    let code = v["code"]
        .as_u64()
        .or_else(|| v["condition"]["code"].as_u64())
        .unwrap_or(0);
    let night = v["is_day"].as_bool() == Some(false)
        || v["icon"].as_str().is_some_and(|s| s.ends_with("night"));
    let sky = match code {
        0 => Sky::Clear,
        1 | 2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Storm,
        _ => Sky::Rain,
    };
    (sky, night)
}

/// A drawn weather symbol filling `rect` (square): a warm sun or a pale moon, white clouds,
/// blue rain, white snow and fog lines. Flat shapes only, so it stays crisp at any size.
fn weather_icon(c: &mut Draw<'_>, rect: Rect, v: &Value) {
    let (sky, night) = sky(v);
    let u = rect.width() / 24.0;
    let at = |x: f32, y: f32| Point::new(rect.left + x * u, rect.top + y * u);
    let sun = Rgba::rgb8([255, 200, 70], 1.0);
    let moon = Rgba::rgb8([236, 232, 214], 1.0);
    let cloud = Rgba::rgb8([244, 246, 250], 0.96);
    let shade = Rgba::rgb8([196, 204, 216], 0.96);
    let rain = Rgba::rgb8([122, 182, 255], 1.0);
    let body = |c: &mut Draw<'_>, cx: f32, cy: f32, r: f32| {
        let p = at(cx, cy);
        c.fill(
            Rect::new(p.x - r * u, p.y - r * u, 2.0 * r * u, 2.0 * r * u),
            r * u,
            if night { moon } else { sun },
        );
        if !night {
            for i in 0..8 {
                let angle = i as f32 * TAU / 8.0;
                let (sin, cos) = angle.sin_cos();
                c.list.ops.push(Op::Line {
                    from: Point::new(p.x + cos * (r + 2.2) * u, p.y + sin * (r + 2.2) * u),
                    to: Point::new(p.x + cos * (r + 4.4) * u, p.y + sin * (r + 4.4) * u),
                    width: 1.8 * u,
                    color: sun,
                });
            }
        }
    };
    // A cloud: a base pill and two puffs, its left edge at x (in 24-unit space).
    let puff = |c: &mut Draw<'_>, x: f32, y: f32, scale: f32, color: Rgba| {
        let k = scale * u;
        let o = at(x, y);
        c.fill(
            Rect::new(o.x, o.y + 6.0 * k, 18.0 * k, 7.0 * k),
            3.5 * k,
            color,
        );
        c.fill(
            Rect::new(o.x + 2.5 * k, o.y + 2.5 * k, 8.0 * k, 8.0 * k),
            4.0 * k,
            color,
        );
        c.fill(
            Rect::new(o.x + 7.0 * k, o.y, 9.5 * k, 9.5 * k),
            4.75 * k,
            color,
        );
    };
    match sky {
        Sky::Clear => body(c, 12.0, 12.0, if night { 7.0 } else { 5.0 }),
        Sky::PartlyCloudy => {
            body(c, 9.0, 8.5, if night { 5.5 } else { 4.0 });
            puff(c, 5.0, 8.5, 1.0, cloud);
        }
        Sky::Cloudy => {
            puff(c, 7.0, 3.5, 0.8, shade);
            puff(c, 2.0, 8.0, 1.1, cloud);
        }
        Sky::Fog => {
            puff(c, 3.0, 2.0, 1.0, shade);
            for (i, w) in [16.0, 12.0, 16.0].into_iter().enumerate() {
                let y = 17.0 + i as f32 * 3.0;
                c.fill(
                    Rect::new(at(4.0, y).x, at(4.0, y).y, w * u, 1.6 * u),
                    0.8 * u,
                    cloud,
                );
            }
        }
        Sky::Rain | Sky::Snow | Sky::Storm => {
            puff(c, 3.0, 1.5, 1.0, cloud);
            for i in 0..3 {
                let x = 7.0 + i as f32 * 4.5;
                match sky {
                    Sky::Snow => {
                        let p = at(x, 19.5);
                        c.fill(
                            Rect::new(p.x - 1.4 * u, p.y - 1.4 * u, 2.8 * u, 2.8 * u),
                            1.4 * u,
                            cloud,
                        );
                    }
                    _ => c.list.ops.push(Op::Line {
                        from: at(x + 0.8, 16.5),
                        to: at(x - 0.8, 21.0),
                        width: 1.8 * u,
                        color: rain,
                    }),
                }
            }
            if sky == Sky::Storm {
                c.glyph(
                    Rect::new(at(9.0, 14.0).x, at(9.0, 14.0).y, 8.0 * u, 9.0 * u),
                    Icon::Charging,
                    8.0 * u / c.scale,
                    sun,
                );
            }
        }
    }
}

/// System monitor. Rings place every metric on the grid that makes them largest for the
/// card's shape (never a scroll bar for the default four); bars and sparklines give each
/// metric a row with its label and value on one line.
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
    let variant = m.str("variant", "rings");
    let primary = c.ink.primary;
    let color = |i: usize, metric: &super::metrics::Metric| {
        if !m.flag("vary", true) {
            primary
        } else if metric.id == "battery" && metric.detail == "Low" {
            Rgba::rgb8([255, 150, 135], 1.0)
        } else {
            Rgba::rgb8(
                [
                    [120, 184, 255],
                    [118, 214, 168],
                    [206, 160, 255],
                    [255, 196, 112],
                ][i % 4],
                1.0,
            )
        }
    };
    // The value shown in a ring or row: the network's download only (upload is the detail).
    let short = |metric: &super::metrics::Metric| -> String {
        if metric.id == "net" {
            metric
                .value
                .split_once(' ')
                .map_or(metric.value.clone(), |(down, _)| {
                    down.trim_start_matches(['↓', '↑']).to_owned()
                })
        } else {
            metric.value.clone()
        }
    };
    if variant == "rings" {
        let n = metrics.len();
        let labels = r.height() > 84.0 * s;
        let label_h = if labels { 34.0 * s } else { 0.0 };
        // The column count that gives the largest ring.
        let (cols, radius) = (1..=n)
            .map(|cols| {
                let rows = n.div_ceil(cols);
                let (w, h) = (r.width() / cols as f32, r.height() / rows as f32);
                let radius = ((w * 0.8).min(h - label_h) / 2.0).min(56.0 * s);
                // Empty slots (a lone ring on the last row) cost: a balanced grid wins
                // unless it is much smaller.
                let filled = (n as f32 / (rows * cols) as f32).sqrt();
                (cols, radius, radius * filled)
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(cols, radius, _)| (cols, radius))
            .unwrap_or((1, 20.0 * s));
        let rows = n.div_ceil(cols);
        let shown = if radius < 14.0 * s { cols } else { n };
        let (w, h) = (
            r.width() / cols as f32,
            r.height() / rows.min(shown.div_ceil(cols)) as f32,
        );
        let radius = radius.max(14.0 * s);
        for (i, metric) in metrics.iter().take(shown).enumerate() {
            let x = r.left + (i % cols) as f32 * w;
            let y = r.top + (i / cols) as f32 * h;
            let block = radius * 2.0 + label_h;
            let center = Point::new(x + w / 2.0, y + (h - block) / 2.0 + radius);
            let stroke = (radius / s * 0.14).clamp(3.0, 6.0);
            c.ring(
                center,
                radius - stroke * s / 2.0,
                metric.percent as f32 / 100.0,
                color(i, metric),
                stroke,
            );
            let inner = Rect::new(
                center.x - radius * 0.78,
                center.y - radius * 0.4,
                radius * 1.56,
                radius * 0.8,
            );
            let mut size = (radius / s * 0.42).clamp(11.0, 22.0);
            let value = short(metric);
            while size > 10.5 && c.text.measure(&value, &c.style(size, 600)).0 > inner.width() {
                size -= 1.0;
            }
            c.centered(inner, &value, size, c.ink.primary, 600);
            if labels {
                let top = center.y + radius + 6.0 * s;
                c.centered(
                    Rect::new(x, top, w, 16.0 * s),
                    &metric.label,
                    12.0,
                    c.ink.secondary,
                    600,
                );
                let detail = if metric.id == "net" {
                    metric
                        .value
                        .split_once(' ')
                        .map_or(String::new(), |(_, up)| up.to_owned())
                } else {
                    metric.detail.clone()
                };
                c.centered(
                    Rect::new(x, top + 15.0 * s, w, 14.0 * s),
                    &detail,
                    10.5,
                    c.ink.tertiary,
                    500,
                );
            }
        }
        return;
    }
    // Rows: label and detail left, value right, then a bar or a sparkline. A card too short
    // for a row each puts the metrics side by side instead (columns of label, value, bar).
    let spark = variant == "spark";
    let n = metrics.len();
    let fits = ((r.height() / (40.0 * s)).floor() as usize).max(1);
    // Columns are at least ~96 px; what does not fit is left out rather than squeezed.
    let most = (((r.width() + 16.0 * s) / (112.0 * s)).floor() as usize).max(1);
    let cols = if fits >= n {
        1
    } else {
        n.div_ceil(fits).min(most)
    };
    let shown = n.min(cols * fits);
    let rows = shown.div_ceil(cols);
    let w = (r.width() - (cols - 1) as f32 * 16.0 * s) / cols as f32;
    let row = if cols == 1 {
        ((r.height() / n as f32).min(if spark { 64.0 } else { 52.0 } * s)).max(30.0 * s)
    } else {
        r.height() / rows as f32
    };
    // The list sits in the middle of the card; in a row each metric's block is centred too.
    let block = 18.0 * s + 5.0 * s + 5.0 * s;
    let top = r.top + ((r.height() - rows as f32 * row) / 2.0).max(0.0);
    let inset = if cols > 1 {
        ((row - block) / 2.0).max(0.0)
    } else {
        0.0
    };
    for (i, metric) in metrics.iter().take(shown).enumerate() {
        let x = r.left + (i % cols) as f32 * (w + 16.0 * s);
        let y = top + (i / cols) as f32 * row + inset;
        let tint = color(i, metric);
        let line = 18.0 * s;
        let value_w = c.text.measure(&metric.value, &c.style(13.0, 600)).0;
        let label_w = c.text.measure(&metric.label, &c.style(13.0, 600)).0;
        c.label(
            Rect::new(x, y, w - value_w - 6.0 * s, line),
            &metric.label,
            13.0,
            c.ink.primary,
            600,
        );
        let detail_w = w - label_w - value_w - 18.0 * s;
        if cols == 1 && !metric.detail.is_empty() && detail_w > 30.0 * s {
            c.label(
                Rect::new(x + label_w + 8.0 * s, y + 1.0 * s, detail_w, line),
                &metric.detail,
                11.0,
                c.ink.tertiary,
                500,
            );
        }
        c.trailing(
            Rect::new(x, y, w, line),
            &metric.value,
            13.0,
            c.ink.primary,
            600,
        );
        let plot = Rect::new(
            x,
            y + line + 5.0 * s,
            w,
            (row - line - 12.0 * s).max(4.0 * s),
        );
        if spark && cols == 1 {
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
                        tint,
                        1.5,
                    );
                }
            }
        } else {
            let bar = Rect::new(plot.left, plot.top, plot.width(), 5.0 * s);
            c.fill(bar, 2.5 * s, c.tokens.colors.text.with_alpha(0.14));
            c.fill(
                Rect::new(
                    bar.left,
                    bar.top,
                    (bar.width() * metric.percent as f32 / 100.0).max(bar.height()),
                    bar.height(),
                ),
                2.5 * s,
                tint,
            );
        }
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
                c.stroke(rect, 6.0 * s, c.ink.secondary, 1.0);
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
        c.ink.secondary,
        500,
    );
    // Reset, start/pause (the primary disc) and skip, as the media transport.
    let disc = 36.0 * s;
    let y = r.bottom - disc;
    let x = if wide {
        center.x + radius + 12.0 * s
    } else {
        r.left + (r.width() - 2.0 * 30.0 * s - disc - 8.0 * s) / 2.0
    };
    let cy = y + disc / 2.0;
    c.icon_button(
        RESET,
        Rect::new(x, cy - 15.0 * s, 30.0 * s, 30.0 * s),
        Icon::Refresh,
        "Reset timer",
    );
    c.primary(
        PRIMARY,
        Rect::new(x + 34.0 * s, y, disc, disc),
        if m.timer.ends_at.is_some() {
            Icon::Pause
        } else {
            Icon::Play
        },
        "Start or pause timer",
        c.ink.primary,
        Rgba::rgb8([16, 16, 20], 1.0),
    );
    c.icon_button(
        NEXT,
        Rect::new(x + 38.0 * s + disc, cy - 15.0 * s, 30.0 * s, 30.0 * s),
        Icon::Next,
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
        c.ink.secondary,
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
        c.ink.secondary,
        400,
    );
    c.centered(
        Rect::new(r.left, r.bottom - 18.0 * s, r.width(), 18.0 * s),
        &super::time::date_label(Stamp::at(target).date, false),
        9.0,
        c.ink.secondary,
        400,
    );
    if !after && !ring {
        c.fill(
            Rect::new(r.left, r.bottom - 1.0 * s, r.width() * progress, 2.0 * s),
            s,
            c.ink.secondary,
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
                c.ink.secondary.with_alpha(0.45),
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
            c.stroke(rect, 3.0 * s, c.ink.secondary, 1.0);
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
        c.ink.secondary,
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
        c.stroke(check, 4.0 * s, c.ink.secondary, 1.0);
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
                    c.ink.secondary
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
        c.icon_button(
            DELETE_TASK + i as u32,
            Rect::new(row.right - 22.0 * s, row.top + 3.0 * s, 22.0 * s, 22.0 * s),
            Icon::Close,
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
            c.ink.secondary,
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
            c.ink.secondary,
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
            c.ink.secondary,
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
        c.ink.secondary,
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
        c.ink.secondary,
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
            c.ink.secondary,
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
    // Where the last month label ended: a month starting too close to it gets none.
    let mut label_end = f32::MIN;
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
            let x = plot.left + col as f32 * cell;
            if row == 0 && (date.day <= 7 || col == 0) && x >= label_end {
                let month = &MONTHS[date.month as usize - 1][..3];
                label_end = x + c.text.measure(month, &c.style(10.5, 400)).0 + 6.0 * s;
                c.label(
                    Rect::new(x, plot.top - 16.0 * s, 45.0 * s, 16.0 * s),
                    month,
                    10.5,
                    c.ink.secondary,
                    400,
                );
            }
            if col == 0 {
                c.label(
                    Rect::new(r.left, plot.top + row as f32 * cell, gutter, cell),
                    WEEKDAYS[date.weekday() as usize],
                    8.0,
                    c.ink.secondary,
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
            c.ink.secondary,
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
        c.ink.secondary,
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

/// The weekday's full English name (the widgets' fallback date forms are English, like
/// `WEEKDAYS` and `MONTHS`).
fn long_weekday(date: Date) -> &'static str {
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

/// A media source's display name from what Windows reports: a packaged app's id
/// ("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify") gives "Spotify", an executable
/// ("chrome.exe") gives "Chrome".
fn app_name(app: &str) -> String {
    let app = app.split('!').next().unwrap_or(app);
    if let Some((family, _)) = app.split_once('_') {
        // The package name's last part, up to its second capitalised word.
        let last = family.rsplit('.').next().unwrap_or(family);
        let end = last
            .char_indices()
            .skip(1)
            .find(|(_, c)| c.is_uppercase())
            .map_or(last.len(), |(i, _)| i);
        last[..end].to_owned()
    } else {
        let stem = app.rsplit(['\\', '/']).next().unwrap_or(app);
        let stem = stem.strip_suffix(".exe").unwrap_or(stem);
        let mut chars = stem.chars();
        chars
            .next()
            .map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
    }
}

#[cfg(test)]
#[test]
fn media_sources_read_as_app_names() {
    assert_eq!(
        app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
        "Spotify"
    );
    assert_eq!(app_name("chrome.exe"), "Chrome");
    assert_eq!(app_name(r"C:\Apps\foobar2000.exe"), "Foobar2000");
}
