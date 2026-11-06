//! The component kit: every control a bar, flyout, menu, widget or Studio draws, built from
//! one [`Theme`] into one [`DisplayList`] by a [`Frame`].
//!
//! - **Immediate mode, retained output.** A view calls components with the rectangles a
//!   layout ([`crate::layout`]) hands out; each appends paint ops and one hit. The surface
//!   compares the list with the one on screen and redraws only what changed (Rule 2).
//! - **States** come from [`Interaction`] (hover, pressed, keyboard focus). A control's wash
//!   and focus ring are always in the list, transparent at rest, so a hover changes two ops
//!   and never shifts the rest of the list.
//! - **No outlines.** Controls are fills and washes; the only border of a glass surface is the
//!   engine's rim ([`Frame::surface`]); Solid draws one hairline. Focus is a ring, shown only
//!   after keyboard focus moved.
//! - **Pixels.** Lengths in calls are device px (`f.px(design)` converts); text and icons land
//!   on whole pixels.

use std::sync::Arc;
use std::time::Instant;

use accesskit::Role;

use crate::geom::{Point, Rect, Rgba};
use crate::glass::Glass;
use crate::icons::{Icon, IconSet};
use crate::interact::{Interaction, State};
use crate::layout::Align;
use crate::scene::{DisplayList, Hit, HitId, Op};
use crate::scroll::Scroll;
use crate::text::{Text, TextStyle};
use crate::theme::{TextRole, Theme};

/// One display list being built.
pub struct Frame<'a> {
    pub text: &'a mut Text,
    pub theme: &'a Theme,
    /// Device px per design px.
    pub s: f32,
    pub list: DisplayList,
    pub input: &'a Interaction,
    motion: Option<&'a crate::interact::motion::InteractionMotion>,
}

/// A button's emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Accent fill: the one primary action.
    Filled,
    /// Accent wash with accent text.
    Tinted,
    /// The neutral control fill.
    Gray,
    /// No fill until hovered.
    Plain,
    /// Destructive text on the neutral fill.
    Destructive,
}

/// What leads a row.
#[derive(Debug, Clone)]
pub enum Lead {
    None,
    Icon(Icon),
    /// An icon in a filled circle (Control Center, settings lists); accent when `on`.
    Badge(Icon, bool),
    Image(Arc<crate::Image>),
    /// A letter in a circle, for an app with no icon.
    Monogram(char),
}

/// What ends a row.
#[derive(Debug, Clone)]
pub enum Accessory<'a> {
    None,
    Chevron,
    Check,
    Text(&'a str),
    Switch(bool),
    /// A small icon button with its own hit (a dismiss ×).
    Button(HitId, Icon, &'a str),
}

/// A list row: lead, title and subtitle, accessory. The row is one button.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub id: HitId,
    pub lead: Lead,
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub accessory: Accessory<'a>,
    pub selected: bool,
    pub enabled: bool,
}

/// A Control Center tile: an icon in a circle, a title and a status; a toggle, with an
/// optional "more" chevron of its own.
#[derive(Debug, Clone)]
pub struct Tile<'a> {
    pub id: HitId,
    pub icon: Icon,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub on: bool,
    pub enabled: bool,
    pub more: Option<HitId>,
}

/// The id of something drawn that is not a control.
pub const NONE: HitId = 0;

impl<'a> Frame<'a> {
    pub fn new(text: &'a mut Text, theme: &'a Theme, s: f32, input: &'a Interaction) -> Self {
        Self {
            text,
            theme,
            s,
            list: DisplayList::default(),
            input,
            motion: None,
        }
    }

