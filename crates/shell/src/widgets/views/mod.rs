//! The native widgets' views, one file per widget, on the UI kit (`docs/ui.md` › "The kit").
//!
//! - **Inputs only.** Time, data and pointer state come in through the [`Model`] and [`Frame`];
//!   a view never installs a timer or fetches anything, and the same inputs give the same list,
//!   so an unchanged widget draws nothing (the surface compares lists).
//! - **Glass.** The engine draws the plate and its one edge; a view fills the plate's shape
//!   with `Rgba::HIT` (clicks land on it) and draws content only: no card under the whole
//!   widget and no outline.
//! - **Kit only.** Text through [`TextRole`]s (and [`Ctx::display`] for hero numerals), colour
//!   through [`Palette`] roles, icons through `Icon`, controls through the kit's components, and
//!   layout through `take_*`/`place`/`Stack`.
//!
//! Add a widget: a file with `pub fn draw(c: &mut Ctx, body: Rect)` and one line in
//! [`content`]'s match.

mod calendar;
mod clock;
mod countdown;
mod crypto;
mod heatmap;
mod launcher;
mod media;
mod notes;
mod photo;
mod quote;
mod rss;
mod system;
mod timer;
mod todo;
mod toggles;
mod weather;
mod integrations;
pub(crate) mod plugin;
#[cfg(test)]
mod plugin_tests;

use super::model::{Model, Widget};
use super::time::Stamp;
use mypaper_theme::Paint;
use mypaper_ui::icons::Icon;
use mypaper_ui::{
    Align, DisplayList, HitId, Interaction, Op, Palette, Point, Rect, Rgba, Role, Scheme, Style,
    Text, TextRole, TextStyle, Theme, Tokens,
};
use serde_json::Value;
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
/// The event editor's "Title: …" button, which puts the title back in the field (the field
/// itself is `FIELD`; one id per hit, or the accessibility tree is invalid).
pub const EVENT_TITLE: HitId = 16;
pub const EVENT_COLOR: HitId = 20;

/// The widgets' default family: Segoe UI Variable through its axes (the kit's ramp).
pub const DEFAULT_FAMILY: &str = mypaper_ui::VARIABLE;

/// Quick toggles: setting key, label, Segoe Fluent glyph (kept for the setting's wire format)
/// and the Settings page each opens.
pub const TOGGLES: [(&str, &str, &str, &str); 8] = [
    ("wifi", "Wi-Fi", "\u{E701}", "ms-settings:network-wifi"),
    ("bluetooth", "Bluetooth", "\u{E702}", "ms-settings:bluetooth"),
    ("focus", "Focus", "\u{E708}", "ms-settings:quiethours"),
    ("airplane", "Airplane mode", "\u{E709}", "ms-settings:network-airplanemode"),
    ("nightlight", "Night light", "\u{E706}", "ms-settings:nightlight"),
    ("sound", "Sound", "\u{E767}", "ms-settings:sound"),
    ("display", "Display", "\u{E7F4}", "ms-settings:display"),
    ("battery", "Battery", "\u{E850}", "ms-settings:batterysaver"),
];

/// Calendar event colours (stored by index in events).
pub const EVENT_COLORS: [[u8; 3]; 6] = [
    [10, 132, 255],
    [48, 209, 88],
    [255, 159, 10],
    [191, 90, 242],
    [255, 55, 95],
    [255, 214, 10],
];

/// Which edges a resize hit drags: left, top, right, bottom.
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

