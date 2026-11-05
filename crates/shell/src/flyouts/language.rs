//! Keyboard layouts: the installed ones, the one the app in front uses checked; choosing one
//! asks that app's window to switch (`WM_INPUTLANGCHANGEREQUEST`).

use mypaper_bars::flyouts::language::{self, Layout};
use mypaper_bars::system;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::Stack;
use mypaper_ui::{Accessory, HitId, Lead, ListRow};

use super::kit::Kit;
use super::{Cx, Effect, LIST, Panel};

const SETTINGS: HitId = 1;

thread_local! {
    /// The layouts when the panel last closed (the UI thread's).
    static LAST: std::cell::RefCell<(Vec<Layout>, Option<String>)> =
        const { std::cell::RefCell::new((Vec::new(), None)) };
}

impl Drop for Language {
    fn drop(&mut self) {
        let last = (std::mem::take(&mut self.layouts), self.current.take());
        LAST.with_borrow_mut(|l| *l = last);
    }
}

pub struct Language {
    layouts: Vec<Layout>,
    current: Option<String>,
    /// The app in front when it opened (before the flyout took the focus).
    window: isize,
}

impl Language {
    pub fn new(cx: &mut Cx) -> Self {
        let window = cx.bars.external_foreground();
        // The layouts come once it is on screen (a registry walk, ~15 ms); until then, the ones
        // last shown, so a reopen keeps its layout.
        let (layouts, current) = LAST.with_borrow(Clone::clone);
        Self::with(layouts, current, window)
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
        kit.frame(|f, area| {
            let mut column = Stack::column(area, f.px(2.0));
            f.section_header(column.next(f.px(24.0)), "Keyboard layout", None);
            if self.layouts.is_empty() {
                let h = f.empty(column.rest(), Some(Icon::Keyboard), "No keyboard layouts", None);
                column.next(h);
            }
            for (i, layout) in self.layouts.iter().enumerate() {
                let current = self.current.as_deref() == Some(layout.id.as_str());
                let row = ListRow {
                    id: LIST + i as HitId,
                    lead: Lead::Badge(Icon::Keyboard, current),
                    title: &layout.language,
                    subtitle: Some(&layout.layout),
                    accessory: if current { Accessory::Check } else { Accessory::None },
                    selected: false,
                    enabled: true,
                };
                let h = f.row_height(&row);
                f.row(&row, column.next(h));
            }
            super::kit::footer(f, &mut column, SETTINGS, "Language settings");
            area.top + column.used()
        });
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