    /// Opt into sampled hover/press washes. Existing kit callers keep immediate states.
    pub fn with_motion(mut self, motion: &'a crate::interact::motion::InteractionMotion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// Design px to device px, on whole pixels.
    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    pub fn font(&self, role: TextRole) -> TextStyle {
        self.theme.font.style(role, self.s)
    }

    /// The line height of `role`, device px.
    pub fn line(&self, role: TextRole) -> f32 {
        self.theme.font.line(role, self.s)
    }

    pub fn push(&mut self, op: Op) {
        self.list.ops.push(op);
    }

    pub fn fill(&mut self, rect: Rect, radius: f32, color: Rgba) {
        self.push(Op::Fill {
            rect,
            radius,
            color,
        });
    }

    pub fn hit(
        &mut self,
        id: HitId,
        rect: Rect,
        role: Role,
        label: &str,
        enabled: bool,
        checked: Option<bool>,
    ) {
        if id == NONE {
            return;
        }
        self.list.hits.push(Hit {
            id,
            rect,
            role,
            label: label.into(),
            enabled,
            checked,
        });
    }

    pub fn state(&self, id: HitId, enabled: bool) -> State {
        if id == NONE {
            return State::Rest;
        }
        self.input.state(id, enabled)
    }

    /// A control's radius at `height`: the theme's, or a capsule.
    pub fn control_radius(&self, height: f32) -> f32 {
        self.theme
            .radius
            .control
            .map_or(height / 2.0, |r| self.px(r).min(height / 2.0))
    }

    /// The hover/pressed wash of `id` over `rect` (always pushed, transparent at rest).
    pub fn wash(&mut self, id: HitId, rect: Rect, radius: f32, enabled: bool) {
        let color = match self.state(id, enabled) {
            State::Hover => self.theme.color.hover,
            State::Pressed => self.theme.color.pressed,
            State::Rest | State::Disabled => Rgba::TRANSPARENT,
        };
        let color = self
            .motion
            .filter(|_| enabled && id != NONE)
            .map_or(color, |motion| {
                motion.wash(id, self.theme.color.hover, self.theme.color.pressed, color)
            });
        self.fill(rect, radius, color);
    }

    /// The keyboard focus ring of `id` just outside `rect` (always pushed, transparent unless
    /// the keyboard moved focus there).
    pub fn focus_ring(&mut self, id: HitId, rect: Rect, radius: f32) {
        let width = self.px(2.0).max(2.0);
        let gap = self.px(2.0);
        self.push(Op::Stroke {
            rect: rect.inflate(gap + width / 2.0),
            radius: radius + gap + width / 2.0,
            width,
            color: if id != NONE && self.input.ring(id) {
                self.theme.color.accent.with_alpha(0.9)
            } else {
                Rgba::TRANSPARENT
            },
        });
    }

    // ---- Surface -----------------------------------------------------------------------

    /// A surface's body over `rect` (the window's shape), drawn first. With glass (the
    /// theme's material and `glass_available`), only an invisible hit fill: the engine draws
    /// the glass and its one rim, and the returned [`Glass`] tells it the exact shape. Without,
    /// the theme's surface fill and, for Solid, its one hairline.
    pub fn surface(&mut self, rect: Rect, glass_available: bool) -> Option<Glass> {
        let radius = self.px(self.theme.radius.surface);
        self.surface_shaped(rect, radius, glass_available)
    }

    /// [`Frame::surface`] at `radius` device px instead of the theme's: a bar strip (0), an
    /// island or a dock plate whose corners come from its own config.
    pub fn surface_shaped(&mut self, rect: Rect, radius: f32, glass_available: bool) -> Option<Glass> {
        match self.theme.glass.filter(|_| glass_available) {
            Some(material) => {
                self.fill(rect, radius, Rgba::HIT);
                Some(Glass {
                    body: rect,
                    radius,
                    material,
                })
            }
            None => {
                self.fill(rect, radius, self.theme.color.surface);
                if let Some(border) = self.theme.border {
                    self.push(Op::Stroke {
                        rect: rect.inflate(-0.5),
                        radius: radius - 0.5,
                        width: 1.0,
                        color: border,
                    });
                }
                None
            }
        }
    }

    /// A surface fill with the same continuous corners as the engine glass. Zero smoothing
    /// retains the existing circular fill and its fast software path.
    pub fn fill_continuous(&mut self, rect: Rect, radius: f32, smoothing: f32, color: Rgba) {
        if smoothing > 0.0 {
            self.push(Op::Continuous { rect, radius, smoothing, width: 0.0, color });
        } else {
            self.fill(rect, radius, color);
        }
    }

    /// A card inside a surface: the card fill at the concentric radius. Returns its content
    /// rect (inset by the panel padding's half, rounded).
    pub fn card(&mut self, rect: Rect) -> Rect {
        let radius = self.px(self.theme.radius.card);
        self.fill(rect, radius, self.theme.color.card);
        rect.inflate(-self.px(self.theme.metrics.pad))
    }

    pub fn separator(&mut self, left: f32, right: f32, y: f32) {
        let h = 1.0_f32.max(self.s.floor());
        self.fill(
            Rect::new(left, y.round(), right - left, h),
            0.0,
            self.theme.color.separator,
        );
    }

    /// A vertical hairline from `top` to `bottom` at `x` (a dock's divider before its trash).
    pub fn separator_v(&mut self, top: f32, bottom: f32, x: f32) {
        let w = 1.0_f32.max(self.s.floor());
        self.fill(
            Rect::new(x.round(), top, w, bottom - top),
            0.0,
            self.theme.color.separator,
        );
    }

    // ---- Text and icons ----------------------------------------------------------------

    /// `text` with its top-left at `(x, y)`, cut with an ellipsis past `max` px. Its size.
    pub fn label(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        role: TextRole,
        color: Rgba,
        max: f32,
    ) -> (f32, f32) {
        let style = self.font(role);
        self.label_styled(x, y, text, &style, color, max)
    }

    pub fn label_styled(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        style: &TextStyle,
        color: Rgba,
        max: f32,
    ) -> (f32, f32) {
        let fitted = self.text.fit(text, style, max.max(0.0));
        let (w, h) = self.text.measure(&fitted, style);
        self.push(Op::Text {
            rect: Rect::new(x.round(), y.round(), w, h),
            text: fitted,
            style: style.clone(),
            color,
        });
        (w, h)
    }

    /// `text` placed in `rect` by `x` (vertically centred), cut to fit. Its size.
    pub fn text_in(
        &mut self,
        rect: Rect,
        text: &str,
        role: TextRole,
        color: Rgba,
        x: Align,
    ) -> (f32, f32) {
        let style = self.font(role);
        let fitted = self.text.fit(text, &style, rect.width().max(0.0));
        let (w, h) = self.text.measure(&fitted, &style);
        let at = rect.place(w, h, x, Align::Center);
        self.push(Op::Text {
            rect: at,
            text: fitted,
            style,
            color,
        });
        (w, h)
    }

    /// Wrapped text from `(x, y)`, `width` wide, at most `lines` lines on the role's line
    /// height. The height used.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: &str,
        role: TextRole,
        color: Rgba,
        lines: usize,
    ) -> f32 {
        let style = self.font(role);
        let line = self.line(role);
        let mut top = y.round();
        let (_, h) = self.text.measure("Ag", &style);
        // Centre each line's box in its line height.
        let lead = ((line - h) / 2.0).round();
        let wrapped = self.text.lines(text, &style, width, lines);
        let count = wrapped.len();
        for text in wrapped {
            let (w, h) = self.text.measure(&text, &style);
            self.push(Op::Text {
                rect: Rect::new(x.round(), top + lead, w, h),
                text,
                style: style.clone(),
                color,
            });
            top += line;
        }
        line * count as f32
    }

