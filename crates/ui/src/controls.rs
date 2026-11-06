//! Immediate-mode controls for app windows (Studio), drawn with the kit ([`crate::kit`]): the
//! gray tokens, the type ramp, the kit's icons, and controls built on ops and hits — buttons,
//! switches, segmented controls, sliders, selects, text-field frames, tiles, list rows,
//! section headers, scroll views, badges and keycaps, plus the panel and scrim helpers
//! palettes, sheets, popovers and toasts are drawn with.
//!
//! A frame is built by a [`Builder`]: every control appends its ops, one hit per target and a
//! [`Target`] saying what it does (`A` is the app's own action type). Input stays the app's:
//! it reads the targets of the frame on screen (`hit id - 1` is the target's index), and tells
//! the next frame what is hovered, pressed and focused through [`Input`]. Keys are stable
//! identities ([`key`]), so focus and hover survive rebuilding the list every frame.
//!
//! The look is the kit's: surfaces and controls are fills, never outlines (the only strokes
//! are the keyboard focus ring and a focused field's ring; selection is a [`Builder::plate`]). The switch, slider, segmented
//! control, list row and section header are the kit's own components ([`Frame`]), drawn with
//! [`Palette::theme`], a monochrome [`Theme`] over these grays.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use accesskit::Role;

use crate::geom::{Point, Rect, Rgba};
use crate::icons::Icon;
use crate::interact::Interaction;
use crate::kit::{Accessory, Frame, Lead, Row};
use crate::scene::{DisplayList, Hit, HitId, Op};
use crate::text::{Text, TextStyle, VARIABLE};
use crate::theme::{Scheme, Style, Theme};

/// A control's stable identity across frames.
pub type Key = u64;

/// A [`Key`] from anything hashable: a string path, a tuple of parts.
pub fn key(parts: impl Hash) -> Key {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// The gray tokens of the monochrome design (plans/native/studio-redesign.md, 8.3), retuned
/// for fills instead of outlines: each surface step is a clear fill step, hover and selection
/// are washes that read on any of them, and every text step meets the kit's floors (ink-1 7:1,
/// ink-2 and ink-3 4.5:1, ink-4 3:1 on its surfaces).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// The main panel (content area).
    pub bg: Rgba,
    /// Sidebar, title area, inspector.
    pub chrome: Rgba,
    /// Cards, popovers, sheets.
    pub panel: Rgba,
    /// The hover wash (over any surface).
    pub hover: Rgba,
    /// Current nav item, selected segment, selected row; the pressed wash.
    pub selected: Rgba,
    /// Separators inside a card.
    pub line: Rgba,
    /// A faint edge, for the rare place a fill can't separate (pictures on the same gray).
    pub edge: Rgba,
    /// A boundary that identifies a control (a hovered tile, a slider's rest track).
    pub control: Rgba,
    /// A control's resting fill: secondary buttons, selects, a switch's off track.
    pub fill: Rgba,
    /// A quieter well: text fields, a segmented control's track, keycaps.
    pub well: Rgba,
    pub ink1: Rgba,
    pub ink2: Rgba,
    pub ink3: Rgba,
    pub ink4: Rgba,
    /// Primary buttons, the switch's on track.
    pub solid: Rgba,
    pub on_solid: Rgba,
    pub solid_hover: Rgba,
    /// Behind the palette and sheets.
    pub scrim: Rgba,
    /// The one stepped shadow of popovers, sheets and toasts.
    pub shadow: Rgba,
}

fn hex(v: u32) -> Rgba {
    Rgba::rgb8([(v >> 16) as u8, (v >> 8) as u8, v as u8], 1.0)
}

const fn white(a: f32) -> Rgba {
    Rgba::new(1.0, 1.0, 1.0, a)
}

const fn black(a: f32) -> Rgba {
    Rgba::new(0.0, 0.0, 0.0, a)
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x0e0e0e),
            chrome: hex(0x171717),
            panel: hex(0x1f1f1f),
            hover: white(0.07),
            selected: white(0.12),
            line: white(0.08),
            edge: white(0.10),
            control: hex(0x8c8c8c),
            fill: white(0.10),
            well: white(0.06),
            ink1: hex(0xf5f5f5),
            ink2: hex(0xd9d9d9),
            ink3: hex(0xb3b3b3),
            ink4: hex(0x7a7a7a),
            solid: hex(0xf2f2f2),
            on_solid: hex(0x0a0a0a),
            solid_hover: hex(0xd9d9d9),
            scrim: black(0.6),
            shadow: black(0.5),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xf0f0f0),
            chrome: hex(0xe6e6e6),
            panel: hex(0xffffff),
            hover: black(0.05),
            selected: black(0.09),
            line: black(0.08),
            edge: black(0.10),
            control: hex(0x7a7a7a),
            fill: black(0.06),
            well: black(0.06),
            ink1: hex(0x0a0a0a),
            ink2: hex(0x2b2b2b),
            ink3: hex(0x525252),
            ink4: hex(0x8a8a8a),
            solid: hex(0x141414),
            on_solid: hex(0xfafafa),
            solid_hover: hex(0x383838),
            scrim: black(0.3),
            shadow: black(0.16),
        }
    }

    /// A Windows contrast theme: the tokens map onto its system colours (`window`, `text`,
    /// `highlight`, `gray`, `button`), so every surface and boundary follows it.
    pub fn contrast(window: Rgba, text: Rgba, highlight: Rgba, gray: Rgba, button: Rgba) -> Self {
        Self {
            dark: window.r + window.g + window.b < 1.5,
            bg: window,
            chrome: window,
            panel: button,
            hover: highlight.with_alpha(0.35),
            selected: highlight,
            line: text,
            edge: text,
            control: text,
            fill: button,
            well: window,
            ink1: text,
            ink2: text,
            ink3: text,
            ink4: gray,
            solid: highlight,
            on_solid: window,
            solid_hover: highlight,
            scrim: Rgba::new(0.0, 0.0, 0.0, 0.5),
            shadow: Rgba::TRANSPARENT,
        }
    }

    /// Hints and placeholders: between ink-3 and ink-4.
    pub fn hint(&self) -> Rgba {
        let (a, b) = (self.ink3, self.ink4);
        Rgba::new((a.r + b.r) / 2.0, (a.g + b.g) / 2.0, (a.b + b.b) / 2.0, 1.0)
    }

    /// The kit theme over these grays: Mono (no accent hue), its colour roles mapped onto the
    /// tokens, so the kit's components draw Studio's look.
    pub fn theme(&self) -> Theme {
        let scheme = if self.dark {
            Scheme::Dark
        } else {
            Scheme::Light
        };
        let mut t = Theme::new(Style::Mono, scheme, self.solid);
        let c = &mut t.color;
        c.label = self.ink1;
        c.secondary = self.ink3;
        c.tertiary = self.hint();
        c.quaternary = self.ink4;
        c.fill = self.fill;
        c.card = self.panel;
        c.well = self.well;
        c.separator = self.line;
        c.accent = self.solid;
        c.on_accent = self.on_solid;
        c.accent_wash = self.selected;
        c.surface = self.chrome;
        c.elevated = self.panel;
        c.hover = self.hover;
        c.pressed = self.selected;
        c.knob = if self.shadow.a > 0.0 {
            white(1.0)
        } else {
            self.ink1
        };
        c.scrollbar = self.ink1.with_alpha(0.32);
        c.shadow = self.shadow;
        t.radius.card = 12.0;
        t.radius.control = Some(8.0);
        t.elevation.color = self.shadow;
        t
    }
}

