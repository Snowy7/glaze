//! Power: lock, sleep, sign out, restart and shut down. The last three ask first (a second
//! click on the confirmation); lock and sleep go at once.

use mypaper_bars::actions::PowerAction;
use mypaper_bars::system;
use mypaper_ui::HitId;

use super::kit::{Icon, Kit, Row, glyph};
use super::{Cx, Effect, LIST, Panel};

const CANCEL: HitId = 1;
const CONFIRM: HitId = 2;

const ACTIONS: [(PowerAction, &str, char); 5] = [
    (PowerAction::Lock, "Lock", glyph::LOCK),
    (PowerAction::Sleep, "Sleep", glyph::MOON),
    (PowerAction::Signout, "Sign out", glyph::SIGN_OUT),
    (PowerAction::Restart, "Restart", glyph::RESTART),
    (PowerAction::Shutdown, "Shut down", glyph::POWER),
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
        240.0
    }

    fn view(&self, kit: &mut Kit) {
        if let Some(i) = self.asking {
            let (_, label, g) = ACTIONS[i];
            kit.row(Row {
                id: LIST + i as HitId,
                icon: Icon::Glyph(g),
                title: &format!("{label} now?"),
                detail: Some("Apps that are open may lose unsaved work."),
                trailing: None,
                selected: false,
                enabled: false,
            });
            kit.buttons(&[(CANCEL, "Cancel", false), (CONFIRM, label, true)]);
            return;
        }
        for (i, (_, label, g)) in ACTIONS.iter().enumerate() {
            kit.row(Row {
                id: LIST + i as HitId,
                icon: Icon::Glyph(*g),
                title: label,
                detail: None,
                trailing: None,
                selected: false,
                enabled: true,
            });
        }
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