    /// How many lines `text` takes at `width` (at most `lines`).
    pub fn line_count(&mut self, text: &str, role: TextRole, width: f32, lines: usize) -> usize {
        let style = self.font(role);
        self.text.lines(text, &style, width, lines).len()
    }

    /// `icon` at `size` design px, centred in `rect`: the theme's set (vectors, or the icon
    /// font for Fluent; icons the font lacks are vectors there too).
    pub fn icon(&mut self, rect: Rect, icon: Icon, size: f32, color: Rgba) {
        let px = self.px(size);
        if let IconSet::Font(family) = &self.theme.icons
            && icon.font_glyph().is_some()
        {
            let family = family.clone();
            let op = crate::icons::centered(self.text, &family, icon, px, rect, color);
            self.push(op);
            return;
        }
        let at = rect.place(px, px, Align::Center, Align::Center);
        // Thin strokes go soft at small sizes: never under 1.35 px (at 1x).
        let weight = (self.theme.icon_weight * px / 24.0).max(1.35 * self.s.min(1.5));
        self.push(Op::Icon {
            rect: at,
            icon,
            weight,
            color,
        });
    }

    // ---- Controls ----------------------------------------------------------------------

    /// A button's intrinsic size for `label` (with a leading icon).
    pub fn button_size(&mut self, label: &str, icon: bool) -> (f32, f32) {
        let style = self.font(TextRole::Headline);
        let (w, _) = self.text.measure(label, &style);
        let icon = if icon {
            self.px(self.theme.metrics.icon) + self.px(6.0)
        } else {
            0.0
        };
        (
            (w + icon + self.px(28.0)).ceil(),
            self.px(self.theme.metrics.control),
        )
    }

    pub fn button(
        &mut self,
        id: HitId,
        rect: Rect,
        label: &str,
        icon: Option<Icon>,
        kind: ButtonKind,
        enabled: bool,
    ) {
        let c = self.theme.color;
        let radius = self.control_radius(rect.height());
        let (fill, ink) = match kind {
            ButtonKind::Filled => (c.accent, c.on_accent),
            ButtonKind::Tinted => (c.accent_wash, c.accent),
            ButtonKind::Gray => (c.fill, c.label),
            ButtonKind::Plain => (Rgba::TRANSPARENT, c.accent),
            ButtonKind::Destructive => (c.fill, c.destructive),
        };
        let ink = if enabled { ink } else { c.quaternary };
        self.fill(rect, radius, if enabled { fill } else { c.well });
        self.wash(id, rect, radius, enabled);
        let style = self.font(TextRole::Headline);
        let (w, h) = self.text.measure(label, &style);
        let icon_w = icon.map_or(0.0, |_| self.px(self.theme.metrics.icon) + self.px(6.0));
        let room = rect.width() - self.px(16.0) - icon_w;
        let width = (w.min(room) + icon_w).max(0.0);
        let left = rect.left + (rect.width() - width) / 2.0;
        if let Some(icon) = icon {
            let size = self.px(self.theme.metrics.icon);
            let r = Rect::new(left, rect.top, size, rect.height());
            self.icon(r, icon, self.theme.metrics.icon, ink);
        }
        let y = rect.top + (rect.height() - h) / 2.0;
        self.label_styled(left + icon_w, y, label, &style, ink, room);
        self.focus_ring(id, rect, radius);
        self.hit(id, rect, Role::Button, label, enabled, None);
    }

    /// A round icon button `size` design px across, centred in `rect`. `filled` gives it the
    /// control fill at rest (media controls), else it shows only when hovered.
    pub fn icon_button(
        &mut self,
        id: HitId,
        rect: Rect,
        icon: Icon,
        size: f32,
        label: &str,
        filled: bool,
    ) {
        let d = self.px(size);
        let r = rect.place(d, d, Align::Center, Align::Center);
        let c = self.theme.color;
        self.fill(r, d / 2.0, if filled { c.fill } else { Rgba::TRANSPARENT });
        self.wash(id, r, d / 2.0, true);
        let glyph = (size * 0.5).round().max(12.0);
        self.icon(r, icon, glyph, c.label);
        self.focus_ring(id, r, d / 2.0);
        self.hit(id, r, Role::Button, label, true, None);
    }

    /// A quiet toolbar icon: no resting outline or fill, a rounded wash on hover.
    pub fn quiet_icon_button(&mut self, id: HitId, rect: Rect, icon: Icon, label: &str, enabled: bool, selected: bool) {
        let radius = self.px(6.0);
        let c = self.theme.color;
        if selected { self.fill(rect, radius, c.accent_wash); }
        self.wash(id, rect, radius, enabled);
        self.icon(rect, icon, 17.0, if !enabled { c.quaternary } else if selected { c.accent } else { c.secondary });
        self.focus_ring(id, rect, radius);
        self.hit(id, rect, Role::Button, label, enabled, selected.then_some(true));
    }