/// The type ramp, in design pixels: the kit's ([`crate::theme::TextRole`]) through Segoe UI
/// Variable's axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// 28/700: first run, the off state.
    Display,
    /// 22/700: sheet titles.
    Title1,
    /// 17/600: page bar and inspector titles.
    Title2,
    /// 14/600: section titles, emphasized labels.
    Heading,
    /// 14/400: labels, descriptions.
    Body,
    /// 14/500: buttons, sidebar items.
    Strong,
    /// 12/400: hints, details, values.
    Caption,
    /// 11/500: badges, keycaps, name tags, counts.
    Micro,
    /// 12/16 monospace: paths, commands, hex.
    Mono,
}

/// Font families and the icon font, resolved once per window.
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub text: Arc<str>,
    pub display: Arc<str>,
    pub mono: Arc<str>,
    pub icons: Arc<str>,
    /// Windows' text size (Accessibility), 1..=2.25: text and rows grow with it.
    pub text_scale: f32,
}

impl Type {
    pub fn new(text: &Text) -> Self {
        let pick = |families: &[&str]| -> Arc<str> {
            families
                .iter()
                .find(|f| text.has_family(f))
                .copied()
                .unwrap_or("Segoe UI")
                .into()
        };
        Self {
            text: pick(&["Segoe UI Variable Text", "Segoe UI"]),
            display: pick(&["Segoe UI Variable Display", "Segoe UI"]),
            mono: pick(&["Cascadia Mono", "Consolas"]),
            icons: crate::icons::system_family(text),
            text_scale: 1.0,
        }
    }

    /// `font` at `scale` device pixels per design pixel.
    pub fn style(&self, font: Font, scale: f32) -> TextStyle {
        let (size, weight, tracking) = match font {
            Font::Display => (28.0, 700, -0.6),
            Font::Title1 => (22.0, 700, -0.4),
            Font::Title2 => (17.0, 600, -0.2),
            Font::Heading => (14.0, 600, 0.0),
            Font::Body => (14.0, 400, 0.0),
            Font::Strong => (14.0, 500, 0.0),
            Font::Caption => (12.0, 400, 0.0),
            Font::Micro => (11.0, 500, 0.1),
            Font::Mono => (12.0, 400, 0.0),
        };
        let size = size * self.text_scale;
        // Segoe UI Variable through its axes (a real 500, optical size from the size); the
        // named faces where it's missing.
        let family = match font {
            Font::Mono => self.mono.clone(),
            _ if self.text.starts_with(VARIABLE) => VARIABLE.into(),
            Font::Display | Font::Title1 | Font::Title2 => self.display.clone(),
            _ => self.text.clone(),
        };
        TextStyle {
            family,
            size: size * scale,
            weight,
            tracking: tracking * scale,
            optical: size * 0.75,
        }
    }

    /// The icon font at `size` design pixels.
    pub fn icon(&self, size: f32, scale: f32) -> TextStyle {
        TextStyle::new(&self.icons, size * scale)
    }
}

/// What a control shows: one of the kit's vector icons, or an icon-font glyph (Segoe Fluent
/// Icons) where the kit has none (the caption buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Icon(Icon),
    Font(char),
}

impl From<Icon> for Glyph {
    fn from(icon: Icon) -> Self {
        Self::Icon(icon)
    }
}

impl From<char> for Glyph {
    fn from(c: char) -> Self {
        Self::Font(c)
    }
}

/// What the app's input state says about controls, for the next frame's visuals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    pub hover: Option<Key>,
    pub pressed: Option<Key>,
    pub focus: Option<Key>,
    /// The focus came from the keyboard: draw its ring.
    pub focus_visible: bool,
    pub pointer: Option<Point>,
}

/// How the app should treat pointer and keys on a target.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// Activates on release over it, or Enter/Space.
    Click,
    /// A switch or check: Space toggles.
    Toggle,
    /// A slider: dragging along `track` (or arrows, Home, End) sets a value.
    Slider {
        track: Rect,
        min: f64,
        max: f64,
        step: f64,
        value: f64,
    },
    /// A text field's frame: a click focuses the field.
    Field,
    /// Pressing starts a drag the app interprets (a stage item, a list row).
    Drag,
    /// A scroll view's thumb: dragging scrolls `view`.
    Thumb {
        view: Key,
        track: Rect,
        content: f32,
    },
}

/// What one hit of the frame does. `group` ties composite items (segments, tiles, list rows)
/// together for arrow keys.
#[derive(Debug, Clone)]
pub struct Target<A> {
    pub key: Key,
    pub action: A,
    pub behavior: Behavior,
    pub group: Option<Key>,
    /// The whole control, wherever it is scrolled.
    pub rect: Rect,
    /// The part the pointer can reach (`None`: scrolled out of its view).
    pub visible: Option<Rect>,
    /// The innermost scroll view holding it: keyboard focus scrolls it into view.
    pub scroll: Option<Key>,
}

/// A scroll view found while building: where it is and how tall its content was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollArea {
    pub key: Key,
    pub viewport: Rect,
    pub content: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// `solid` fill: the page's main action.
    Primary,
    /// The gray control fill.
    Secondary,
    /// No fill until hovered.
    Ghost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// Builds one frame: ops, hits and their targets.
pub struct Builder<'a, A> {
    pub list: DisplayList,
    pub semantics: Vec<(crate::HitId, crate::Semantics)>,
    pub text: &'a mut Text,
    pub p: &'a Palette,
    pub t: &'a Type,
    /// The kit theme over `p` ([`Palette::theme`]).
    pub kit: Theme,
    /// Device pixels per design pixel.
    pub s: f32,
    pub input: &'a Input,
    pub targets: Vec<Target<A>>,
    pub scrolls: Vec<ScrollArea>,
    clips: Vec<(Key, Rect)>,
    group: Option<Key>,
}

