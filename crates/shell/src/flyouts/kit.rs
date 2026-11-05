//! The flyouts' building blocks, laid out top to bottom into one display list: headers, rows,
//! tiles, sliders, switches, buttons. Lengths are design pixels times the surface's scale `s`.
//!
//! Every control is a [`Hit`] whose id the panel chooses; hover, press and keyboard focus are
//! drawn here from the ids the controller passes in, so panels only say what is where.

use std::sync::Arc;

use mypaper_ui::{DisplayList, Hit, HitId, Image, Op, Point, Rect, Rgba, Role, Text, TextStyle};

use super::look::Look;

/// Segoe Fluent Icons code points the flyouts use.
pub mod glyph {
    pub const WIFI: [char; 4] = ['\u{E872}', '\u{E873}', '\u{E874}', '\u{E701}'];
    pub const BLUETOOTH: char = '\u{E702}';
    pub const BRIGHTNESS: char = '\u{E706}';
    pub const VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
    pub const MUTE: char = '\u{E74F}';
    pub const SPEAKERS: char = '\u{E7F5}';
    pub const SETTINGS: char = '\u{E713}';
    pub const CHEVRON_RIGHT: char = '\u{E76C}';
    pub const CHEVRON_LEFT: char = '\u{E76B}';
    pub const CLOSE: char = '\u{E711}';
    pub const LOCK: char = '\u{E72E}';
    pub const BELL: char = '\u{EA8F}';
    pub const MOON: char = '\u{E708}';
    pub const FOCUS: char = '\u{E916}';
    pub const SAVER: char = '\u{E83F}';
    pub const GAUGE: char = '\u{EC4A}';
    pub const KEYBOARD: char = '\u{E765}';
    pub const POWER: char = '\u{E7E8}';
    pub const RESTART: char = '\u{E777}';
    pub const SIGN_OUT: char = '\u{E748}';
    pub const REFRESH: char = '\u{E72C}';
    pub const SEARCH: char = '\u{E721}';
    pub const APP: char = '\u{E8A9}';
    pub const CHECK: char = '\u{E73E}';
    pub const CLEAR: char = '\u{E894}';
}

/// What sits at a row's left.
#[derive(Clone)]
pub enum Icon {
    Glyph(char),
    Image(Arc<Image>),
}

/// One row of a list.
pub struct Row<'a> {
    pub id: HitId,
    pub icon: Icon,
    pub title: &'a str,
    pub detail: Option<&'a str>,
    /// A glyph at the right end (a lock, a chevron, a check).
    pub trailing: Option<char>,
    /// The current choice (the connected network, the default device).
    pub selected: bool,
    pub enabled: bool,
}

/// A quick-settings tile: a toggle with an optional "more" chevron.
pub struct Tile<'a> {
    pub id: HitId,
    pub more: Option<HitId>,
    pub glyph: char,
    pub label: &'a str,
    pub detail: &'a str,
    pub on: bool,
    pub enabled: bool,
}

/// One flyout's display list, being built.
pub struct Kit<'a> {
    pub text: &'a mut Text,
    pub look: &'a Look,
    /// Device pixels per design pixel.
    pub s: f32,
    pub list: DisplayList,
    /// The content's edges (inside the panel's padding).
    pub left: f32,
    pub right: f32,
    /// Where the next block goes.
    pub y: f32,
    /// How far down content may go (the screen's height): lists show what fits.
    pub limit: f32,
    pub hover: Option<HitId>,
    pub pressed: Option<HitId>,
    /// Keyboard focus, drawn as a ring.
    pub focus: Option<HitId>,
}

/// The panel's inner padding, design px.
pub const PAD: f32 = 14.0;
/// The id of a glyph that only decorates (an icon before a slider that is not a button).
pub const DECORATION: HitId = 0;

impl<'a> Kit<'a> {
    /// A kit for a panel `width` device pixels wide, its content starting under the padding.
    pub fn new(text: &'a mut Text, look: &'a Look, s: f32, width: f32) -> Self {
        Self {
            text,
            look,
            s,
            list: DisplayList::default(),
            left: PAD * s,
            right: width - PAD * s,
            y: PAD * s,
            limit: f32::MAX,
            hover: None,
            pressed: None,
            focus: None,
        }
    }

    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// The UI font at `size` design px and `weight`.
    pub fn font(&self, size: f32, weight: u16) -> TextStyle {
        let mut style = TextStyle::new(&self.look.font, size * self.s);
        style.weight = weight;
        style
    }