    /// An on/off switch with its right edge at `right`, centred on `center_y`. Its rect.
    pub fn switch(
        &mut self,
        id: HitId,
        right: f32,
        center_y: f32,
        on: bool,
        label: &str,
        enabled: bool,
    ) -> Rect {
        let (w, h) = (self.px(40.0), self.px(24.0));
        let rect = Rect::new(right - w, (center_y - h / 2.0).round(), w, h);
        let c = self.theme.color;
        let track = match (on, enabled) {
            (true, true) => c.accent,
            (true, false) => c.accent.with_alpha(0.4),
            (false, _) => c.fill,
        };
        self.fill(rect, h / 2.0, track);
        self.wash(id, rect, h / 2.0, enabled);
        let knob = h - self.px(4.0);
        let x = if on {
            rect.right - self.px(2.0) - knob
        } else {
            rect.left + self.px(2.0)
        };
        let k = Rect::new(x, rect.top + self.px(2.0), knob, knob);
        self.push(Op::Shadow {
            rect: k,
            radius: knob / 2.0,
            spread: self.px(2.0),
            drop: self.px(1.0),
            color: Rgba::new(0.0, 0.0, 0.0, 0.10),
        });
        let knob_color = if enabled {
            c.knob
        } else {
            c.knob.with_alpha(0.6)
        };
        self.fill(k, knob / 2.0, knob_color);
        self.focus_ring(id, rect, h / 2.0);
        self.hit(
            id,
            rect.inflate(self.px(4.0)),
            Role::Switch,
            label,
            enabled,
            Some(on),
        );
        rect
    }

    /// A slider across `rect` (`value` 0..=1). With `icon`, the Control Center bar: a thick
    /// capsule filled to the value with the glyph inside; without, a thin track and a knob.
    /// The hit spans `rect`; its label ends in the percentage (the flyouts read it back).
    pub fn slider(&mut self, id: HitId, rect: Rect, value: f32, label: &str, icon: Option<Icon>) {
        let value = value.clamp(0.0, 1.0);
        let c = self.theme.color;
        match icon {
            Some(icon) => {
                let h = self.px(28.0).min(rect.height());
                let bar = rect.place(rect.width(), h, Align::Start, Align::Center);
                let r = h / 2.0;
                self.fill(bar, r, c.fill);
                // The fill keeps the capsule's round start even near zero.
                let at = bar.left + h + (bar.width() - h) * value;
                // Control Center's white fill on dark glass; on light surfaces white would
                // vanish into the card, so the accent fills there. The glyph always sits on
                // the fill (it starts a full capsule wide).
                let (fill, glyph_ink) = match (self.theme.style, self.theme.dark()) {
                    (crate::theme::Style::Mono, _) => (c.label, c.surface.with_alpha(1.0)),
                    (_, true) => (c.knob, Rgba::new(0.0, 0.0, 0.0, 0.62)),
                    (_, false) => (c.accent, c.on_accent),
                };
                self.fill(Rect { right: at, ..bar }, r, fill);
                self.wash(id, bar, r, true);
                let glyph = Rect::new(bar.left, bar.top, h, h);
                self.icon(glyph, icon, 14.0, glyph_ink);
                self.focus_ring(id, bar, r);
            }
            None => {
                let knob = self.px(18.0);
                let track_h = self.px(4.0);
                let track = Rect::new(
                    rect.left + knob / 2.0,
                    (rect.top + (rect.height() - track_h) / 2.0).round(),
                    rect.width() - knob,
                    track_h,
                );
                self.fill(track, track_h / 2.0, c.fill);
                let at = track.left + track.width() * value;
                self.fill(Rect { right: at, ..track }, track_h / 2.0, c.accent);
                let k = Rect::new(
                    (at - knob / 2.0).round(),
                    (track.top + track_h / 2.0 - knob / 2.0).round(),
                    knob,
                    knob,
                );
                self.wash(id, k.inflate(self.px(4.0)), knob / 2.0 + self.px(4.0), true);
                self.push(Op::Shadow {
                    rect: k,
                    radius: knob / 2.0,
                    spread: self.px(3.0),
                    drop: self.px(1.0),
                    color: Rgba::new(0.0, 0.0, 0.0, 0.12),
                });
                self.fill(k, knob / 2.0, c.knob);
                self.focus_ring(id, k, knob / 2.0);
            }
        }
        let percent = (value * 100.0).round();
        self.hit(id, rect, Role::Slider, &format!("{label} {percent}%"), true, None);
    }