impl<'a, A: Clone> Builder<'a, A> {
    pub fn new(text: &'a mut Text, p: &'a Palette, t: &'a Type, s: f32, input: &'a Input) -> Self {
        Self {
            list: DisplayList::default(),
            semantics: Vec::new(),
            text,
            p,
            t,
            kit: p.theme(),
            s,
            input,
            targets: Vec::new(),
            scrolls: Vec::new(),
            clips: Vec::new(),
            group: None,
        }
    }

    /// Design pixels to device pixels.
    pub fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    pub fn font(&self, font: Font) -> TextStyle {
        self.t.style(font, self.s)
    }

    pub fn hovered(&self, key: Key) -> bool {
        self.input.hover == Some(key)
    }

    pub fn pressed(&self, key: Key) -> bool {
        self.input.pressed == Some(key)
    }

    pub fn focused(&self, key: Key) -> bool {
        self.input.focus == Some(key) && self.input.focus_visible
    }

    /// Ties the targets added by `f` into one arrow-key group.
    pub fn group<R>(&mut self, key: Key, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.group.replace(key);
        let r = f(self);
        self.group = outer;
        r
    }

    /// The innermost clip, if any.
    pub fn clip_rect(&self) -> Option<Rect> {
        self.clips.last().map(|(_, r)| *r)
    }

    /// Attach a semantic value or action to an existing control key.
    pub fn semantics(&mut self, key: Key, value: crate::Semantics) {
        if let Some(index) = self.targets.iter().position(|target| target.key == key) {
            self.semantics.push((index as u32 + 1, value));
        }
    }

    /// A stable polite live region; changing the message announces save/status feedback.
    pub fn announce(&mut self, message: &str, revision: u64) {
        self.semantics.push((
            u32::MAX - 1 - (revision % (1 << 20)) as u32,
            crate::Semantics::Status(message.into()),
        ));
    }

    /// Registers a target with its hit, clipped to the current scroll view. A control scrolled
    /// out keeps its target (the keyboard can still reach it) with an empty hit. Returns whether
    /// the pointer is over it.
    pub fn hit(
        &mut self,
        key: Key,
        rect: Rect,
        role: Role,
        label: &str,
        action: A,
        behavior: Behavior,
    ) -> bool {
        self.hit_checked(key, rect, role, label, None, action, behavior)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_checked(
        &mut self,
        key: Key,
        rect: Rect,
        role: Role,
        label: &str,
        checked: Option<bool>,
        action: A,
        behavior: Behavior,
    ) -> bool {
        let visible = match self.clips.last() {
            Some((_, clip)) => intersect(rect, *clip),
            None => Some(rect),
        };
        self.targets.push(Target {
            key,
            action,
            behavior,
            group: self.group,
            rect,
            visible,
            scroll: self.clips.last().map(|(k, _)| *k),
        });
        let id = self.targets.len() as u32;
        self.list.hits.push(Hit {
            id,
            rect: visible.unwrap_or(Rect::new(rect.left, rect.top, 0.0, 0.0)),
            role,
            label: label.into(),
            enabled: true,
            checked,
        });
        if self.input.focus == Some(key) {
            self.list.focus = Some(id);
        }
        visible.is_some() && self.hovered(key)
    }

    /// Draws kit components for the controls `keys`, which must be the next targets
    /// registered, in order: the kit sees their hover, press and keyboard focus as the hit ids
    /// they will get (`base`, `base + 1`…). Only the kit's ops are kept; the caller registers
    /// the targets. `tweak` adjusts the theme for this control.
    fn kit<R>(
        &mut self,
        keys: &[Key],
        tweak: impl FnOnce(&mut Theme),
        draw: impl FnOnce(&mut Frame, HitId) -> R,
    ) -> R {
        let base = self.targets.len() as HitId + 1;
        let id = |k: Option<Key>| {
            k.and_then(|k| keys.iter().position(|x| *x == k))
                .map(|i| base + i as HitId)
        };
        let input = Interaction {
            hover: id(self.input.hover),
            pressed: id(self.input.pressed),
            focus: id(self.input.focus),
            focus_visible: self.input.focus_visible,
        };
        let mut theme = self.kit.clone();
        tweak(&mut theme);
        let mut f = Frame::new(&mut *self.text, &theme, self.s, &input);
        let r = draw(&mut f, base);
        let ops = std::mem::take(&mut f.list.ops);
        self.list.ops.extend(ops);
        r
    }

    // --- Primitives ---------------------------------------------------------------------------

    pub fn fill(&mut self, rect: Rect, radius: f32, color: Rgba) {
        if color.a > 0.0 && !rect.is_empty() {
            self.list.ops.push(Op::Fill {
                rect,
                radius,
                color,
            });
        }
    }

    pub fn stroke(&mut self, rect: Rect, radius: f32, width: f32, color: Rgba) {
        self.list.ops.push(Op::Stroke {
            rect: rect.inflate(-width / 2.0),
            radius: (radius - width / 2.0).max(0.0),
            width,
            color,
        });
    }

    pub fn line(&mut self, from: Point, to: Point, width: f32, color: Rgba) {
        self.list.ops.push(Op::Line {
            from,
            to,
            width,
            color,
        });
    }

    /// A horizontal 1 px divider.
    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, color: Rgba) {
        let y = y.floor() + 0.5;
        self.line(Point::new(x0, y), Point::new(x1, y), 1.0, color);
    }

