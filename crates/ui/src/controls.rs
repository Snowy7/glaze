//! Immediate-mode controls for app windows (Studio): the gray design tokens, the type ramp and
//! the controls built on ops and hits — buttons, switches, segmented controls, sliders,
//! selects, text-field frames, tiles, rows, scroll views, badges and keycaps, plus the panel
//! and scrim helpers palettes, sheets, popovers and toasts are drawn with.
//!
//! A frame is built by a [`Builder`]: every control appends its ops, one hit per target and a
//! [`Target`] saying what it does (`A` is the app's own action type). Input stays the app's:
//! it reads the targets of the frame on screen (`hit id - 1` is the target's index), and tells
//! the next frame what is hovered, pressed and focused through [`Input`]. Keys are stable
//! identities ([`key`]), so focus and hover survive rebuilding the list every frame.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use accesskit::Role;

use crate::geom::{Point, Rect, Rgba};
use crate::scene::{DisplayList, Hit, Op};
use crate::text::{Text, TextStyle};

/// A control's stable identity across frames.
pub type Key = u64;

/// A [`Key`] from anything hashable: a string path, a tuple of parts.
pub fn key(parts: impl Hash) -> Key {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// The gray tokens of the monochrome design (plans/native/studio-redesign.md, 8.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// The main panel (content area).
    pub bg: Rgba,
    /// Sidebar, title area.
    pub chrome: Rgba,
    /// Inspector, cards, inputs, popovers.
    pub panel: Rgba,
    pub hover: Rgba,
    /// Current nav item, selected segment, selected row.
    pub selected: Rgba,
    /// Dividers.
    pub line: Rgba,
    /// Card and panel edges (decorative).
    pub edge: Rgba,
    /// Boundaries that identify a control.
    pub control: Rgba,
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

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x0a0a0a),
            chrome: hex(0x141414),
            panel: hex(0x1c1c1c),
            hover: hex(0x282828),
            selected: hex(0x363636),
            line: hex(0x2b2b2b),
            edge: hex(0x404040),
            control: hex(0x787878),
            ink1: hex(0xf5f5f5),
            ink2: hex(0xc8c8c8),
            ink3: hex(0xa0a0a0),
            ink4: hex(0x666666),
            solid: hex(0xf2f2f2),
            on_solid: hex(0x0a0a0a),
            solid_hover: hex(0xd6d6d6),
            scrim: Rgba::new(0.0, 0.0, 0.0, 0.6),
            shadow: Rgba::new(0.0, 0.0, 0.0, 0.45),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xf5f5f5),
            chrome: hex(0xe9e9e9),
            panel: hex(0xffffff),
            hover: hex(0xececec),
            selected: hex(0xdcdcdc),
            line: hex(0xe2e2e2),
            edge: hex(0xcccccc),
            control: hex(0x7f7f7f),
            ink1: hex(0x0a0a0a),
            ink2: hex(0x333333),
            ink3: hex(0x5c5c5c),
            ink4: hex(0xa3a3a3),
            solid: hex(0x111111),
            on_solid: hex(0xfafafa),
            solid_hover: hex(0x333333),
            scrim: Rgba::new(0.0, 0.0, 0.0, 0.28),
            shadow: Rgba::new(0.0, 0.0, 0.0, 0.12),
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
}