    /// A segmented control: `labels` as segments, `selected` raised. Segment `i` is hit
    /// `base + i`.
    pub fn segmented(&mut self, base: HitId, rect: Rect, labels: &[&str], selected: usize) {
        let c = self.theme.color;
        let radius = self.control_radius(rect.height());
        self.fill(rect, radius, c.well);
        let n = labels.len().max(1) as f32;
        let inner = rect.inflate(-self.px(2.0));
        let w = inner.width() / n;
        let inner_radius = (radius - self.px(2.0)).max(0.0);
        for (i, label) in labels.iter().enumerate() {
            let seg = Rect::new(
                (inner.left + w * i as f32).round(),
                inner.top,
                w.round(),
                inner.height(),
            );
            let id = base + i as HitId;
            let on = i == selected;
            self.fill(
                seg,
                inner_radius,
                if on {
                    if self.theme.dark() {
                        c.fill
                    } else {
                        c.knob
                    }
                } else {
                    Rgba::TRANSPARENT
                },
            );
            self.wash(id, seg, inner_radius, true);
            let ink = if on { c.label } else { c.secondary };
            self.text_in(
                seg.inflate(-self.px(6.0)),
                label,
                TextRole::Callout,
                ink,
                Align::Center,
            );
            self.focus_ring(id, seg, inner_radius);
            self.hit(id, seg, Role::RadioButton, label, true, Some(on));
        }
    }

    /// A row's height for its content.
    pub fn row_height(&self, row: &Row) -> f32 {
        let m = self.theme.metrics;
        self.px(if row.subtitle.is_some() {
            m.row
        } else {
            m.row_single
        })
    }

    /// A list row in `rect`. The row's highlight is concentric with a card around it.
    pub fn row(&mut self, row: &Row, rect: Rect) {
        let c = self.theme.color;
        let radius = self.px(Radii::concentric_row(self.theme));
        if row.selected {
            self.fill(rect, radius, c.accent_wash);
        }
        self.wash(row.id, rect, radius, row.enabled);
        let inset = self.px(10.0);
        let mut left = rect.left + inset;
        let mid = rect.top + rect.height() / 2.0;
        let lead = self.px(28.0);
        let lead_rect = Rect::new(left, (mid - lead / 2.0).round(), lead, lead);
        let ink = if row.enabled { c.label } else { c.quaternary };
        match &row.lead {
            Lead::None => {}
            Lead::Icon(icon) => {
                self.icon(lead_rect, *icon, self.theme.metrics.icon + 2.0, ink);
                left += lead + self.px(10.0);
            }
            Lead::Badge(icon, on) => {
                let (fill, glyph) = if *on {
                    (c.accent, c.on_accent)
                } else {
                    (c.fill, ink)
                };
                self.fill(lead_rect, lead / 2.0, fill);
                self.icon(lead_rect, *icon, 15.0, glyph);
                left += lead + self.px(10.0);
            }
            Lead::Image(image) => {
                let size = self.px(24.0);
                let r = lead_rect.place(size, size, Align::Center, Align::Center);
                self.push(Op::Image {
                    rect: r,
                    image: image.clone(),
                    opacity: if row.enabled { 1.0 } else { 0.5 },
                    source: None,
                });
                left += lead + self.px(10.0);
            }
            Lead::Monogram(letter) => {
                self.fill(lead_rect, lead / 2.0, c.fill);
                let mut buf = [0u8; 4];
                let text: &str = letter.encode_utf8(&mut buf);
                self.text_in(lead_rect, text, TextRole::Subheadline, ink, Align::Center);
                left += lead + self.px(10.0);
            }
        }
        // The accessory, right to left.
        let mut right = rect.right - inset;
        match &row.accessory {
            Accessory::None => {}
            Accessory::Chevron | Accessory::Check => {
                let size = self.px(16.0);
                let r = Rect::new(right - size, rect.top, size, rect.height());
                let (icon, color) = if matches!(row.accessory, Accessory::Check) {
                    (Icon::Check, c.accent)
                } else {
                    (Icon::ChevronRight, c.tertiary)
                };
                self.icon(r, icon, 14.0, color);
                right -= size + self.px(8.0);
            }
            Accessory::Text(text) => {
                let style = self.font(TextRole::Callout);
                let (w, h) = self.text.measure(text, &style);
                let w = w.min(rect.width() / 3.0);
                self.label_styled(right - w, mid - h / 2.0, text, &style, c.secondary, w + 1.0);
                right -= w + self.px(8.0);
            }
            Accessory::Switch(on) => {
                let r = self.switch(NONE, right, mid, *on, row.title, row.enabled);
                right = r.left - self.px(8.0);
            }
            Accessory::Button(id, icon, label) => {
                let size = self.px(24.0);
                let r = Rect::new(right - size, (mid - size / 2.0).round(), size, size);
                self.icon_button(*id, r, *icon, 24.0, label, false);
                right -= size + self.px(6.0);
            }
        }
        let room = (right - left).max(0.0);
        let title = self.font(TextRole::Headline);
        let (_, th) = self.text.measure("Ag", &title);
        match row.subtitle {
            Some(subtitle) => {
                let sub = self.font(TextRole::Callout);
                let (_, sh) = self.text.measure("Ag", &sub);
                let top = mid - (th + sh) / 2.0;
                self.label_styled(left, top, row.title, &title, ink, room);
                let sub_ink = if row.enabled { c.secondary } else { c.quaternary };
                self.label_styled(left, top + th, subtitle, &sub, sub_ink, room);
            }
            None => {
                self.label_styled(left, mid - th / 2.0, row.title, &title, ink, room);
            }
        }
        self.focus_ring(row.id, rect, radius);
        let label = match row.subtitle {
            Some(s) => format!("{}, {s}", row.title),
            None => row.title.to_owned(),
        };
        let checked = match row.accessory {
            Accessory::Switch(on) => Some(on),
            Accessory::Check => Some(true),
            _ => row.selected.then_some(true),
        };
        let role = match row.accessory {
            Accessory::Switch(_) => Role::Switch,
            _ if row.selected => Role::RadioButton,
            _ => Role::Button,
        };
        // The row's hit goes first, so an accessory button inside it wins.
        let hit = Hit {
            id: row.id,
            rect,
            role,
            label: label.into(),
            enabled: row.enabled,
            checked,
        };
        if row.id != NONE {
            let at = self
                .list
                .hits
                .iter()
                .rposition(|h| !rect.contains(Point::new(h.rect.left + 1.0, h.rect.top + 1.0)))
                .map_or(0, |i| i + 1);
            self.list.hits.insert(at, hit);
        }
    }

