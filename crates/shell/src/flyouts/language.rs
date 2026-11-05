//! Keyboard layouts: the installed ones, the one the app in front uses checked; choosing one
//! asks that app's window to switch (`WM_INPUTLANGCHANGEREQUEST`).

use mypaper_bars::flyouts::language::{self, Layout};
use mypaper_bars::system;
use mypaper_ui::HitId;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::kit::{Icon, Kit, Row, glyph};
use super::{Cx, Effect, LIST, Panel};

const SETTINGS: HitId = 1;

pub struct Language {
    layouts: Vec<Layout>,
    current: Option<String>,
    /// The app in front when it opened (before the flyout took the focus).
    window: isize,
}

impl Language {
    pub fn new(_: &mut Cx) -> Self {
        // SAFETY: a read-only query; the flyout has not taken the focus yet.
        let window = unsafe { GetForegroundWindow() }.0 as isize;
        // The layouts come once it is on screen (a registry walk, ~15 ms).
        Self::with(Vec::new(), None, window)
    }

    pub fn with(layouts: Vec<Layout>, current: Option<String>, window: isize) -> Self {
        Self {
            layouts,
            current,
            window,
        }
    }
}

impl Panel for Language {
    fn width(&self) -> f32 {
        300.0
    }

    fn view(&self, kit: &mut Kit) {
        kit.caption("Keyboard layout");
        if self.layouts.is_empty() {
            kit.notice("No keyboard layouts");
        }
        for (i, layout) in self.layouts.iter().enumerate() {
            let current = self.current.as_deref() == Some(layout.id.as_str());
            kit.row(Row {
                id: LIST + i as HitId,
                icon: Icon::Glyph(glyph::KEYBOARD),
                title: &layout.language,
                detail: Some(&layout.layout),
                trailing: current.then_some(glyph::CHECK),
                selected: current,
                enabled: true,
            });
        }
        kit.footer_link(SETTINGS, "Language settings");
    }

    fn opened(&mut self, _: &mut Cx) -> bool {
        self.layouts = language::layouts();
        self.current = language::current(Some(self.window));
        true
    }

    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        if id == SETTINGS {
            system::open("ms-settings:regionlanguage".into());
            return Effect::Close;
        }
        let Some(layout) = id
            .checked_sub(LIST)
            .and_then(|i| self.layouts.get(i as usize))
        else {
            return Effect::None;
        };
        if let Err(e) = language::switch(self.window, &layout.id) {
            log::warn!("language: {e}");
        }
        Effect::Close
    }
}