/// The plate's corner radius in device px: the engine's glass uses the same shape.
pub fn plate_radius(model: &Model, rect: Rect, scale: f32) -> f32 {
    if model.placed.widget == "media-pill" {
        match model.str("variant", "pill").as_str() {
            "pill" | "mini" => rect.height() / 2.0,
            _ => 14.0 * scale,
        }
    } else {
        let radius = model
            .placed
            .appearance
            .get("radius")
            .and_then(Value::as_f64)
            .unwrap_or(22.0) as f32;
        // The style shapes the corners: Windows 11's widgets are 8 px, One UI's rounder.
        let radius = match mypaper_ui::theme::active() {
            Style::Fluent => radius.min(8.0),
            Style::OneUi => radius.max(26.0),
            _ => radius,
        };
        radius * scale
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

/// The kit theme a widget draws with: Liquid, in the scheme its text tone asks for, and its
/// font (or the clock's own). The calm looks are monochrome, as the web widgets were: today's
/// disc, rings, progress and "on" take the label's ink. A colour shows only when the widget
/// asks for one (its own accent, or a lit style: depth, paper, aero, neon); data colours
/// (events, prices, warnings) are the palette's.
pub fn theme(model: &Model, tokens: &Tokens) -> Theme {
    // The notch pill is always the dark island.
    let notch = model.placed.widget == "media-pill" && model.str("variant", "pill") == "notch";
    let dark = notch || mypaper_ui::theme::luminance(tokens.colors.text) > 0.5;
    let color = |map: &serde_json::Map<String, Value>| {
        map.get("accent")
            .and_then(|v| serde_json::from_value::<Paint>(v.clone()).ok())
            .and_then(|paint| match paint {
                Paint::Color(color) => Some(Rgba::rgb8(color.0, 1.0)),
                _ => None,
            })
    };
    let lit = matches!(
        model.placed.appearance.get("style").and_then(Value::as_str),
        Some("depth" | "paper" | "aero" | "neon")
    );
    let accent = color(&model.placed.own).or_else(|| {
        lit.then(|| color(&model.placed.appearance))
            .flatten()
    });
    // The user's style (`config.json` › `look`, set by the widget host).
    let mut theme = Theme::new(
        mypaper_ui::theme::active(),
        if dark { Scheme::Dark } else { Scheme::Light },
        accent.unwrap_or(mypaper_ui::theme::SYSTEM_BLUE),
    );
    if accent.is_none() {
        let p = &mut theme.color;
        (p.accent, p.on_accent) = if dark {
            (Rgba::rgb8([242; 3], 1.0), Rgba::rgb8([17, 17, 19], 1.0))
        } else {
            (Rgba::rgb8([28, 28, 30], 1.0), Rgba::rgb8([255; 3], 1.0))
        };
        p.accent_wash = p.accent.with_alpha(if dark { 0.16 } else { 0.10 });
    }
    let clock_font = model.str("font", "");
    let family = if model.kind == Widget::Clock && !clock_font.trim().is_empty() {
        clock_font.as_str().into()
    } else {
        tokens.font.family.clone()
    };
    if family.as_ref() != DEFAULT_FAMILY {
        theme.font.family = family;
    }
    theme
}

pub fn view(model: &Model, tokens: &Tokens, frame: Frame, text: &mut Text) -> DisplayList {
    let Frame {
        size,
        scale,
        now,
        instant,
        editing,
    } = frame;
    let theme = theme(model, tokens);
    // A click focuses nothing but a field; keyboard focus (Tab, UI Automation) shows a ring.
    let input = Interaction {
        hover: model.hover,
        pressed: model.pressed,
        focus: model.focus,
        focus_visible: model.focus.is_some_and(|id| id != FIELD && id != BODY),
    };
    let rect = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let mut c = Ctx {
        f: mypaper_ui::Frame::new(text, &theme, scale, &input),
        m: model,
        now,
        instant,
        rect,
        pad: 0.0,
    };
    // The web widgets' clamp(12px, 9cqmin, 22px), a little tighter at the top.
    c.pad = c.px((rect.width().min(rect.height()) / scale * 0.09).clamp(12.0, 20.0));
    let radius = plate_radius(model, rect, scale);
    c.f.fill(rect, radius, Rgba::HIT);
    let surface = model
        .placed
        .appearance
        .get("surface")
        .and_then(Value::as_str)
        .unwrap_or("glass");
    match surface {
        "solid" => c.f.fill(rect, radius, tokens.material.tint.with_alpha(1.0)),
        "tint" => c.f.fill(rect, radius, tokens.material.tint),
        "none" => {}
        // A glass widget in a style without glass (Solid, Mono) draws the style's own body.
        _ if theme.glass.is_none() => c.f.fill(rect, radius, theme.color.surface),
        _ => {}
    }
    c.f.hit(BODY, rect, Role::Pane, &model.placed.widget, true, None);
    let body = c.body();
    content(&mut c, body);
    if let Some(message) = &model.notice {
        notice(&mut c, message);
    }
    if editing {
        edit_chrome(&mut c, radius);
    }
    let mut list = c.f.list;
    list.focus = model.focus;
    // Text straight on the wallpaper (no plate) keeps a soft shadow to stay readable.
    if surface == "none"
        && model
            .placed
            .appearance
            .get("shadow")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    {
        list.ops = text_shadows(list.ops, scale);
    }
    list
}

/// One widget's content inside its padded body. One line per widget.
fn content(c: &mut Ctx, body: Rect) {
    match c.m.kind {
        Widget::Clock => clock::draw(c, body),
        Widget::Calendar => {
            let body = integrations::calendar::controls(c, body);
            if !body.is_empty() { calendar::draw(c, body); }
        },
        Widget::Media => media::draw(c, body),
        Widget::Weather => weather::draw(c, body),
        Widget::QuickToggles => toggles::draw(c, body),
        Widget::Launcher => launcher::draw(c, body),
        Widget::System => system::draw(c, body),
        Widget::Timer => timer::draw(c, body),
        Widget::Countdown => countdown::draw(c, body),
        Widget::Photo => photo::draw(c, body),
        Widget::Notes => notes::draw(c, body),
        Widget::Todo => todo::draw(c, body),
        Widget::Rss => rss::draw(c, body),
        Widget::Crypto => crypto::draw(c, body),
        Widget::Heatmap => heatmap::draw(c, body),
        Widget::Quote => quote::draw(c, body),
        Widget::Discord => integrations::discord::view(&mut c.f, c.m, c.rect, c.now.ms),
        Widget::T3 => integrations::t3::view(&mut c.f, c.m, c.rect, c.now.ms),
        Widget::Obs => integrations::obs::view(&mut c.f, c.m, c.rect, c.now.ms),
        Widget::Docker => integrations::docker::view(&mut c.f, c.m, c.rect, c.now.ms),
        Widget::Grok => integrations::grok::view(&mut c.f, c.m, c.rect, c.now.ms),
        Widget::Custom => integrations::custom::view(&mut c.f, c.m, c.rect),
        Widget::Plugin => plugin_face(c),
    }
}

/// An SDK plugin's face from the view the host cached in `m.data` (see `widgets::sync_plugin_faces`).
fn plugin_face(c: &mut Ctx) {
    let data = &c.m.data;
    let face = serde_json::from_value::<mypaper_plugin_sdk::Face>(data["face"].clone()).ok();
    let status = match data["status"].as_str() {
        Some("live") => plugin::Status::Live,
        Some("restarting") => plugin::Status::Restarting,
        Some("failed") => plugin::Status::Failed(data["why"].as_str().unwrap_or("Unavailable")),
        _ => plugin::Status::Starting,
    };
    let title = data["title"].as_str().unwrap_or_default();
    let icon = plugin::icon(data["icon"].as_str().unwrap_or_default());
    plugin::draw(&mut c.f, c.rect, title, icon, face.as_ref(), status);
}

/// One widget's list being built: the kit's frame plus the widget's inputs.
pub struct Ctx<'a> {
    pub f: mypaper_ui::Frame<'a>,
    pub m: &'a Model,
    pub now: Stamp,
    pub instant: Instant,
    /// The whole widget, device px.
    pub rect: Rect,
    /// The content inset, device px.
    pub pad: f32,
}

impl Ctx<'_> {
    /// The theme's colour roles.
    pub fn color(&self) -> Palette {
        self.f.theme.color
    }

    pub fn s(&self) -> f32 {
        self.f.s
    }

    /// Design px to device px, on whole pixels.
    pub fn px(&self, v: f32) -> f32 {
        self.f.px(v)
    }

    /// The widget inside its padding.
    pub fn body(&self) -> Rect {
        self.rect.inflate(-self.pad)
    }

    /// The radius of a card or highlight just inside the plate (concentric across the pad).
    pub fn inner_radius(&self) -> f32 {
        let outer = plate_radius(self.m, self.rect, self.s());
        (outer - self.pad).max(self.px(6.0))
    }

    /// A hero numeral or title at `size` design px: the ramp's family, tightened as Apple
    /// tracks its display sizes.
    pub fn display(&self, size: f32, weight: u16) -> TextStyle {
        let s = self.s();
        TextStyle {
            family: self.f.theme.font.family.clone(),
            size: (size * s).round(),
            weight,
            tracking: if size >= 20.0 { -0.02 * size * s } else { 0.0 },
            optical: size * 0.75,
        }
    }

    /// `role`'s size of `text`.
    pub fn measure(&mut self, text: &str, role: TextRole) -> (f32, f32) {
        let style = self.f.font(role);
        self.f.text.measure(text, &style)
    }

    /// `text` in `rect`, aligned by `x` and centred vertically, cut to fit.
    pub fn text(&mut self, rect: Rect, text: &str, role: TextRole, color: Rgba, x: Align) -> f32 {
        self.f.text_in(rect, text, role, color, x).0
    }

    /// `text` in `style`, placed in `rect` by `x` and `y`, cut to fit. Its rect.
    pub fn styled(
        &mut self,
        rect: Rect,
        text: &str,
        style: &TextStyle,
        color: Rgba,
        x: Align,
        y: Align,
    ) -> Rect {
        let fitted = self.f.text.fit(text, style, rect.width().max(0.0));
        let (w, h) = self.f.text.measure(&fitted, style);
        let at = rect.place(w, h, x, y);
        self.f.push(Op::Text {
            rect: at,
            text: fitted,
            style: style.clone(),
            color,
        });
        at
    }

    /// A theme icon centred in `rect`.
    pub fn icon(&mut self, rect: Rect, icon: Icon, size: f32, color: Rgba) {
        self.f.icon(rect, icon, size, color);
    }

    /// A custom control's wash, focus ring and hit (drawn before its content).
    pub fn control(
        &mut self,
        id: HitId,
        rect: Rect,
        radius: f32,
        role: Role,
        label: &str,
        checked: Option<bool>,
    ) {
        self.f.wash(id, rect, radius, true);
        self.f.focus_ring(id, rect, radius);
        self.f.hit(id, rect, role, label, true, checked);
    }

    /// A plain hit (a region with no hover look of its own).
    pub fn hit(&mut self, id: HitId, rect: Rect, role: Role, label: &str) {
        self.f.hit(id, rect, role, label, true, None);
    }

    /// A vertical scroll view over `viewport` for content `height` px tall, driven by the
    /// model's scroll offset (the host routes the wheel to it). `content` draws from the top
    /// it is given; drawing and hits are cut to the viewport. A thin thumb shows while the
    /// pointer is over the widget.
    pub fn scroll(&mut self, viewport: Rect, height: f32, content: impl FnOnce(&mut Self, f32)) {
        let s = self.s();
        let max = ((height - viewport.height()) / s).max(0.0);
        self.m.scroll_max.set(max);
        self.m.scroll_page.set(viewport.height() / s);
        let offset = (self.m.scroll.clamp(0.0, max) * s).round();
        let outer = std::mem::take(&mut self.f.list.ops);
        let first_hit = self.f.list.hits.len();
        content(self, viewport.top - offset);
        let inner = std::mem::replace(&mut self.f.list.ops, outer);
        self.f.push(Op::Clipped {
            rect: viewport,
            ops: inner.into(),
        });
        let mut i = first_hit;
        while i < self.f.list.hits.len() {
            let cut = self.f.list.hits[i].rect.intersect(viewport);
            if cut.is_empty() {
                self.f.list.hits.remove(i);
            } else {
                self.f.list.hits[i].rect = cut;
                i += 1;
            }
        }
        let m = self.f.theme.metrics;
        let (w, inset) = (self.px(m.scrollbar - 1.0), self.px(2.0));
        let track = viewport.inflate(-inset);
        let thumb = if max > 0.0 {
            (track.height() * viewport.height() / height).max(self.px(24.0))
        } else {
            0.0
        };
        let top = if max > 0.0 {
            track.top + (track.height() - thumb) * (offset / s / max)
        } else {
            track.top
        };
        let shown = max > 0.0 && self.m.hover.is_some();
        let color = self.color().scrollbar;
        self.f.fill(
            Rect::new(viewport.right - w - self.px(1.0), top.round(), w, thumb.round()),
            w / 2.0,
            if shown { color } else { Rgba::TRANSPARENT },
        );
    }

    /// A header line (icon and title) when the widget is tall enough; the rest.
    pub fn header(&mut self, r: Rect, icon: Icon, title: &str) -> Rect {
        if self.rect.height() <= self.px(120.0) {
            return r;
        }
        let color = self.color();
        let (head, rest) = r.take_top(self.f.line(TextRole::Subheadline), self.px(8.0));
        let (glyph, text) = head.take_left(self.px(16.0), self.px(6.0));
        self.icon(glyph, icon, 14.0, color.accent);
        self.text(text, title, TextRole::Subheadline, color.secondary, Align::Start);
        rest
    }

    /// A footer line for errors and freshness; the rest above it.
    pub fn footer(&mut self, r: Rect, text: &str) -> Rect {
        if text.is_empty() {
            return r;
        }
        let color = self.color();
        let (foot, rest) = r.take_bottom(self.f.line(TextRole::Caption), self.px(6.0));
        self.text(foot, text, TextRole::Caption, color.tertiary, Align::Start);
        rest
    }

    /// A quiet message centred in `rect`: an icon when there is room, a title and a detail.
    pub fn empty(&mut self, rect: Rect, icon: Option<Icon>, title: &str, detail: Option<&str>) {
        let c = self.color();
        let roomy = rect.height() >= self.px(110.0);
        let icon = icon.filter(|_| roomy);
        let title_h = self.f.line(TextRole::Headline);
        let detail_style = self.f.font(TextRole::Footnote);
        let detail_line = self.f.line(TextRole::Footnote);
        let lines = detail.map_or(Vec::new(), |d| {
            let max = if roomy { 3 } else { 1 };
            self.f.text.lines(d, &detail_style, rect.width(), max)
        });
        let icon_h = icon.map_or(0.0, |_| self.px(28.0) + self.px(8.0));
        let height = icon_h + title_h + lines.len() as f32 * detail_line;
        let mut y = (rect.top + (rect.height() - height) / 2.0).round();
        if let Some(icon) = icon {
            self.icon(Rect::new(rect.left, y, rect.width(), self.px(28.0)), icon, 26.0, c.tertiary);
            y += icon_h;
        }
        self.text(
            Rect::new(rect.left, y, rect.width(), title_h),
            title,
            TextRole::Headline,
            c.secondary,
            Align::Center,
        );
        y += title_h;
        for line in lines {
            self.styled(
                Rect::new(rect.left, y, rect.width(), detail_line),
                &line,
                &detail_style,
                c.tertiary,
                Align::Center,
                Align::Center,
            );
            y += detail_line;
        }
    }
}