    /// A section header: a small title, and an optional plain action at its right. Its
    /// height is `metrics.row_single` less a third.
    pub fn section_header(&mut self, rect: Rect, title: &str, action: Option<(HitId, &str)>) {
        let c = self.theme.color;
        let mut room = rect.width();
        if let Some((id, label)) = action {
            let style = self.font(TextRole::Subheadline);
            let (w, _) = self.text.measure(label, &style);
            let r = Rect::new(
                rect.right - w - self.px(12.0),
                rect.top,
                w + self.px(12.0),
                rect.height(),
            );
            let radius = self.control_radius(r.height());
            self.wash(id, r, radius, true);
            self.text_in(r, label, TextRole::Subheadline, c.accent, Align::Center);
            self.focus_ring(id, r, radius);
            self.hit(id, r, Role::Button, label, true, None);
            room -= r.width() + self.px(8.0);
        }
        let r = Rect { right: rect.left + room, ..rect };
        self.text_in(r, title, TextRole::Subheadline, c.secondary, Align::Start);
    }

    /// A Control Center tile in `rect`.
    pub fn tile(&mut self, tile: &Tile, rect: Rect) {
        let c = self.theme.color;
        let radius = self.px(self.theme.radius.card);
        self.fill(rect, radius, c.card);
        let more = tile.more.map_or(0.0, |_| self.px(30.0));
        let main = Rect {
            right: rect.right - more,
            ..rect
        };
        self.wash(tile.id, main, radius, tile.enabled);
        let d = self.px(32.0);
        let mid = rect.top + rect.height() / 2.0;
        let circle = Rect::new(rect.left + self.px(10.0), (mid - d / 2.0).round(), d, d);
        let (fill, glyph) = if tile.on && tile.enabled {
            (c.accent, c.on_accent)
        } else {
            (c.fill, if tile.enabled { c.label } else { c.quaternary })
        };
        self.fill(circle, d / 2.0, fill);
        self.icon(circle, tile.icon, 16.0, glyph);
        let left = circle.right + self.px(10.0);
        let room = (main.right - left - self.px(8.0)).max(0.0);
        let title = self.font(TextRole::Headline);
        let sub = self.font(TextRole::Footnote);
        let (_, th) = self.text.measure("Ag", &title);
        let (_, sh) = self.text.measure("Ag", &sub);
        let top = mid - (th + sh) / 2.0;
        let ink = if tile.enabled { c.label } else { c.quaternary };
        self.label_styled(left, top, tile.title, &title, ink, room);
        self.label_styled(left, top + th, tile.subtitle, &sub, c.secondary, room);
        self.focus_ring(tile.id, main, radius);
        self.hit(tile.id, main, Role::Switch, tile.title, tile.enabled, Some(tile.on));
        if let Some(id) = tile.more {
            let r = Rect {
                left: rect.right - more,
                ..rect
            };
            self.wash(id, r, radius, true);
            self.icon(r, Icon::ChevronRight, 13.0, c.tertiary);
            self.focus_ring(id, r, radius);
            self.hit(id, r, Role::Button, &format!("{} settings", tile.title), true, None);
        }
    }

    /// A progress bar across `rect` (`value` 0..=1), capsule ends.
    pub fn progress(&mut self, rect: Rect, value: f32, color: Option<Rgba>) {
        let c = self.theme.color;
        let r = rect.height() / 2.0;
        self.fill(rect, r, c.fill);
        let value = value.clamp(0.0, 1.0);
        if value > 0.0 {
            let right = rect.left + (rect.width() * value).max(rect.height());
            self.fill(
                Rect { right, ..rect },
                r,
                color.unwrap_or(c.accent),
            );
        }
    }

    /// A ring gauge around `center` (`value` 0..=1), `width` px thick.
    pub fn ring(&mut self, center: Point, radius: f32, width: f32, value: f32, color: Option<Rgba>) {
        let c = self.theme.color;
        self.push(Op::Arc {
            center,
            radius,
            sweep: std::f32::consts::TAU,
            width,
            color: c.fill,
        });
        self.push(Op::Arc {
            center,
            radius,
            sweep: value.clamp(0.0, 1.0) * std::f32::consts::TAU,
            width,
            color: color.unwrap_or(c.accent),
        });
    }

    /// A count or status badge with its left at `x`, centred on `center_y`. Its width.
    pub fn badge(&mut self, x: f32, center_y: f32, text: &str, color: Option<Rgba>) -> f32 {
        let c = self.theme.color;
        let style = self.font(TextRole::Caption);
        let (w, h) = self.text.measure(text, &style);
        let height = self.px(18.0);
        let width = (w + self.px(12.0)).max(height);
        let rect = Rect::new(x, (center_y - height / 2.0).round(), width.round(), height);
        let (fill, ink) = match color {
            Some(color) => (color, c.on_accent),
            None => (c.fill, c.label),
        };
        self.fill(rect, height / 2.0, fill);
        self.push(Op::Text {
            rect: rect.place(w, h, Align::Center, Align::Center),
            text: text.into(),
            style,
            color: ink,
        });
        rect.width()
    }