    pub fn icon_font(&self, size: f32) -> TextStyle {
        TextStyle::new(&self.look.icons, size * self.s)
    }

    pub fn fill(&mut self, rect: Rect, radius: f32, color: Rgba) {
        self.list.ops.push(Op::Fill {
            rect,
            radius,
            color,
        });
    }

    pub fn hit(&mut self, id: HitId, rect: Rect, role: Role, label: &str, enabled: bool) {
        self.hit_checked(id, rect, role, label, enabled, None);
    }

    pub fn hit_checked(
        &mut self,
        id: HitId,
        rect: Rect,
        role: Role,
        label: &str,
        enabled: bool,
        checked: Option<bool>,
    ) {
        self.list.hits.push(Hit {
            id,
            rect,
            role,
            label: label.into(),
            enabled,
            checked,
        });
    }

    /// The hover/press fill and the focus ring for control `id` at `rect`.
    ///
    /// Both are always in the list (transparent at rest): damage is compared op by op in list
    /// order, so a hover must not shift every later op and redraw the whole panel.
    pub fn state(&mut self, id: HitId, rect: Rect, radius: f32) {
        let color = if self.pressed == Some(id) {
            self.look.pressed()
        } else if self.hover == Some(id) {
            self.look.hover()
        } else {
            Rgba::TRANSPARENT
        };
        self.fill(rect, radius, color);
        self.list.ops.push(Op::Stroke {
            rect: rect.inflate(-1.0),
            radius,
            width: 2.0_f32.max(self.s.round()),
            color: if self.focus == Some(id) {
                self.look.accent
            } else {
                Rgba::TRANSPARENT
            },
        });
    }

    /// The keyboard focus ring of `id` (transparent unless focused, see [`Self::state`]).
    fn focus_ring(&mut self, id: HitId, rect: Rect, radius: f32) {
        self.list.ops.push(Op::Stroke {
            rect,
            radius,
            width: 2.0_f32.max(self.s.round()),
            color: if self.focus == Some(id) {
                self.look.accent
            } else {
                Rgba::TRANSPARENT
            },
        });
    }

    /// `text` at `(x, y)` (its top-left), cut with an ellipsis past `max` px. Returns its size.
    pub fn text_at(
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
        self.list.ops.push(Op::Text {
            rect: Rect::new(x, y, w, h),
            text: fitted,
            style: style.clone(),
            color,
        });
        (w, h)
    }

    /// `text` with its centre at `center`.
    pub fn text_centered(&mut self, center: Point, text: &str, style: &TextStyle, color: Rgba) {
        let (w, h) = self.text.measure(text, style);
        self.list.ops.push(Op::Text {
            rect: Rect::new(center.x - w / 2.0, center.y - h / 2.0, w, h),
            text: text.into(),
            style: style.clone(),
            color,
        });
    }

    pub fn glyph(&mut self, center: Point, glyph: char, size: f32, color: Rgba) {
        let style = self.icon_font(size);
        self.text_centered(center, &glyph.to_string(), &style, color);
    }

    pub fn image(&mut self, rect: Rect, image: Arc<Image>) {
        self.list.ops.push(Op::Image {
            rect,
            image,
            opacity: 1.0,
            source: None,
        });
    }

    /// A round icon button with `glyph`, `size` design px across, centred at `center`.
    /// Id [`DECORATION`] draws the glyph alone (no button).
    pub fn icon_button(&mut self, id: HitId, center: Point, glyph: char, label: &str) {
        if id == DECORATION {
            self.glyph(center, glyph, 14.0, self.look.dim());
            return;
        }
        let r = self.px(15.0);
        let rect = Rect::new(center.x - r, center.y - r, r * 2.0, r * 2.0);
        self.state(id, rect, r);
        self.glyph(center, glyph, 14.0, self.look.ink);
        self.hit(id, rect, Role::Button, label, true);
    }

    /// A title on the left and icon buttons on the right.
    pub fn header(&mut self, title: &str, buttons: &[(HitId, char, &str)]) {
        let height = self.px(32.0);
        let style = self.font(15.0, 600);
        let (_, h) = self.text.measure(title, &style);
        let room = self.width() - buttons.len() as f32 * self.px(32.0);
        let (left, y) = (self.left, self.y);
        self.text_at(
            left,
            y + (height - h) / 2.0,
            title,
            &style,
            self.look.ink,
            room,
        );
        let mut x = self.right - self.px(15.0);
        for &(id, glyph, label) in buttons.iter().rev() {
            self.icon_button(id, Point::new(x, y + height / 2.0), glyph, label);
            x -= self.px(32.0);
        }
        self.y += height + self.px(6.0);
    }