/// A text field: the kit's well, with the model's RichEdit field in it while one is open, else
/// the placeholder. Hit [`FIELD`].
pub fn field(c: &mut Ctx, rect: Rect, hint: &str, icon: Option<Icon>) {
    let open = c.m.field.as_ref();
    let mut inner = c.f.field_frame(rect, hint, open.is_none(), open.is_some(), icon);
    if let Some(field) = open {
        // RichEdit draws its line at the top of its rect: a one-line field gets a rect one line
        // tall, centred like the icon and the placeholder (as the launcher's search field).
        if !field.multiline() {
            let style = c.f.font(TextRole::Body);
            let (_, h) = c.f.text.measure("Ag", &style);
            let line = h.ceil().min(inner.height());
            let top = (inner.top + (inner.height() - line) / 2.0).round();
            inner = Rect { top, bottom: top + line, ..inner };
        }
        field.set_rect(inner);
        c.f.push(Op::TextField {
            rect: inner,
            field: field.clone(),
            revision: field.revision(),
        });
    }
    c.hit(FIELD, rect, Role::TextInput, hint);
}

/// A notice (a failed action, a hint) along the bottom edge, up to two lines.
fn notice(c: &mut Ctx, message: &str) {
    let color = c.color();
    let style = c.f.font(TextRole::Footnote);
    let line = c.f.line(TextRole::Footnote);
    let inset = c.px(10.0);
    let width = c.rect.width() - 2.0 * c.px(8.0);
    let lines = c.f.text.lines(message, &style, width - 2.0 * inset, 2);
    let height = lines.len().max(1) as f32 * line + 2.0 * c.px(7.0);
    let pill = Rect::new(
        c.px(8.0),
        c.rect.bottom - height - c.px(8.0),
        width,
        height,
    );
    c.f.fill(pill, c.inner_radius().min(height / 2.0), color.elevated);
    let mut y = pill.top + c.px(7.0);
    for text in lines {
        c.styled(
            Rect::new(pill.left + inset, y, pill.width() - 2.0 * inset, line),
            &text,
            &style,
            color.label,
            Align::Start,
            Align::Center,
        );
        y += line;
    }
}