    /// A tooltip for `anchor`, below it (above when it would leave `bounds`), drawn on the
    /// elevated fill with a soft shadow. Its rect.
    pub fn tooltip(&mut self, anchor: Rect, text: &str, bounds: Rect) -> Rect {
        let c = self.theme.color;
        let style = self.font(TextRole::Footnote);
        let (w, h) = self.text.measure(text, &style);
        let (pw, ph) = (self.px(10.0), self.px(6.0));
        let (tw, th) = (w + 2.0 * pw, h + 2.0 * ph);
        let gap = self.px(6.0);
        let left = (anchor.center().x - tw / 2.0)
            .clamp(bounds.left, (bounds.right - tw).max(bounds.left))
            .round();
        let below = anchor.bottom + gap;
        let top = if below + th <= bounds.bottom {
            below
        } else {
            anchor.top - gap - th
        };
        let rect = Rect::new(left, top.round(), tw, th);
        let radius = self.px(self.theme.radius.small + 2.0);
        self.push(Op::Shadow {
            rect,
            radius,
            spread: self.px(8.0),
            drop: self.px(2.0),
            color: c.shadow.with_alpha(c.shadow.a * 0.25),
        });
        self.fill(rect, radius, c.elevated);
        self.push(Op::Text {
            rect: Rect::new(rect.left + pw, rect.top + ph, w, h),
            text: text.into(),
            style,
            color: c.label,
        });
        rect
    }

    /// A text field's frame in `rect`: the well, the placeholder while `empty`, and the
    /// accent ring while `focused`. The field itself (`Op::TextField`) goes inside the
    /// returned rect.
    pub fn field_frame(
        &mut self,
        rect: Rect,
        placeholder: &str,
        empty: bool,
        focused: bool,
        icon: Option<Icon>,
    ) -> Rect {
        let c = self.theme.color;
        let radius = self.control_radius(rect.height());
        self.fill(rect, radius, c.well);
        let mut inner = rect.inflate(-self.px(10.0));
        inner.top = rect.top;
        inner.bottom = rect.bottom;
        if let Some(icon) = icon {
            let size = self.px(18.0);
            let r = Rect { right: inner.left + size, ..inner };
            self.icon(r, icon, 15.0, c.tertiary);
            inner.left += size + self.px(6.0);
        }
        let style = self.font(TextRole::Body);
        let (_, h) = self.text.measure("Ag", &style);
        if empty {
            self.label_styled(
                inner.left,
                inner.top + (inner.height() - h) / 2.0,
                placeholder,
                &style,
                c.tertiary,
                inner.width(),
            );
        }
        let width = self.px(2.0);
        self.push(Op::Stroke {
            rect: rect.inflate(width / 2.0),
            radius: radius + width / 2.0,
            width,
            color: if focused {
                c.accent.with_alpha(0.75)
            } else {
                Rgba::TRANSPARENT
            },
        });
        inner
    }

    /// A centred message where a list would be (empty, loading, an error), with an icon.
    /// The height used.
    pub fn empty(&mut self, rect: Rect, icon: Option<Icon>, title: &str, detail: Option<&str>) -> f32 {
        let c = self.theme.color;
        let mut y = rect.top + self.px(12.0);
        if let Some(icon) = icon {
            let size = self.px(28.0);
            let r = Rect::new(rect.left, y, rect.width(), size);
            self.icon(r, icon, 26.0, c.tertiary);
            y += size + self.px(10.0);
        }
        let (_, h) = self.text_in(
            Rect::new(rect.left, y, rect.width(), self.line(TextRole::Headline)),
            title,
            TextRole::Headline,
            c.secondary,
            Align::Center,
        );
        y += h.max(self.line(TextRole::Headline)) + self.px(2.0);
        if let Some(detail) = detail {
            let style = self.font(TextRole::Callout);
            for line in self.text.lines(detail, &style, rect.width(), 3) {
                let (w, h) = self.text.measure(&line, &style);
                self.push(Op::Text {
                    rect: Rect::new(
                        (rect.left + (rect.width() - w) / 2.0).round(),
                        y.round(),
                        w,
                        h,
                    ),
                    text: line,
                    style: style.clone(),
                    color: c.tertiary,
                });
                y += self.line(TextRole::Callout);
            }
        }
        y + self.px(12.0) - rect.top
    }

    // ---- Scroll view -------------------------------------------------------------------