    /// A small section title.
    pub fn caption(&mut self, text: &str) {
        let style = self.font(12.0, 600);
        let (left, y, width) = (self.left, self.y, self.width());
        let (_, h) = self.text_at(left, y, text, &style, self.look.dim(), width);
        self.y += h + self.px(6.0);
    }

    /// A line of (wrapped) secondary text, at most `lines` lines.
    pub fn paragraph(&mut self, text: &str, lines: usize, color: Rgba) {
        let style = self.font(13.0, 400);
        for line in self.text.lines(text, &style, self.width(), lines) {
            let (w, h) = self.text.measure(&line, &style);
            self.list.ops.push(Op::Text {
                rect: Rect::new(self.left, self.y, w, h),
                text: line,
                style: style.clone(),
                color,
            });
            self.y += h;
        }
        self.y += self.px(4.0);
    }

    /// A list row: icon, title and detail, a trailing glyph; the whole row is one button.
    pub fn row(&mut self, row: Row) -> Rect {
        let height = self.px(if row.detail.is_some() { 48.0 } else { 38.0 });
        let rect = Rect::new(
            self.left - self.px(6.0),
            self.y,
            self.width() + self.px(12.0),
            height,
        );
        let radius = self.px(10.0);
        if row.selected {
            self.fill(rect, radius, self.look.control());
        }
        self.state(row.id, rect, radius);
        let icon_size = self.px(24.0);
        let icon_center = Point::new(self.left + icon_size / 2.0, rect.top + height / 2.0);
        let mut text_left = self.left;
        let ink = if row.enabled {
            self.look.ink
        } else {
            self.look.faint()
        };
        match &row.icon {
            Icon::Glyph(g) => {
                self.glyph(icon_center, *g, 16.0, ink);
                text_left += icon_size + self.px(10.0);
            }
            Icon::Image(image) => {
                let r = Rect::new(
                    icon_center.x - icon_size / 2.0,
                    icon_center.y - icon_size / 2.0,
                    icon_size,
                    icon_size,
                );
                self.image(r, image.clone());
                text_left += icon_size + self.px(10.0);
            }
        }
        let trailing = row.trailing.map_or(0.0, |_| self.px(24.0));
        let room = self.right - text_left - trailing;
        let title = self.font(13.0, if row.selected { 600 } else { 400 });
        let detail = self.font(12.0, 400);
        let (_, th) = self.text.measure("Ag", &title);
        let (_, dh) = self.text.measure("Ag", &detail);
        let block = th + row.detail.map_or(0.0, |_| dh);
        let top = rect.top + (height - block) / 2.0;
        self.text_at(text_left, top, row.title, &title, ink, room);
        if let Some(d) = row.detail {
            self.text_at(text_left, top + th, d, &detail, self.look.dim(), room);
        }
        if let Some(g) = row.trailing {
            let center = Point::new(self.right - self.px(10.0), rect.top + height / 2.0);
            self.glyph(center, g, 12.0, self.look.dim());
        }
        let label = match row.detail {
            Some(d) => format!("{}, {d}", row.title),
            None => row.title.to_owned(),
        };
        let role = if row.selected {
            Role::RadioButton
        } else {
            Role::Button
        };
        self.hit_checked(
            row.id,
            rect,
            role,
            &label,
            row.enabled,
            row.selected.then_some(true),
        );
        self.y += height + self.px(2.0);
        rect
    }