/// The type ramp (8.1), in design pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// 28/36 semibold display: first run, the off state.
    Display,
    /// 20/28 semibold: sheet titles.
    Title1,
    /// 15/20 semibold: page bar and inspector titles.
    Title2,
    /// 13/18 semibold: section titles.
    Heading,
    /// 13/20 regular: labels, descriptions.
    Body,
    /// 13/20 medium: buttons, sidebar items.
    Strong,
    /// 12/16 regular: hints, details, values.
    Caption,
    /// 11/14 medium: badges, keycaps, name tags, counts.
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
        let (family, size, weight) = match font {
            Font::Display => (&self.display, 28.0, 600),
            Font::Title1 => (&self.display, 20.0, 600),
            Font::Title2 => (&self.display, 15.0, 600),
            Font::Heading => (&self.text, 13.0, 600),
            Font::Body => (&self.text, 13.0, 400),
            Font::Strong => (&self.text, 13.0, 500),
            Font::Caption => (&self.text, 12.0, 400),
            Font::Micro => (&self.text, 11.0, 500),
            Font::Mono => (&self.mono, 12.0, 400),
        };
        TextStyle {
            family: family.clone(),
            size: size * scale * self.text_scale,
            weight,
        }
    }

    /// The icon font at `size` design pixels.
    pub fn icon(&self, size: f32, scale: f32) -> TextStyle {
        TextStyle::new(&self.icons, size * scale)
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
    /// `panel` fill with an edge.
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
    pub text: &'a mut Text,
    pub p: &'a Palette,
    pub t: &'a Type,
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
            text,
            p,
            t,
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
            text: shown.into(),
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

    /// An icon glyph centered in `rect`, `size` design pixels.
    pub fn icon(&mut self, rect: Rect, glyph: char, size: f32, color: Rgba) {
        let style = self.t.icon(size, self.s);
        let mut buf = [0u8; 4];
        let glyph = glyph.encode_utf8(&mut buf);
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

    /// A popover, palette, sheet or toast body: the stepped shadow, the panel and its edge.
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
        self.stroke(rect, radius, 1.0, self.p.edge);
    }

    /// The scrim behind a modal, over `rect`. It swallows the pointer (`dismiss` on click).
    pub fn scrim(&mut self, key: Key, rect: Rect, dismiss: A) {
        self.fill(rect, 0.0, self.p.scrim);
        self.hit(key, rect, Role::Pane, "", dismiss, Behavior::Click);
    }

    // --- Controls -----------------------------------------------------------------------------

    /// A button: an optional icon glyph before its label. Returns its width.
    #[allow(clippy::too_many_arguments)]
    pub fn button(
        &mut self,
        key: Key,
        rect: Rect,
        label: &str,
        icon: Option<char>,
        kind: ButtonKind,
        enabled: bool,
        action: A,
    ) {
        let r = self.px(6.0);
        let hot = enabled && self.hovered(key);
        let p = *self.p;
        let (fill, ink, edge) = match kind {
            ButtonKind::Primary => (if hot { p.solid_hover } else { p.solid }, p.on_solid, None),
            ButtonKind::Secondary => (
                if hot { p.hover } else { p.panel },
                p.ink1,
                Some(if hot { p.control } else { p.edge }),
            ),
            ButtonKind::Ghost => (
                if hot { p.hover } else { Rgba::TRANSPARENT },
                if hot { p.ink1 } else { p.ink2 },
                None,
            ),
        };
        let ink = if enabled { ink } else { p.ink4 };
        self.fill(rect, r, fill);
        if let Some(edge) = edge {
            self.stroke(rect, r, 1.0, edge);
        }
        let style = self.font(Font::Strong);
        let (tw, _) = self.text.measure(label, &style);
        let gap = if icon.is_some() && !label.is_empty() {
            self.px(7.0)
        } else {
            0.0
        };
        let iw = if icon.is_some() { self.px(15.0) } else { 0.0 };
        let total = tw + gap + iw;
        let mut x = rect.left + (rect.width() - total) / 2.0;
        if let Some(glyph) = icon {
            self.icon(Rect::new(x, rect.top, iw, rect.height()), glyph, 14.0, ink);
            x += iw + gap;
        }
        self.text_in(
            Rect::new(x, rect.top, rect.right - x, rect.height()),
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
        let pad = if small { 10.0 } else { 12.0 };
        let icon = if icon {
            15.0 + if label.is_empty() { 0.0 } else { 7.0 }
        } else {
            0.0
        };
        tw + self.px(pad * 2.0 + icon)
    }

    /// A square ghost button with a glyph; `on` keeps it lit (a toggled tool).
    #[allow(clippy::too_many_arguments)]
    pub fn icon_button(
        &mut self,
        key: Key,
        rect: Rect,
        glyph: char,
        label: &str,
        on: bool,
        enabled: bool,
        action: A,
    ) {
        let r = self.px(6.0);
        let hot = enabled && self.hovered(key);
        if hot || on {
            self.fill(rect, r, self.p.hover);
        }
        let ink = if !enabled {
            self.p.ink4
        } else if hot || on {
            self.p.ink1
        } else {
            self.p.ink2
        };
        self.icon(rect, glyph, 16.0, ink);
        if enabled {
            self.hit(key, rect, Role::Button, label, action, Behavior::Click);
        }
        self.focus_ring(key, rect, r);
    }

    /// A 36×20 switch, right-aligned in `rect`'s height. `label` names it for assistive tech.
    pub fn switch(&mut self, key: Key, at: Point, on: bool, label: &str, action: A) -> Rect {
        let rect = Rect::new(at.x, at.y, self.px(36.0), self.px(20.0));
        let r = rect.height() / 2.0;
        let p = *self.p;
        let thumb = self.px(14.0);
        if on {
            self.fill(rect, r, p.solid);
            let c = Rect::new(
                rect.right - self.px(3.0) - thumb,
                rect.top + self.px(3.0),
                thumb,
                thumb,
            );
            self.fill(c, thumb / 2.0, p.bg);
        } else {
            self.stroke(
                rect,
                r,
                self.px(1.5),
                if self.hovered(key) { p.ink2 } else { p.control },
            );
            let c = Rect::new(
                rect.left + self.px(3.0),
                rect.top + self.px(3.0),
                thumb,
                thumb,
            );
            self.fill(
                c,
                thumb / 2.0,
                if self.hovered(key) { p.ink2 } else { p.control },
            );
        }
        self.hit_checked(
            key,
            rect.inflate(self.px(4.0)),
            Role::Switch,
            label,
            Some(on),
            action,
            Behavior::Toggle,
        );
        self.focus_ring(key, rect, r);
        rect
    }

    /// A segmented control: equal segments across `rect` when `fill`, else each as wide as its
    /// label. `action(i)` picks segment `i`.
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
        let p = *self.p;
        let pad = self.px(2.0);
        let style = self.font(Font::Strong).scaled(12.5 / 13.0);
        let widths: Vec<f32> = if fill {
            let inner = rect.width() - pad * 2.0 - pad * (items.len() as f32 - 1.0);
            vec![inner / items.len().max(1) as f32; items.len()]
        } else {
            items
                .iter()
                .map(|s| self.text.measure(s, &style).0 + self.px(20.0))
                .collect()
        };
        let total = widths.iter().sum::<f32>() + pad * (items.len() as f32 + 1.0);
        let outer = if fill {
            rect
        } else {
            Rect::new(rect.right - total, rect.top, total, rect.height())
        };
        self.fill(outer, self.px(7.0), p.panel);
        self.stroke(outer, self.px(7.0), 1.0, p.edge);
        let mut x = outer.left + pad;
        self.group(key, |b| {
            for (i, item) in items.iter().enumerate() {
                let seg = Rect::new(x, outer.top + pad, widths[i], outer.height() - pad * 2.0);
                let k = crate::controls::key((key, i));
                let on = selected == Some(i);
                let hot = b.hovered(k);
                if on {
                    b.fill(seg, b.px(5.0), p.selected);
                } else if hot {
                    b.fill(seg, b.px(5.0), p.hover);
                }
                b.text_in(
                    seg.inflate(-b.px(4.0)),
                    item,
                    &style,
                    if on || hot { p.ink1 } else { p.ink3 },
                    Align::Center,
                );
                b.hit_checked(
                    k,
                    seg,
                    Role::RadioButton,
                    &format!("{label}: {item}"),
                    Some(on),
                    action(i),
                    Behavior::Click,
                );
                b.focus_ring(k, seg, b.px(5.0));
                x += widths[i] + pad;
            }
        });
        outer
    }

    /// A slider over `rect` (track plus the value text at the right, `value_width` wide).
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
        let track = Rect::new(
            rect.left + self.px(8.0),
            rect.top + rect.height() / 2.0 - self.px(2.0),
            rect.width() - value_width - self.px(16.0 + 12.0),
            self.px(4.0),
        );
        let t = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
        self.fill(track, self.px(2.0), p.control);
        let filled = Rect::new(track.left, track.top, track.width() * t, track.height());
        self.fill(filled, self.px(2.0), p.ink1);
        let cx = track.left + track.width() * t;
        let cy = track.top + track.height() / 2.0;
        let thumb = self.px(16.0);
        let ring = Rect::new(
            cx - thumb / 2.0 - self.px(2.0),
            cy - thumb / 2.0 - self.px(2.0),
            thumb + self.px(4.0),
            thumb + self.px(4.0),
        );
        self.fill(ring, ring.width() / 2.0, p.chrome);
        let knob = Rect::new(cx - thumb / 2.0, cy - thumb / 2.0, thumb, thumb);
        self.fill(
            knob,
            thumb / 2.0,
            if self.pressed(key) { p.ink2 } else { p.ink1 },
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
        let area = Rect::new(
            rect.left,
            rect.top,
            track.right + self.px(8.0) - rect.left,
            rect.height(),
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
        self.focus_ring(key, knob, thumb / 2.0);
    }

    /// A select: a secondary-button look with the value and a chevron. The app opens its menu.
    pub fn select(&mut self, key: Key, rect: Rect, value: &str, label: &str, action: A) {
        let p = *self.p;
        let r = self.px(6.0);
        let hot = self.hovered(key);
        self.fill(rect, r, if hot { p.hover } else { p.panel });
        self.stroke(rect, r, 1.0, if hot { p.control } else { p.edge });
        let style = self.font(Font::Body).scaled(12.5 / 13.0);
        let chevron = self.px(24.0);
        self.text_in(
            Rect::new(
                rect.left + self.px(10.0),
                rect.top,
                rect.width() - self.px(10.0) - chevron,
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
                chevron - self.px(4.0),
                rect.height(),
            ),
            '\u{E70D}',
            10.0,
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
        let style = self.font(Font::Body).scaled(12.5 / 13.0);
        self.text.measure(value, &style).0 + self.px(10.0 + 28.0)
    }

    /// A text field's frame (32 high, `control` edge, a 2 px ring when focused) with an
    /// optional leading glyph. The field itself is an `Op::TextField` the app appends inside
    /// [`field_inner`]; while it's empty and unfocused, `placeholder` shows.
    #[allow(clippy::too_many_arguments)]
    pub fn field(
        &mut self,
        key: Key,
        rect: Rect,
        glyph: Option<char>,
        placeholder: &str,
        empty: bool,
        focused: bool,
        label: &str,
        action: A,
    ) {
        let p = *self.p;
        let r = self.px(6.0);
        self.fill(rect, r, p.panel);
        if focused {
            self.stroke(
                rect.inflate(self.px(2.0)),
                r + self.px(2.0),
                self.px(2.0),
                p.ink1,
            );
        } else {
            self.stroke(
                rect,
                r,
                1.0,
                if self.hovered(key) { p.ink3 } else { p.control },
            );
        }
        if let Some(glyph) = glyph {
            self.icon(
                Rect::new(
                    rect.left + self.px(8.0),
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
            self.text_in(inner, placeholder, &style, p.ink4, Align::Start);
        }
        self.hit(key, rect, Role::TextInput, label, action, Behavior::Field);
    }

    /// Where a field's text goes inside its frame.
    pub fn field_inner(&self, rect: Rect, glyph: bool) -> Rect {
        let left = if glyph { 32.0 } else { 10.0 };
        Rect::new(
            rect.left + self.px(left),
            rect.top + (rect.height() - self.px(20.0)) / 2.0,
            rect.width() - self.px(left + 8.0),
            self.px(20.0),
        )
    }

    /// A small badge (20 high); `solid` for "On desktop". Returns its width.
    pub fn badge(&mut self, x: f32, y: f32, label: &str, glyph: Option<char>, solid: bool) -> f32 {
        let p = *self.p;
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let icon = if glyph.is_some() { self.px(16.0) } else { 0.0 };
        let rect = Rect::new(x, y, tw + icon + self.px(14.0), self.px(20.0));
        let r = self.px(4.0);
        if solid {
            self.fill(rect, r, p.solid);
        } else {
            self.fill(rect, r, p.panel);
            self.stroke(rect, r, 1.0, p.edge);
        }
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
    pub fn overlay_badge(&mut self, x: f32, y: f32, label: &str, glyph: Option<char>) -> f32 {
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let icon = if glyph.is_some() { self.px(16.0) } else { 0.0 };
        let rect = Rect::new(x, y, tw + icon + self.px(14.0), self.px(20.0));
        let r = self.px(4.0);
        self.fill(rect, r, Rgba::new(0.0, 0.0, 0.0, 0.55));
        self.stroke(rect, r, 1.0, Rgba::new(1.0, 1.0, 1.0, 0.18));
        let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
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

    /// A keycap (20 high, an edge with a heavier bottom). Returns its width.
    pub fn keycap(&mut self, x: f32, y: f32, label: &str) -> f32 {
        let p = *self.p;
        let style = self.font(Font::Micro);
        let (tw, _) = self.text.measure(label, &style);
        let rect = Rect::new(x, y, (tw + self.px(10.0)).max(self.px(20.0)), self.px(20.0));
        let r = self.px(4.0);
        self.fill(rect, r, p.edge);
        self.fill(
            Rect::new(
                rect.left,
                rect.top,
                rect.width(),
                rect.height() - self.px(1.0),
            ),
            r,
            p.panel,
        );
        self.stroke(
            Rect::new(
                rect.left,
                rect.top,
                rect.width(),
                rect.height() - self.px(1.0),
            ),
            r,
            1.0,
            p.edge,
        );
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

    /// An 8 px status dot: filled (running) or a ring (stopped).
    pub fn dot(&mut self, center: Point, on: bool) {
        let d = self.px(8.0);
        let rect = Rect::new(center.x - d / 2.0, center.y - d / 2.0, d, d);
        if on {
            self.fill(rect, d / 2.0, self.p.ink1);
        } else {
            self.stroke(rect, d / 2.0, self.px(1.5), self.p.control);
        }
    }

    /// A tile's frame around `rect` (its picture drawn by the caller): selected is a 2 px ink-1
    /// outline with a gap, hover a 1 px control outline.
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
            self.stroke(rect.inflate(1.0), radius + 1.0, 1.0, p.control);
        } else {
            self.stroke(rect.inflate(1.0), radius + 1.0, 1.0, p.edge);
        }
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
            Some((_, outer)) => intersect(viewport, *outer)
                .unwrap_or(Rect::new(viewport.left, viewport.top, 0.0, 0.0)),
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
            let track = Rect::new(
                viewport.right - self.px(7.0),
                viewport.top + self.px(4.0),
                self.px(4.0),
                viewport.height() - self.px(8.0),
            );
            let ratio = viewport.height() / height;
            let len = (track.height() * ratio).max(self.px(24.0));
            let max = (height - viewport.height()).max(1.0);
            let y = track.top + (track.height() - len) * (offset / max).clamp(0.0, 1.0);
            let bar = Rect::new(track.left, y, track.width(), len);
            let k = crate::controls::key((key, "thumb"));
            let hot = self.hovered(k) || self.pressed(k);
            self.fill(bar, self.px(2.0), if hot { p.control } else { p.edge });
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
        assert!(b.list.hits[2].rect.is_empty(), "the scrolled-out button takes no clicks");
        assert_eq!(b.targets[2].visible, None);
        assert_eq!(b.targets[2].scroll, Some(key("view")), "but the keyboard can reach it");
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
}