    /// A scroll view over `viewport`: `content` draws from the top-left it is given (already
    /// moved by the offset) and returns the content's height. What it draws is clipped to
    /// the viewport, its hits are cut to it (and dropped outside), and the overlay scrollbar
    /// is drawn inside the viewport's right edge. Nothing outside the viewport moves.
    pub fn scroll(
        &mut self,
        viewport: Rect,
        scroll: &Scroll,
        now: Instant,
        content: impl FnOnce(&mut Self, Point) -> f32,
    ) {
        let offset = scroll.offset(now);
        let outer = std::mem::take(&mut self.list.ops);
        let first_hit = self.list.hits.len();
        let height = content(self, Point::new(viewport.left, viewport.top - offset));
        scroll.set_extent(height, viewport.height());
        let inner = std::mem::replace(&mut self.list.ops, outer);
        self.push(Op::Clipped {
            rect: viewport,
            ops: inner.into(),
        });
        let mut i = first_hit;
        while i < self.list.hits.len() {
            let cut = self.list.hits[i].rect.intersect(viewport);
            if cut.is_empty() {
                self.list.hits.remove(i);
            } else {
                self.list.hits[i].rect = cut;
                i += 1;
            }
        }
        // The overlay scrollbar: inside the viewport, its thumb proportional to what shows.
        let m = self.theme.metrics;
        let alpha = scroll.bar(now);
        let (w, inset) = (self.px(m.scrollbar), self.px(m.scrollbar_inset));
        let track = Rect::new(
            viewport.right - inset - w,
            viewport.top + inset,
            w,
            (viewport.height() - 2.0 * inset).max(0.0),
        );
        let max = scroll.max();
        let thumb_h = if height > 0.0 {
            (track.height() * viewport.height() / height).max(self.px(28.0)).min(track.height())
        } else {
            track.height()
        };
        let y = if max > 0.0 {
            track.top + (track.height() - thumb_h) * (offset / max)
        } else {
            track.top
        };
        let color = self.theme.color.scrollbar;
        self.fill(
            Rect::new(track.left, y.round(), w, thumb_h.round()),
            w / 2.0,
            if alpha > 0.0 && max > 0.0 {
                color.with_alpha(color.a * alpha)
            } else {
                Rgba::TRANSPARENT
            },
        );
    }
}

/// Row highlights inside a card are concentric with it across the row's inset.
struct Radii;

impl Radii {
    fn concentric_row(theme: &Theme) -> f32 {
        crate::theme::Radii::concentric(theme.radius.card, 4.0, 6.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{SYSTEM_BLUE, Scheme, Style};

    fn theme() -> Theme {
        Theme::new(Style::Liquid, Scheme::Dark, SYSTEM_BLUE)
    }

    #[test]
    fn a_hover_changes_only_its_wash_and_a_click_never_shows_a_ring() {
        let mut ui = crate::Ui::new().unwrap();
        let t = theme();
        let draw = |ui: &mut crate::Ui, input: &Interaction| {
            let mut f = Frame::new(&mut ui.text, &t, 1.0, input);
            f.button(1, Rect::new(0.0, 0.0, 100.0, 30.0), "One", None, ButtonKind::Gray, true);
            f.button(2, Rect::new(0.0, 40.0, 100.0, 30.0), "Two", None, ButtonKind::Filled, true);
            f.list
        };
        let rest = draw(&mut ui, &Interaction::default());
        let hover = Interaction {
            hover: Some(2),
            ..Default::default()
        };
        let hovered = draw(&mut ui, &hover);
        assert_eq!(rest.ops.len(), hovered.ops.len(), "no op shifts");
        let changed: Vec<_> = (0..rest.ops.len())
            .filter(|&i| rest.ops[i] != hovered.ops[i])
            .collect();
        assert_eq!(changed.len(), 1, "only the wash: {changed:?}");
        let clicked = Interaction {
            hover: Some(2),
            pressed: Some(2),
            focus: Some(2),
            focus_visible: false,
        };
        let list = draw(&mut ui, &clicked);
        assert!(
            !list.ops.iter().any(|op| matches!(op, Op::Stroke { color, .. } if color.a > 0.0)),
            "no outline anywhere after a click"
        );
    }

    #[test]
    fn a_scroll_view_clips_its_content_and_hits_and_keeps_its_header() {
        let mut ui = crate::Ui::new().unwrap();
        let t = theme();
        let input = Interaction::default();
        let mut scroll = Scroll::new(&t.motion);
        let now = Instant::now();
        let viewport = Rect::new(0.0, 40.0, 200.0, 100.0);
        let draw = |ui: &mut crate::Ui, scroll: &Scroll| {
            let mut f = Frame::new(&mut ui.text, &t, 1.0, &input);
            f.hit(1, Rect::new(0.0, 0.0, 200.0, 40.0), Role::Button, "Header", true, None);
            f.scroll(viewport, scroll, now, |f, at| {
                for i in 0..10 {
                    let r = Rect::new(at.x, at.y + i as f32 * 30.0, 200.0, 30.0);
                    f.fill(r, 0.0, Rgba::HIT);
                    f.hit(10 + i, r, Role::Button, "Row", true, None);
                }
                300.0
            });
            f.list
        };
        let list = draw(&mut ui, &scroll);
        assert_eq!(scroll.max(), 200.0);
        let ids: Vec<_> = list.hits.iter().map(|h| h.id).collect();
        assert_eq!(ids, [1, 10, 11, 12, 13], "rows past the viewport have no hits");
        assert_eq!(list.hits[4].rect.bottom, 140.0, "the last row is cut to the viewport");
        assert!(scroll.wheel(-90, 1.0, now), "36 px");
        let list = draw(&mut ui, &scroll);
        assert_eq!(list.hits[0].rect, Rect::new(0.0, 0.0, 200.0, 40.0), "the header stays");
        assert_eq!(list.hits[1].id, 11, "row 10 scrolled out under the header");
        assert!(list.hits.iter().all(|h| h.id == 1 || h.rect.top >= 40.0));
    }
}