    /// Tiles two to a row.
    pub fn tiles(&mut self, tiles: &[Tile]) {
        let gap = self.px(8.0);
        let width = (self.width() - gap) / 2.0;
        let height = self.px(56.0);
        let radius = self.px(match self.look.family {
            mypaper_bars::style::Family::Fluent => 6.0,
            mypaper_bars::style::Family::Macos => 12.0,
            mypaper_bars::style::Family::Material => 22.0,
            mypaper_bars::style::Family::Mono => 14.0,
        });
        for (i, tile) in tiles.iter().enumerate() {
            let x = self.left + (i % 2) as f32 * (width + gap);
            let y = self.y + (i / 2) as f32 * (height + gap);
            let rect = Rect::new(x, y, width, height);
            let (fill, ink) = if tile.on && tile.enabled {
                match self.look.family {
                    mypaper_bars::style::Family::Mono => (
                        self.look.ink.with_alpha(0.85),
                        self.look.fill.with_alpha(1.0),
                    ),
                    _ => (self.look.accent, self.look.on_accent),
                }
            } else {
                (self.look.control(), self.look.ink)
            };
            let ink = if tile.enabled { ink } else { self.look.faint() };
            self.fill(rect, radius, fill);
            let more = tile.more.map_or(0.0, |_| self.px(30.0));
            let main = Rect::new(x, y, width - more, height);
            self.state(tile.id, main, radius);
            self.glyph(
                Point::new(x + self.px(22.0), y + height / 2.0),
                tile.glyph,
                16.0,
                ink,
            );
            let text_left = x + self.px(42.0);
            let room = width - self.px(48.0) - more;
            let label = self.font(13.0, 600);
            let detail = self.font(12.0, 400);
            let (_, lh) = self.text.measure("Ag", &label);
            let (_, dh) = self.text.measure("Ag", &detail);
            let top = y + (height - lh - dh) / 2.0;
            self.text_at(text_left, top, tile.label, &label, ink, room);
            self.text_at(
                text_left,
                top + lh,
                tile.detail,
                &detail,
                ink.with_alpha(ink.a * 0.75),
                room,
            );
            self.hit_checked(
                tile.id,
                main,
                Role::Switch,
                tile.label,
                tile.enabled,
                Some(tile.on),
            );
            if let Some(more) = tile.more {
                let r = Rect::new(x + width - self.px(30.0), y, self.px(30.0), height);
                self.state(more, r, radius);
                self.glyph(
                    Point::new(r.left + r.width() / 2.0, y + height / 2.0),
                    glyph::CHEVRON_RIGHT,
                    11.0,
                    ink,
                );
                self.hit(
                    more,
                    r,
                    Role::Button,
                    &format!("{} settings", tile.label),
                    true,
                );
            }
        }
        let rows = tiles.len().div_ceil(2) as f32;
        self.y += rows * height + (rows - 1.0).max(0.0) * gap + self.px(12.0);
    }