    /// A vertical 1 px divider.
    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, color: Rgba) {
        let x = x.floor() + 0.5;
        self.line(Point::new(x, y0), Point::new(x, y1), 1.0, color);
    }

    /// The measured size of `text` in `style`.
    pub fn measure(&mut self, text: &str, style: &TextStyle) -> (f32, f32) {
        self.text.measure(text, style)
    }

    /// Text placed in `rect`: aligned horizontally, centered vertically, shortened with an
    /// ellipsis when it doesn't fit. Returns its drawn width.
    pub fn text_in(
        &mut self,
        rect: Rect,
        text: &str,
        style: &TextStyle,
        color: Rgba,
        align: Align,
    ) -> f32 {
        if text.is_empty() || rect.width() <= 0.0 {
            return 0.0;
        }
        let shown = self.text.fit(text, style, rect.width());
        let (w, h) = self.text.measure(&shown, style);
        let x = match align {
            Align::Start => rect.left,
            Align::Center => rect.left + (rect.width() - w) / 2.0,
            Align::End => rect.right - w,
        };
        let y = rect.top + (rect.height() - h) / 2.0;
        self.list.ops.push(Op::Text {
            rect: Rect::new(x.round(), y.round(), w, h),
            text: shown,
            style: style.clone(),
            color,
        });
        w
    }

    /// Text with its top-left at `(x, y)`; returns its size.
    pub fn text_at(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        style: &TextStyle,
        color: Rgba,
    ) -> (f32, f32) {
        let (w, h) = self.text.measure(text, style);
        if !text.is_empty() {
            self.list.ops.push(Op::Text {
                rect: Rect::new(x.round(), y.round(), w, h),
                text: text.into(),
                style: style.clone(),
                color,
            });
        }
        (w, h)
    }

    /// Word-wrapped text from `(x, y)` within `width`, `line` pixels per line; returns the
    /// height used.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: &str,
        style: &TextStyle,
        line: f32,
        color: Rgba,
    ) -> f32 {
        let lines = self.text.lines(text, style, width, usize::MAX);
        for (i, l) in lines.iter().enumerate() {
            let (_, h) = self.text.measure(l, style);
            let top = y + i as f32 * line + (line - h) / 2.0;
            self.text_at(x, top, l, style, color);
        }
        lines.len() as f32 * line
    }

    /// An icon centered in `rect`, about `size` design pixels: the kit's vector (a touch
    /// larger, as Lucide's outlines sit inside their box) or a font glyph.
    pub fn icon(&mut self, rect: Rect, glyph: impl Into<Glyph>, size: f32, color: Rgba) {
        match glyph.into() {
            Glyph::Icon(icon) => {
                let px = ((size + 1.0).max(13.0) * self.s).round();
                let at = Rect::new(
                    (rect.left + (rect.width() - px) / 2.0).round(),
                    (rect.top + (rect.height() - px) / 2.0).round(),
                    px,
                    px,
                );
                let weight = (self.kit.icon_weight * px / 24.0).max(1.35 * self.s.min(1.5));
                self.list.ops.push(Op::Icon {
                    rect: at,
                    icon,
                    weight,
                    color,
                });
            }
            Glyph::Font(c) => {
                let style = self.t.icon(size, self.s);
                let mut buf = [0u8; 4];
                let glyph = c.encode_utf8(&mut buf);
                let (w, h) = self.text.measure(glyph, &style);
                self.list.ops.push(Op::Text {
                    rect: Rect::new(
                        (rect.left + (rect.width() - w) / 2.0).round(),
                        (rect.top + (rect.height() - h) / 2.0).round(),
                        w,
                        h,
                    ),
                    text: (&*glyph).into(),
                    style,
                    color,
                });
            }
        }
    }

    /// The 2 px focus ring with a 2 px gap around `rect`, when `key` has keyboard focus.
    pub fn focus_ring(&mut self, key: Key, rect: Rect, radius: f32) {
        if self.focused(key) {
            let gap = self.px(2.0);
            self.stroke(
                rect.inflate(gap + self.px(2.0)),
                radius + gap + self.px(2.0),
                self.px(2.0),
                self.p.ink1,
            );
        }
    }

    /// The hover (or pressed) wash of `key` over `rect`.
    pub fn wash(&mut self, key: Key, rect: Rect, radius: f32) {
        if self.pressed(key) && self.hovered(key) {
            self.fill(rect, radius, self.p.selected);
        } else if self.hovered(key) {
            self.fill(rect, radius, self.p.hover);
        }
    }

    /// A popover, palette, sheet or toast body: the stepped shadow and the panel. No edge:
    /// the shadow (and the scrim behind modals) sets it apart.
    pub fn elevated(&mut self, rect: Rect, radius: f32) {
        let s = self.s;
        self.list.ops.push(Op::Shadow {
            rect,
            radius,
            spread: 24.0 * s,
            drop: 8.0 * s,
            color: self.p.shadow.with_alpha(self.p.shadow.a / 6.0),
        });
        self.fill(rect, radius, self.p.panel);
    }

    /// The scrim behind a modal, over `rect`. It swallows the pointer (`dismiss` on click).
    pub fn scrim(&mut self, key: Key, rect: Rect, dismiss: A) {
        self.fill(rect, 0.0, self.p.scrim);
        self.hit(key, rect, Role::Pane, "", dismiss, Behavior::Click);
    }

    // --- Controls -----------------------------------------------------------------------------

    /// A button: an optional icon before its label.
    #[allow(clippy::too_many_arguments)]
    pub fn button(
        &mut self,
        key: Key,
        rect: Rect,
        label: &str,
        icon: Option<Glyph>,
        kind: ButtonKind,
        enabled: bool,
        action: A,
    ) {
        let r = self.px(8.0).min(rect.height() / 2.0);
        let p = *self.p;
        let (fill, ink) = match kind {
            ButtonKind::Primary => (
                if enabled && self.hovered(key) {
                    p.solid_hover
                } else {
                    p.solid
                },
                p.on_solid,
            ),
            ButtonKind::Secondary => (p.fill, p.ink1),
            ButtonKind::Ghost => (
                Rgba::TRANSPARENT,
                if self.hovered(key) { p.ink1 } else { p.ink2 },
            ),
        };
        let (fill, ink) = if enabled {
            (fill, ink)
        } else {
            (p.well, p.ink4)
        };
        self.fill(rect, r, fill);
        if enabled && kind != ButtonKind::Primary {
            self.wash(key, rect, r);
        }
        let style = self.font(Font::Strong);
        let (tw, _) = self.text.measure(label, &style);
        let gap = if icon.is_some() && !label.is_empty() {
            self.px(6.0)
        } else {
            0.0
        };
        let iw = if icon.is_some() { self.px(16.0) } else { 0.0 };
        let total = tw + gap + iw;
        let mut x = rect.left + ((rect.width() - total) / 2.0).max(self.px(8.0));
        if let Some(glyph) = icon {
            self.icon(Rect::new(x, rect.top, iw, rect.height()), glyph, 15.0, ink);
            x += iw + gap;
        }
        self.text_in(
            Rect::new(x, rect.top, rect.right - x - self.px(6.0), rect.height()),
            label,
            &style,
            ink,
            Align::Start,
        );
        if enabled {
            self.hit(key, rect, Role::Button, label, action, Behavior::Click);
        }
        self.focus_ring(key, rect, r);
    }

    /// The width a button needs for `label` (and an icon).
    pub fn button_width(&mut self, label: &str, icon: bool, small: bool) -> f32 {
        let style = self.font(Font::Strong);
        let (tw, _) = self.text.measure(label, &style);
        let pad = if small { 10.0 } else { 14.0 };
        let icon = if icon {
            16.0 + if label.is_empty() { 0.0 } else { 6.0 }
        } else {
            0.0
        };
        (tw + self.px(pad * 2.0 + icon)).ceil()
    }

    /// A square ghost button with an icon; `on` keeps it lit (a toggled tool).
    #[allow(clippy::too_many_arguments)]
    pub fn icon_button(
        &mut self,
        key: Key,
        rect: Rect,
        glyph: impl Into<Glyph>,
        label: &str,
        on: bool,
        enabled: bool,
        action: A,
    ) {
        let r = self.px(8.0).min(rect.height() / 2.0);
        let hot = enabled && self.hovered(key);
        if on {
            self.fill(rect, r, self.p.selected);
        } else if enabled {
            self.wash(key, rect, r);
        }
        let ink = if !enabled {
            self.p.ink4
        } else if hot || on {
            self.p.ink1
        } else {
            self.p.ink2
        };
        self.icon(rect, glyph, 15.0, ink);
        if enabled {
            self.hit(key, rect, Role::Button, label, action, Behavior::Click);
        }
        self.focus_ring(key, rect, r);
    }

    /// The kit's switch with its top-left near `at` (the 36×20 box it replaces: the kit's
    /// 40×24 shares its right edge and centre). `label` names it for assistive tech.
    pub fn switch(&mut self, key: Key, at: Point, on: bool, label: &str, action: A) -> Rect {
        let right = at.x + self.px(36.0);
        let mid = at.y + self.px(10.0);
        // On a dark scheme the on track is near white, so its knob is dark.
        let knob = (on && self.p.dark && self.p.shadow.a > 0.0).then_some(self.p.on_solid);
        let rect = self.kit(
            &[key],
            |t| {
                if let Some(k) = knob {
                    t.color.knob = k;
                }
            },
            |f, id| f.switch(id, right, mid, on, label, true),
        );
        self.hit_checked(
            key,
            rect.inflate(self.px(4.0)),
            Role::Switch,
            label,
            Some(on),
            action,
            Behavior::Toggle,
        );
        rect
    }

    /// The kit's segmented control: equal segments across `rect` when `fill`, else as wide as
    /// the widest label needs, at `rect`'s right. `action(i)` picks segment `i`.
    #[allow(clippy::too_many_arguments)]
    pub fn segmented(
        &mut self,
        key: Key,
        rect: Rect,
        items: &[&str],
        selected: Option<usize>,
        label: &str,
        fill: bool,
        action: impl Fn(usize) -> A,
    ) -> Rect {
        let n = items.len().max(1) as f32;
        let outer = if fill {
            rect
        } else {
            let w = self.segmented_width(items);
            Rect::new(rect.right - w, rect.top, w, rect.height())
        };
        let keys: Vec<Key> = (0..items.len())
            .map(|i| crate::controls::key((key, i)))
            .collect();
        let inset = (2.0 * self.s).round();
        let inner = outer.inflate(-inset);
        // The kit's equal segments, unless a label wouldn't fit one: then each segment is as
        // wide as its label needs, drawn the kit's way.
        let style = self.kit.font.style(crate::theme::TextRole::Callout, self.s);
        let widths: Vec<f32> = items
            .iter()
            .map(|s| self.text.measure(s, &style).0 + self.px(14.0))
            .collect();
        let equal = inner.width() / n;
        let segs: Vec<Rect> = if widths.iter().all(|w| *w <= equal) {
            (0..items.len())
                .map(|i| {
                    Rect::new(
                        (inner.left + equal * i as f32).round(),
                        inner.top,
                        equal.round(),
                        inner.height(),
                    )
                })
                .collect()
        } else {
            let extra = (inner.width() - widths.iter().sum::<f32>()) / n;
            let mut x = inner.left;
            widths
                .iter()
                .map(|w| {
                    let r = Rect::new(x.round(), inner.top, (w + extra).round(), inner.height());
                    x += w + extra;
                    r
                })
                .collect()
        };
        let proportional = segs
            .first()
            .is_some_and(|r| (r.width() - equal.round()).abs() > 0.5);
        if proportional {
            self.segments_drawn(&keys, outer, &segs, items, selected);
        } else {
            self.kit(
                &keys,
                |_| {},
                |f, base| {
                    f.segmented(base, outer, items, selected.unwrap_or(usize::MAX));
                },
            );
        }
        self.group(key, |b| {
            for (i, item) in items.iter().enumerate() {
                let seg = segs[i];
                b.hit_checked(
                    keys[i],
                    seg,
                    Role::RadioButton,
                    &format!("{label}: {item}"),
                    Some(selected == Some(i)),
                    action(i),
                    Behavior::Click,
                );
            }
        });
        outer
    }

    /// Segments of unequal widths in the kit's look (its segmented control has equal ones):
    /// the well, the selected segment raised, washes, labels and focus rings.
    fn segments_drawn(
        &mut self,
        keys: &[Key],
        outer: Rect,
        segs: &[Rect],
        items: &[&str],
        selected: Option<usize>,
    ) {
        let c = self.kit.color;
        let radius = self.px(8.0).min(outer.height() / 2.0);
        let inner_radius = (radius - (2.0 * self.s).round()).max(0.0);
        self.fill(outer, radius, c.well);
        let style = self.kit.font.style(crate::theme::TextRole::Callout, self.s);
        for (i, (seg, item)) in segs.iter().zip(items).enumerate() {
            let on = selected == Some(i);
            if on {
                self.fill(
                    *seg,
                    inner_radius,
                    if self.p.dark { c.fill } else { c.knob },
                );
            }
            self.wash(keys[i], *seg, inner_radius);
            let ink = if on { c.label } else { c.secondary };
            self.text_in(seg.inflate(-self.px(4.0)), item, &style, ink, Align::Center);
            self.focus_ring(keys[i], *seg, inner_radius);
        }
    }

    /// The width a segmented control needs for `items` (equal segments, the widest label).
    pub fn segmented_width(&mut self, items: &[&str]) -> f32 {
        let style = self.kit.font.style(crate::theme::TextRole::Callout, self.s);
        let widest = items
            .iter()
            .map(|s| self.text.measure(s, &style).0)
            .fold(0.0, f32::max);
        ((widest + self.px(24.0)) * items.len() as f32 + self.px(4.0)).ceil()
    }

    /// The kit's slider over `rect`: the track, then the value text at the right
    /// (`value_width` wide).
    #[allow(clippy::too_many_arguments)]
    pub fn slider(
        &mut self,
        key: Key,
        rect: Rect,
        value: f64,
        (min, max, step): (f64, f64, f64),
        value_text: &str,
        value_width: f32,
        label: &str,
        action: A,
    ) {
        let p = *self.p;
        let area = Rect::new(
            rect.left,
            rect.top,
            rect.width() - value_width - self.px(12.0),
            rect.height(),
        );
        let t = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
        self.kit(&[key], |_| {}, |f, id| f.slider(id, area, t, label, None));
        let knob = (18.0 * self.s).round();
        let track = Rect::new(
            area.left + knob / 2.0,
            rect.top + rect.height() / 2.0 - self.px(2.0),
            area.width() - knob,
            self.px(4.0),
        );
        let style = self.font(Font::Caption);
        self.text_in(
            Rect::new(
                rect.right - value_width,
                rect.top,
                value_width,
                rect.height(),
            ),
            value_text,
            &style,
            p.ink2,
            Align::End,
        );
        self.hit(
            key,
            area,
            Role::Slider,
            &format!("{label}, {value_text}"),
            action,
            Behavior::Slider {
                track,
                min,
                max,
                step,
                value,
            },
        );
        self.semantics(
            key,
            crate::Semantics::Range {
                value,
                min,
                max,
                step,
            },
        );
    }

    /// A select: the gray control fill with the value and a chevron. The app opens its menu.
    pub fn select(&mut self, key: Key, rect: Rect, value: &str, label: &str, action: A) {
        let p = *self.p;
        let r = self.px(8.0).min(rect.height() / 2.0);
        self.fill(rect, r, p.fill);
        self.wash(key, rect, r);
        let style = self.font(Font::Body);
        let chevron = self.px(26.0);
        self.text_in(
            Rect::new(
                rect.left + self.px(12.0),
                rect.top,
                rect.width() - self.px(12.0) - chevron,
                rect.height(),
            ),
            value,
            &style,
            p.ink1,
            Align::Start,
        );
        self.icon(
            Rect::new(
                rect.right - chevron,
                rect.top,
                chevron - self.px(6.0),
                rect.height(),
            ),
            Icon::ChevronDown,
            12.0,
            p.ink3,
        );
        self.hit(
            key,
            rect,
            Role::ComboBox,
            &format!("{label}: {value}"),
            action,
            Behavior::Click,
        );
        self.focus_ring(key, rect, r);
    }

    /// The width a select needs to show `value`.
    pub fn select_width(&mut self, value: &str) -> f32 {
        let style = self.font(Font::Body);
        (self.text.measure(value, &style).0 + self.px(12.0 + 30.0)).ceil()
    }

    /// A text field's frame: the well, a 2 px ring while focused, an optional leading icon.
    /// The field itself is an `Op::TextField` the app appends inside [`field_inner`]; while
    /// it's empty and unfocused, `placeholder` shows.
    #[allow(clippy::too_many_arguments)]
    pub fn field(
        &mut self,
        key: Key,
        rect: Rect,
        glyph: Option<Glyph>,
        placeholder: &str,
        empty: bool,
        focused: bool,
        label: &str,
        action: A,
    ) {
        let p = *self.p;
        let r = self.px(8.0).min(rect.height() / 2.0);
        self.fill(rect, r, p.well);
        if !focused {
            self.wash(key, rect, r);
        }
        if focused {
            self.stroke(
                rect.inflate(self.px(2.0)),
                r + self.px(2.0),
                self.px(2.0),
                p.ink1,
            );
        }
        if let Some(glyph) = glyph {
            self.icon(
                Rect::new(
                    rect.left + self.px(9.0),
                    rect.top,
                    self.px(16.0),
                    rect.height(),
                ),
                glyph,
                14.0,
                p.ink3,
            );
        }
        if empty && !focused {
            let inner = self.field_inner(rect, glyph.is_some());
            let style = self.font(Font::Body);
            self.text_in(inner, placeholder, &style, p.hint(), Align::Start);
        }
        self.hit(key, rect, Role::TextInput, label, action, Behavior::Field);
    }

    /// Where a field's text goes inside its frame.
    pub fn field_inner(&self, rect: Rect, glyph: bool) -> Rect {
        let left = if glyph { 32.0 } else { 12.0 };
        Rect::new(
            rect.left + self.px(left),
            rect.top + (rect.height() - self.px(20.0)) / 2.0,
            rect.width() - self.px(left + 8.0),
            self.px(20.0),
        )
    }

    /// A small badge (20 high) on the gray fill; `solid` for "On desktop". Returns its width.
    pub fn badge(&mut self, x: f32, y: f32, label: &str, glyph: Option<Glyph>, solid: bool) -> f32 {
        let p = *self.p;
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let icon = if glyph.is_some() { self.px(16.0) } else { 0.0 };
        let rect = Rect::new(x, y, tw + icon + self.px(14.0), self.px(20.0));
        let r = self.px(5.0);
        self.fill(rect, r, if solid { p.solid } else { p.fill });
        let ink = if solid { p.on_solid } else { p.ink2 };
        if let Some(g) = glyph {
            self.icon(
                Rect::new(
                    rect.left + self.px(5.0),
                    rect.top,
                    self.px(12.0),
                    rect.height(),
                ),
                g,
                11.0,
                ink,
            );
        }
        self.text_in(
            Rect::new(
                rect.left + self.px(7.0) + icon,
                rect.top,
                tw + self.px(2.0),
                rect.height(),
            ),
            label,
            &style,
            ink,
            Align::Start,
        );
        rect.width()
    }

    /// A badge drawn over a picture: dark translucent, white text. Returns its width.
    pub fn overlay_badge(&mut self, x: f32, y: f32, label: &str, glyph: Option<Glyph>) -> f32 {
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let icon = if glyph.is_some() { self.px(16.0) } else { 0.0 };
        let rect = Rect::new(x, y, tw + icon + self.px(14.0), self.px(20.0));
        let r = self.px(5.0);
        self.fill(rect, r, black(0.62));
        let white = white(1.0);
        if let Some(g) = glyph {
            self.icon(
                Rect::new(
                    rect.left + self.px(5.0),
                    rect.top,
                    self.px(12.0),
                    rect.height(),
                ),
                g,
                11.0,
                white,
            );
        }
        self.text_in(
            Rect::new(
                rect.left + self.px(7.0) + icon,
                rect.top,
                tw + self.px(2.0),
                rect.height(),
            ),
            label,
            &style,
            white,
            Align::Start,
        );
        rect.width()
    }

    /// A keycap (20 high, the well fill). Returns its width.
    pub fn keycap(&mut self, x: f32, y: f32, label: &str) -> f32 {
        let p = *self.p;
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let rect = Rect::new(x, y, (tw + self.px(10.0)).max(self.px(20.0)), self.px(20.0));
        self.fill(rect, self.px(5.0), p.fill);
        self.text_in(rect, label, &style, p.ink2, Align::Center);
        rect.width()
    }

    /// Keycaps for a chord ("Ctrl", "K"); returns the total width.
    pub fn keys(&mut self, x: f32, y: f32, keys: &[&str]) -> f32 {
        let mut at = x;
        for k in keys {
            at += self.keycap(at, y, k) + self.px(4.0);
        }
        at - x - self.px(4.0)
    }

    /// An 8 px status dot: ink-1 (running) or ink-4 (stopped).
    pub fn dot(&mut self, center: Point, on: bool) {
        let d = self.px(8.0);
        let rect = Rect::new(center.x - d / 2.0, center.y - d / 2.0, d, d);
        self.fill(rect, d / 2.0, if on { self.p.ink1 } else { self.p.ink4 });
    }

    /// A tile's frame around `rect` (its picture drawn by the caller): selected is a 2 px
    /// ink-1 ring outside a `gap`-coloured one; hover a 2 px ring of the hover wash. At rest,
    /// nothing: the picture is its own edge. Studio draws selection with [`Self::plate`]
    /// instead; this stays for callers that still want a ring.
    pub fn tile_frame(&mut self, key: Key, rect: Rect, radius: f32, selected: bool, gap: Rgba) {
        let p = *self.p;
        if selected {
            self.stroke(
                rect.inflate(self.px(4.0)),
                radius + self.px(4.0),
                self.px(2.0),
                p.ink1,
            );
            self.stroke(
                rect.inflate(self.px(2.0)),
                radius + self.px(2.0),
                self.px(2.0),
                gap,
            );
        } else if self.hovered(key) {
            self.stroke(
                rect.inflate(self.px(4.0)),
                radius + self.px(4.0),
                self.px(2.0),
                p.selected,
            );
        }
    }

    /// The kit's list row in `rect`: lead, title and subtitle, accessory; the row is one
    /// target. `Accessory::Button` is not supported here (its own hit would be lost).
    #[allow(clippy::too_many_arguments)]
    pub fn row(
        &mut self,
        key: Key,
        rect: Rect,
        lead: Lead,
        title: &str,
        subtitle: Option<&str>,
        accessory: Accessory,
        selected: bool,
        action: A,
    ) {
        debug_assert!(!matches!(accessory, Accessory::Button(..)));
        let switch = match accessory {
            Accessory::Switch(on) => Some(on),
            _ => None,
        };
        self.kit(
            &[key],
            |_| {},
            |f, id| {
                f.row(
                    &Row {
                        id,
                        lead,
                        title,
                        subtitle,
                        accessory,
                        selected,
                        enabled: true,
                    },
                    rect,
                );
            },
        );
        let label = match subtitle {
            Some(s) => format!("{title}: {s}"),
            None => title.to_owned(),
        };
        let (role, checked, behavior) = match switch {
            Some(on) => (Role::Switch, Some(on), Behavior::Toggle),
            None if selected => (Role::RadioButton, Some(true), Behavior::Click),
            None => (Role::Button, None, Behavior::Click),
        };
        self.hit_checked(key, rect, role, &label, checked, action, behavior);
    }

    /// The height of a kit row with or without a subtitle.
    pub fn row_height(&self, subtitle: bool) -> f32 {
        let m = self.kit.metrics;
        (if subtitle { m.row } else { m.row_single } * self.s).round()
    }

    /// A section's heading: the 14/600 heading in ink-1, so it reads first on the page (the
    /// kit's 12/600 secondary header is for glass panels; on an opaque app window it was too
    /// faint to find sections by).
    pub fn section_header(&mut self, rect: Rect, title: &str) {
        let style = self.font(Font::Heading);
        self.text_in(rect, title, &style, self.p.ink1, Align::Start);
    }

    /// The plate behind a selectable picture tile and its label, drawn **before** them: the
    /// selected wash when `selected`, the hover wash when hovered, nothing at rest. Selection
    /// is a fill, never a ring.
    pub fn plate(&mut self, key: Key, rect: Rect, radius: f32, selected: bool) {
        let p = *self.p;
        if selected {
            self.fill(rect, radius, p.selected);
        } else if self.hovered(key) {
            self.fill(rect, radius, p.hover);
        }
    }

    /// A selected card's mark: a 20 px solid circle with a check, top-right inside `card`.
    pub fn check_mark(&mut self, card: Rect) {
        let p = *self.p;
        let r = Rect::new(
            card.right - self.px(32.0),
            card.top + self.px(12.0),
            self.px(20.0),
            self.px(20.0),
        );
        self.fill(r, self.px(10.0), p.solid);
        self.icon(r, Icon::Check, 12.0, p.on_solid);
    }

    /// A card: the panel fill at `radius`, no edge.
    pub fn card(&mut self, rect: Rect, radius: f32) {
        self.fill(rect, radius, self.p.panel);
    }

    // --- Scroll views -------------------------------------------------------------------------

    /// A vertical scroll view: `content` draws from the top `y` it's given and returns where
    /// its content ends. Everything is clipped to `viewport`, hits included. Returns the
    /// content's height (the app clamps `offset` with it). `thumb` is the scrollbar thumb's
    /// target action (it drags with [`Behavior::Thumb`]).
    pub fn scroll(
        &mut self,
        key: Key,
        viewport: Rect,
        offset: f32,
        thumb: A,
        content: impl FnOnce(&mut Self, f32) -> f32,
    ) -> f32 {
        self.list.ops.push(Op::Clip { rect: viewport });
        let clip = match self.clips.last() {
            Some((_, outer)) => intersect(viewport, *outer).unwrap_or(Rect::new(
                viewport.left,
                viewport.top,
                0.0,
                0.0,
            )),
            None => viewport,
        };
        self.clips.push((key, clip));
        let top = viewport.top - offset;
        let bottom = content(self, top);
        self.clips.pop();
        self.list.ops.push(Op::Unclip);
        let height = (bottom - top).max(0.0);
        self.scrolls.push(ScrollArea {
            key,
            viewport,
            content: height,
        });
        if height > viewport.height() + 0.5 {
            let p = *self.p;
            let w = self.px(5.0);
            let track = Rect::new(
                viewport.right - self.px(3.0) - w,
                viewport.top + self.px(4.0),
                w,
                viewport.height() - self.px(8.0),
            );
            let ratio = viewport.height() / height;
            let len = (track.height() * ratio).max(self.px(28.0));
            let max = (height - viewport.height()).max(1.0);
            let y = track.top + (track.height() - len) * (offset / max).clamp(0.0, 1.0);
            let bar = Rect::new(track.left, y, track.width(), len);
            let k = crate::controls::key((key, "thumb"));
            let hot = self.hovered(k) || self.pressed(k);
            // Quiet at rest (it is always there: no fade timer in an app window), clear on hover.
            self.fill(
                bar,
                w / 2.0,
                p.ink1.with_alpha(if hot { 0.45 } else { 0.16 }),
            );
            self.hit(
                k,
                bar.inflate(self.px(3.0)),
                Role::ScrollBar,
                "Scroll",
                thumb,
                Behavior::Thumb {
                    view: key,
                    track,
                    content: height,
                },
            );
        }
        height
    }
}

