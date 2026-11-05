//! Power: lock, sleep, sign out, restart and shut down. The last three ask first (a second
//! click on the confirmation); lock and sleep go at once.

use mypaper_bars::actions::PowerAction;
use mypaper_bars::system;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{Align, Stack};
use mypaper_ui::{Accessory, ButtonKind, HitId, Lead, ListRow, Rect, TextRole};

use super::kit::Kit;
use super::{Cx, Effect, LIST, Panel};

const CANCEL: HitId = 1;
const CONFIRM: HitId = 2;

const ACTIONS: [(PowerAction, &str, Icon); 5] = [
    (PowerAction::Lock, "Lock", Icon::Lock),
    (PowerAction::Sleep, "Sleep", Icon::Moon),
    (PowerAction::Signout, "Sign out", Icon::SignOut),
    (PowerAction::Restart, "Restart", Icon::Restart),
    (PowerAction::Shutdown, "Shut down", Icon::Power),
];

#[derive(Default)]
pub struct Power {
    /// The action waiting for its confirmation.
    asking: Option<usize>,
}

impl Power {
    pub fn new(_: &mut Cx) -> Self {
        Self::default()
    }

    /// Whether `action` needs a confirmation (it ends the session or the running apps).
    pub fn confirms(action: PowerAction) -> bool {
        matches!(
            action,
            PowerAction::Signout | PowerAction::Restart | PowerAction::Shutdown
        )
    }

    #[cfg(test)]
    pub fn asking(index: usize) -> Self {
        Self {
            asking: Some(index),
        }
    }
}

impl Panel for Power {
    fn width(&self) -> f32 {
        260.0
    }

    fn view(&self, kit: &mut Kit) {
        let asking = self.asking;
        kit.frame(|f, area| {
            let c = f.theme.color;
            let mut column = Stack::column(area, f.px(2.0));
            if let Some(i) = asking {
                // The question, why it asks, and the two answers.
                let (_, label, icon) = ACTIONS[i];
                column = Stack::column(area, f.px(10.0));
                let head = column.next(f.px(32.0));
                let (glyph, title) = head.take_left(f.px(32.0), f.px(10.0));
                f.fill(glyph, glyph.height() / 2.0, c.fill);
                f.icon(glyph, icon, 16.0, c.label);
                f.text_in(title, &format!("{label} now?"), TextRole::Title3, c.label, Align::Start);
                let detail = "Apps that are open may lose unsaved work.";
                let width = column.rect.width();
                let lines = f.line_count(detail, TextRole::Callout, width, 3);
                let at = column.next(lines as f32 * f.line(TextRole::Callout));
                f.paragraph(at.left, at.top, width, detail, TextRole::Callout, c.secondary, 3);
                let row = column.next(f.px(f.theme.metrics.control));
                let gap = f.px(8.0);
                let w = (row.width() - gap) / 2.0;
                f.button(CANCEL, Rect { right: row.left + w, ..row }, "Cancel", None, ButtonKind::Gray, true);
                f.button(CONFIRM, Rect { left: row.right - w, ..row }, label, None, ButtonKind::Filled, true);
                return area.top + column.used();
            }
            for (i, (_, label, icon)) in ACTIONS.iter().enumerate() {
                let row = ListRow {
                    id: LIST + i as HitId,
                    lead: Lead::Icon(*icon),
                    title: label,
                    subtitle: None,
                    accessory: Accessory::None,
                    selected: false,
                    enabled: true,
                };
                let h = f.row_height(&row);
                f.row(&row, column.next(h));
            }
            area.top + column.used()
        });
    }

    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        match id {
            CANCEL => {
                self.asking = None;
                Effect::Redraw
            }
            CONFIRM => {
                let Some(i) = self.asking.take() else {
                    return Effect::None;
                };
                system::power(ACTIONS[i].0);
                Effect::Close
            }
            id => {
                let Some(i) = id
                    .checked_sub(LIST)
                    .map(|i| i as usize)
                    .filter(|&i| i < ACTIONS.len())
                else {
                    return Effect::None;
                };
                let action = ACTIONS[i].0;
                if Self::confirms(action) {
                    self.asking = Some(i);
                    return Effect::Redraw;
                }
                system::power(action);
                Effect::Close
            }
        }
    }
}