    /// A slider with an optional glyph button before it and one after; `value` is 0..=1.
    pub fn slider(
        &mut self,
        id: HitId,
        value: f32,
        label: &str,
        before: Option<(HitId, char, &str)>,
        after: Option<(HitId, char, &str)>,
    ) {
        let height = self.px(34.0);
        let center_y = self.y + height / 2.0;
        let mut left = self.left;
        let mut right = self.right;
        if let Some((bid, g, l)) = before {
            self.icon_button(bid, Point::new(left + self.px(15.0), center_y), g, l);
            left += self.px(36.0);
        }
        if let Some((aid, g, l)) = after {
            self.icon_button(aid, Point::new(right - self.px(15.0), center_y), g, l);
            right -= self.px(36.0);
        }
        let knob = self.px(9.0);
        let track = Rect::new(
            left + knob,
            center_y - self.px(2.5),
            right - left - knob * 2.0,
            self.px(5.0),
        );
        let value = value.clamp(0.0, 1.0);
        self.fill(track, track.height() / 2.0, self.look.control());
        let at = track.left + track.width() * value;
        self.fill(
            Rect { right: at, ..track },
            track.height() / 2.0,
            self.look.accent,
        );
        let k = Rect::new(at - knob, center_y - knob, knob * 2.0, knob * 2.0);
        let halo = if self.hover == Some(id) || self.pressed == Some(id) {
            self.look.hover()
        } else {
            Rgba::TRANSPARENT
        };
        self.fill(k.inflate(self.px(3.0)), knob + self.px(3.0), halo);
        self.fill(k, knob, Rgba::rgb8([250, 250, 252], 1.0));
        self.list.ops.push(Op::Stroke {
            rect: k,
            radius: knob,
            width: 1.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.18),
        });
        self.focus_ring(id, k.inflate(self.px(2.0)), knob + self.px(2.0));
        // The hit is the track (with the knob's room): the controller maps x along it.
        let rect = Rect::new(track.left, self.y, track.width(), height);
        self.hit(
            id,
            rect,
            Role::Slider,
            &format!("{label} {}%", (value * 100.0).round()),
            true,
        );
        self.y += height + self.px(4.0);
    }

    /// An on/off switch whose right edge is at `right`, vertically centred on `center_y`.
    pub fn switch(&mut self, id: HitId, on: bool, right: f32, center_y: f32, label: &str) {
        let (w, h) = (self.px(38.0), self.px(22.0));
        let rect = Rect::new(right - w, center_y - h / 2.0, w, h);
        let fill = if on {
            self.look.accent
        } else {
            self.look.ink.with_alpha(0.22)
        };
        self.fill(rect, h / 2.0, fill);
        let knob = h / 2.0 - self.px(3.0);
        let cx = if on {
            rect.right - h / 2.0
        } else {
            rect.left + h / 2.0
        };
        let color = if on {
            self.look.on_accent
        } else {
            self.look.ink
        };
        self.fill(
            Rect::new(cx - knob, center_y - knob, knob * 2.0, knob * 2.0),
            knob,
            color,
        );
        self.focus_ring(id, rect.inflate(self.px(2.0)), h / 2.0 + self.px(2.0));
        self.hit_checked(
            id,
            rect.inflate(self.px(4.0)),
            Role::Switch,
            label,
            true,
            Some(on),
        );
    }

    /// Text buttons, right-aligned; `primary` ones are accent-filled.
    pub fn buttons(&mut self, buttons: &[(HitId, &str, bool)]) {
        let height = self.px(30.0);
        let style = self.font(13.0, 500);
        let mut x = self.right;
        for &(id, label, primary) in buttons.iter().rev() {
            let (w, h) = self.text.measure(label, &style);
            let rect = Rect::new(x - w - self.px(28.0), self.y, w + self.px(28.0), height);
            let (fill, ink) = if primary {
                (self.look.accent, self.look.on_accent)
            } else {
                (self.look.control(), self.look.ink)
            };
            self.fill(rect, self.px(8.0), fill);
            self.state(id, rect, self.px(8.0));
            self.list.ops.push(Op::Text {
                rect: Rect::new(
                    rect.left + self.px(14.0),
                    rect.top + (height - h) / 2.0,
                    w,
                    h,
                ),
                text: label.into(),
                style: style.clone(),
                color: ink,
            });
            self.hit(id, rect, Role::Button, label, true);
            x = rect.left - self.px(8.0);
        }
        self.y += height + self.px(8.0);
    }

    /// A text link-like row at the bottom: `label` with a chevron, as a button.
    pub fn footer_link(&mut self, id: HitId, label: &str) {
        self.separator();
        let height = self.px(32.0);
        let rect = Rect::new(
            self.left - self.px(6.0),
            self.y,
            self.width() + self.px(12.0),
            height,
        );
        self.state(id, rect, self.px(8.0));
        let style = self.font(13.0, 400);
        let (_, h) = self.text.measure(label, &style);
        let (left, room) = (self.left, self.width() - self.px(20.0));
        self.text_at(
            left,
            rect.top + (height - h) / 2.0,
            label,
            &style,
            self.look.ink,
            room,
        );
        self.glyph(
            Point::new(self.right - self.px(8.0), rect.top + height / 2.0),
            glyph::CHEVRON_RIGHT,
            11.0,
            self.look.dim(),
        );
        self.hit(id, rect, Role::Link, label, true);
        self.y += height;
    }

    pub fn separator(&mut self) {
        let y = self.y + self.px(6.0);
        let rect = Rect::new(self.left, y, self.width(), 1.0_f32.max(self.s.floor()));
        self.fill(rect, 0.0, self.look.line());
        self.y = y + self.px(7.0);
    }

    /// A centred message in place of a list (empty, error, loading).
    pub fn notice(&mut self, text: &str) {
        let style = self.font(13.0, 400);
        let lines = self.text.lines(text, &style, self.width(), 3);
        self.y += self.px(10.0);
        for line in lines {
            let (w, h) = self.text.measure(&line, &style);
            self.list.ops.push(Op::Text {
                rect: Rect::new(self.left + (self.width() - w) / 2.0, self.y, w, h),
                text: line,
                style: style.clone(),
                color: self.look.dim(),
            });
            self.y += h;
        }
        self.y += self.px(10.0);
    }

    /// The panel's body, filling the whole window: glass under it (an invisible hit fill), or
    /// the style's own fill. Goes first, under everything else.
    pub fn body(list: &mut DisplayList, size: (f32, f32), radius: f32, look: &Look, glass: bool) {
        let rect = Rect::new(0.0, 0.0, size.0, size.1);
        let mut ops = vec![Op::Fill {
            rect,
            radius,
            color: if glass { Rgba::HIT } else { look.fill },
        }];
        ops.push(Op::Stroke {
            rect: rect.inflate(-0.5),
            radius,
            width: 1.0,
            color: look.ink.with_alpha(if glass { 0.16 } else { 0.12 }),
        });
        ops.append(&mut list.ops);
        list.ops = ops;
    }
}