/// The overlap of two rectangles, if any.
pub fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let r = Rect {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (!r.is_empty()).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Act {
        Go(usize),
        Flip,
    }

    #[test]
    fn hits_index_targets_and_scrolled_out_items_take_no_clicks() {
        let mut text = Text::new().unwrap();
        let (p, t) = (Palette::dark(), Type::new(&text));
        let input = Input::default();
        let mut b = Builder::new(&mut text, &p, &t, 1.0, &input);
        b.switch(key("a"), Point::new(0.0, 0.0), true, "A", Act::Flip);
        let height = b.scroll(
            key("view"),
            Rect::new(0.0, 100.0, 200.0, 100.0),
            0.0,
            Act::Flip,
            |b, y| {
                b.button(
                    key("in"),
                    Rect::new(0.0, y + 10.0, 80.0, 32.0),
                    "In",
                    None,
                    ButtonKind::Ghost,
                    true,
                    Act::Go(1),
                );
                b.button(
                    key("out"),
                    Rect::new(0.0, y + 400.0, 80.0, 32.0),
                    "Out",
                    None,
                    ButtonKind::Ghost,
                    true,
                    Act::Go(2),
                );
                y + 432.0
            },
        );
        assert_eq!(height, 432.0);
        let labels: Vec<_> = b.list.hits.iter().map(|h| h.label.to_string()).collect();
        assert_eq!(labels, ["A", "In", "Out", "Scroll"]);
        assert!(
            b.list.hits[2].rect.is_empty(),
            "the scrolled-out button takes no clicks"
        );
        assert_eq!(b.targets[2].visible, None);
        assert_eq!(
            b.targets[2].scroll,
            Some(key("view")),
            "but the keyboard can reach it"
        );
        assert_eq!(b.targets[1].visible, Some(b.targets[1].rect));
        assert_eq!(b.targets[1].action, Act::Go(1));
        assert!(matches!(b.targets[3].behavior, Behavior::Thumb { .. }));
        assert_eq!(
            b.list
                .ops
                .iter()
                .filter(|o| matches!(o, Op::Clip { .. }))
                .count(),
            1
        );
        assert_eq!(
            b.list
                .ops
                .iter()
                .filter(|o| matches!(o, Op::Unclip))
                .count(),
            1
        );
    }

    #[test]
    fn segments_share_a_group_and_report_their_check_state() {
        let mut text = Text::new().unwrap();
        let (p, t) = (Palette::light(), Type::new(&text));
        let input = Input::default();
        let mut b = Builder::new(&mut text, &p, &t, 1.0, &input);
        b.segmented(
            key("edge"),
            Rect::new(0.0, 0.0, 200.0, 30.0),
            &["Top", "Bottom"],
            Some(1),
            "Edge",
            true,
            Act::Go,
        );
        assert_eq!(b.targets.len(), 2);
        assert!(b.targets.iter().all(|t| t.group == Some(key("edge"))));
        assert_eq!(b.list.hits[1].checked, Some(true));
        assert_eq!(b.targets[0].action, Act::Go(0));
    }

    /// The kit draws Studio's controls: a hovered or keyboard-focused control shows it, and
    /// nothing at rest is outlined.
    #[test]
    fn kit_controls_follow_the_builders_input_and_draw_no_outlines_at_rest() {
        let mut text = Text::new().unwrap();
        let (p, t) = (Palette::dark(), Type::new(&text));
        let draw = |text: &mut Text, input: &Input| {
            let mut b: Builder<'_, Act> = Builder::new(text, &p, &t, 1.0, input);
            b.button(
                key("b"),
                Rect::new(0.0, 0.0, 90.0, 32.0),
                "Go",
                None,
                ButtonKind::Secondary,
                true,
                Act::Flip,
            );
            b.switch(key("s"), Point::new(0.0, 40.0), false, "S", Act::Flip);
            b.segmented(
                key("g"),
                Rect::new(0.0, 70.0, 200.0, 30.0),
                &["A", "B"],
                Some(0),
                "G",
                true,
                Act::Go,
            );
            b.select(
                key("c"),
                Rect::new(0.0, 110.0, 120.0, 30.0),
                "Value",
                "C",
                Act::Flip,
            );
            b.field(
                key("f"),
                Rect::new(0.0, 150.0, 120.0, 32.0),
                None,
                "Type",
                true,
                false,
                "F",
                Act::Flip,
            );
            b.list
        };
        let rest = draw(&mut text, &Input::default());
        assert!(
            !rest
                .ops
                .iter()
                .any(|op| matches!(op, Op::Stroke { color, .. } if color.a > 0.0)),
            "no outlines at rest"
        );
        let focused = draw(
            &mut text,
            &Input {
                focus: Some(key((key("g"), 1usize))),
                focus_visible: true,
                ..Default::default()
            },
        );
        assert!(
            focused
                .ops
                .iter()
                .any(|op| matches!(op, Op::Stroke { color, .. } if color.a > 0.0)),
            "the kit draws the focused segment's ring"
        );
        let hovered = draw(
            &mut text,
            &Input {
                hover: Some(key("s")),
                ..Default::default()
            },
        );
        assert_ne!(rest.ops, hovered.ops, "the kit's switch shows its hover");
    }

    #[test]
    fn text_steps_meet_the_kit_floors_on_every_surface() {
        use crate::theme::{contrast, over};
        for p in [Palette::dark(), Palette::light()] {
            for surface in [p.bg, p.chrome, p.panel, over(p.selected, p.chrome)] {
                let ratio = |c| contrast(c, surface);
                assert!(ratio(p.ink1) >= 7.0, "ink-1 {:.2}", ratio(p.ink1));
                assert!(ratio(p.ink2) >= 4.5, "ink-2 {:.2}", ratio(p.ink2));
                assert!(ratio(p.ink3) >= 4.5, "ink-3 {:.2}", ratio(p.ink3));
                assert!(ratio(p.hint()) >= 3.0, "hint {:.2}", ratio(p.hint()));
            }
        }
    }
}