/// Edit mode's controls over a widget (owned by the grid/edit work; its hit ids are the
/// hooks): duplicate and remove chips, the resize grip and the edge handles.
fn edit_chrome(c: &mut Ctx, radius: f32) {
    let s = c.s();
    let rect = c.rect;
    let color = c.color();
    c.f.push(Op::Stroke {
        rect: rect.inflate(-1.0),
        radius,
        width: 2.0 * s,
        color: color.accent.with_alpha(0.95),
    });
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
        c.f.fill(at, at.height() / 2.0, color.elevated);
        c.f.icon_button(id, at, icon, 26.0, label, false);
    }
    let handle = Rect::new(
        rect.width() - 24.0 * s,
        rect.height() - 24.0 * s,
        24.0 * s,
        24.0 * s,
    );
    c.f.push(Op::Line {
        from: Point::new(handle.left + 6.0 * s, handle.bottom - 5.0 * s),
        to: Point::new(handle.right - 5.0 * s, handle.top + 6.0 * s),
        width: 2.0 * s,
        color: color.label,
    });
    c.hit(RESIZE, handle, Role::Button, "Resize widget");
    let edge = 10.0 * s;
    for (i, (x, y, w, h)) in [
        (0.0, edge, edge, rect.height() - 2.0 * edge),
        (edge, 0.0, rect.width() - 2.0 * edge, edge),
        (rect.right - edge, edge, edge, rect.height() - 2.0 * edge),
        (edge, rect.bottom - edge, rect.width() - 2.0 * edge, edge),
        (0.0, 0.0, edge, edge),
        (rect.right - edge, 0.0, edge, edge),
        (0.0, rect.bottom - edge, edge, edge),
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

/// Every text op again under itself, one pixel down in translucent black.
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

/// A price with its currency's symbol, with enough decimals for coins under a unit.
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

/// "now", "5m", "3h", "2d", else the date.
pub fn relative_age(at: i64, now: Stamp) -> String {
    let seconds = (now.ms / 1000 - at).max(0);
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        86400..604800 => format!("{}d", seconds / 86400),
        _ => super::time::date_label(Stamp::at(at * 1000).date, false),
    }
}
