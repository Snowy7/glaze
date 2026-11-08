//! A flyout's display list being built, and the few blocks several flyouts share. Panels draw
//! with the UI kit (`glaze_ui::kit`, docs/ui.md "The kit") through [`Kit::frame`]; this keeps
//! where the next block goes, the room the screen leaves, and the input state the controller
//! passes in. Lengths are device pixels: design pixels times the surface's scale `s`.

use glaze_ui::interact::Interaction;
use glaze_ui::kit::Frame;
use glaze_ui::{DisplayList, HitId, Op, Rect, Rgba, Text, Theme};

use super::look::Look;

/// One flyout's display list, being built.
pub struct Kit<'a> {
    pub text: &'a mut Text,
    pub look: &'a Look,
    /// The kit's theme for this look (`Look::theme`).
    pub theme: Theme,
    /// Device pixels per design pixel.
    pub s: f32,
    pub list: DisplayList,
    /// The content's edges (inside the panel's padding).
    pub left: f32,
    pub right: f32,
    /// Where the next block goes.
    pub y: f32,
    /// How far down content may go (the screen's height): lists scroll inside what is left.
    pub limit: f32,
    pub hover: Option<HitId>,
    pub pressed: Option<HitId>,
    /// Keyboard focus, drawn as a ring.
    pub focus: Option<HitId>,
    pub motion: Option<&'a glaze_ui::interact::motion::InteractionMotion>,
}

impl<'a> Kit<'a> {
    /// A kit for a panel `width` device pixels wide, its content starting under the padding.
    pub fn new(text: &'a mut Text, look: &'a Look, s: f32, width: f32) -> Self {
        let theme = look.theme();
        let pad = (theme.metrics.pad * s).round();
        Self {
            text,
            look,
            theme,
            s,
            list: DisplayList::default(),
            left: pad,
            right: width - pad,
            y: pad,
            limit: f32::MAX,
            hover: None,
            pressed: None,
            focus: None,
            motion: None,
        }
    }

    /// The panel's padding, device px.
    pub fn pad(&self) -> f32 {
        (self.theme.metrics.pad * self.s).round()
    }

    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    /// Draws with the kit (docs/ui.md "The kit"): `draw` gets a [`Frame`] in this flyout's theme
    /// and input state, and the room left (from `y` to `limit`, inside the padding). It returns
    /// the bottom of what it drew, where the next block goes.
    pub fn frame(&mut self, draw: impl FnOnce(&mut Frame, Rect) -> f32) {
        let input = Interaction {
            hover: self.hover,
            pressed: self.pressed,
            focus: self.focus,
            focus_visible: self.focus.is_some(),
        };
        let area = Rect::new(
            self.left,
            self.y,
            self.right - self.left,
            (self.limit - self.y).max(0.0),
        );
        let mut f = Frame::new(self.text, &self.theme, self.s, &input);
        if let Some(motion) = self.motion {
            f = f.with_motion(motion);
        }
        let bottom = draw(&mut f, area);
        self.list.ops.append(&mut f.list.ops);
        self.list.hits.append(&mut f.list.hits);
        self.y = bottom.round();
    }

    /// The panel's body, filling the whole window at the theme's surface radius (`radius`,
    /// device px): with glass only an invisible hit fill (the engine draws the glass and its one
    /// rim, `Frame::surface`'s contract); without, the theme's surface and, for Solid, its one
    /// hairline. Goes first, under everything else.
    pub fn body(list: &mut DisplayList, size: (f32, f32), radius: f32, theme: &Theme, glass: bool) {
        let rect = Rect::new(0.0, 0.0, size.0, size.1);
        let mut ops = vec![Op::Fill {
            rect,
            radius,
            color: if glass {
                Rgba::HIT
            } else {
                theme.color.surface
            },
        }];
        if let Some(border) = theme.border {
            ops.push(Op::Stroke {
                rect: rect.inflate(-0.5),
                radius: (radius - 0.5).max(0.0),
                width: 1.0,
                color: border,
            });
        }
        ops.append(&mut list.ops);
        list.ops = ops;
    }
}

/// A flyout's last row, from `column`: a separator, then `label` with a chevron (it opens
/// Settings or another flyout).
pub fn footer(f: &mut Frame, column: &mut glaze_ui::Stack, id: HitId, label: &str) {
    let line = column.next(f.px(9.0));
    f.separator(line.left, line.right, line.top + f.px(4.0));
    link(f, column, id, label);
}

/// A row from `column` with `label` and a chevron: a link to Settings or another flyout.
pub fn link(f: &mut Frame, column: &mut glaze_ui::Stack, id: HitId, label: &str) {
    let row = glaze_ui::ListRow {
        id,
        lead: glaze_ui::Lead::None,
        title: label,
        subtitle: None,
        accessory: glaze_ui::Accessory::Chevron,
        selected: false,
        enabled: true,
    };
    let h = f.row_height(&row);
    f.row(&row, column.next(h));
}

/// A card with a title row (icon buttons at its right) over a Control Center slider (`slider`:
/// id, value 0..=1, accessible name, icon). Its height is [`slider_card_height`].
pub fn slider_card(
    f: &mut Frame,
    rect: Rect,
    title: &str,
    buttons: &[(HitId, glaze_ui::icons::Icon, &str)],
    slider: (HitId, f32, &str, glaze_ui::icons::Icon),
) {
    let c = f.theme.color;
    let inner = f.card(rect);
    let (head, rest) = inner.take_top(f.px(24.0), f.px(6.0));
    let mut label = head;
    for &(id, icon, name) in buttons.iter().rev() {
        let (button, left) = label.take_right(f.px(26.0), f.px(2.0));
        f.icon_button(id, button, icon, 26.0, name, false);
        label = left;
    }
    f.text_in(
        label,
        title,
        glaze_ui::TextRole::Headline,
        c.label,
        glaze_ui::Align::Start,
    );
    let (id, value, name, icon) = slider;
    f.slider(id, rest, value, name, Some(icon));
}

/// [`slider_card`]'s height, device px.
pub fn slider_card_height(f: &Frame) -> f32 {
    f.px(24.0) + f.px(6.0) + f.px(28.0) + 2.0 * f.px(f.theme.metrics.pad)
}

/// The volume glyph for `level` (0..=1), or muted.
pub fn volume_icon(level: f32, muted: bool) -> glaze_ui::icons::Icon {
    use glaze_ui::icons::Icon;
    if muted || level == 0.0 {
        Icon::Mute
    } else {
        Icon::Volume(((level * 3.0).ceil() as u8).min(3))
    }
}
