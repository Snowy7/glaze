//! Tray overflow: the notification-area icons the bar doesn't show, four to a row. Clicks go
//! to each icon's app exactly as from the bar (the tray host's versioned callbacks, with the
//! icon's screen rectangle as the anchor), so its own menus and windows open.

use std::sync::Arc;

use mypaper_bars::tray::OVERFLOW;
use mypaper_bars::tray_data::TrayItem;
use mypaper_ui::{Button, HitId, Image, Point, Rect, Role};

use super::kit::{Kit, glyph};
use super::{Cx, Effect, LIST, Panel};

const COLUMNS: usize = 4;

pub struct Overflow {
    items: Vec<TrayItem>,
    icons: Vec<Option<Arc<Image>>>,
}

impl Overflow {
    pub fn new(cx: &mut Cx) -> Self {
        let mut overflow = Self {
            items: Vec::new(),
            icons: Vec::new(),
        };
        overflow.refresh(cx);
        overflow
    }

    /// The overflowing icons: unpinned ones, without the chevron itself.
    pub fn of(tray: &[TrayItem]) -> Vec<TrayItem> {
        tray.iter()
            .filter(|i| !i.pinned && i.id != OVERFLOW)
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub fn with(items: Vec<TrayItem>) -> Self {
        Self {
            icons: vec![None; items.len()],
            items,
        }
    }

    fn click(&mut self, id: HitId, button: Button, cx: &mut Cx) -> Effect {
        let Some(item) = id
            .checked_sub(LIST)
            .and_then(|i| self.items.get(i as usize))
        else {
            return Effect::None;
        };
        let hit = cx.hit;
        let point = (
            (hit.left + hit.width() / 2.0) as i32,
            (hit.top + hit.height() / 2.0) as i32,
        );
        cx.bars.click_tray(&item.id, button, point, hit);
        // The app shows its own menu or window, which takes the focus and closes this.
        Effect::None
    }
}

impl Panel for Overflow {
    fn width(&self) -> f32 {
        (COLUMNS as f32) * 44.0 + 2.0 * super::kit::PAD - 4.0
    }

    fn view(&self, kit: &mut Kit) {
        if self.items.is_empty() {
            kit.notice("No more icons");
            return;
        }
        let cell = kit.px(44.0);
        let icon = kit.px(20.0);
        for (i, item) in self.items.iter().enumerate() {
            let x = kit.left - kit.px(2.0) + (i % COLUMNS) as f32 * cell;
            let y = kit.y + (i / COLUMNS) as f32 * cell;
            let rect = Rect::new(x, y, cell - kit.px(4.0), cell - kit.px(4.0));
            let id = LIST + i as HitId;
            kit.state(id, rect, kit.px(10.0));
            let center = Point::new(
                rect.left + rect.width() / 2.0,
                rect.top + rect.height() / 2.0,
            );
            match self.icons.get(i).cloned().flatten() {
                Some(image) => kit.image(
                    Rect::new(center.x - icon / 2.0, center.y - icon / 2.0, icon, icon),
                    image,
                ),
                None => kit.glyph(center, glyph::APP, 16.0, kit.look.dim()),
            }
            kit.hit(id, rect, Role::Button, &item.name, true);
        }
        kit.y += self.items.len().div_ceil(COLUMNS) as f32 * cell;
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.click(id, Button::Left, cx)
    }

    fn context(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.click(id, Button::Right, cx)
    }

    fn refresh(&mut self, cx: &mut Cx) -> bool {
        let items = Self::of(cx.bars.tray());
        if items == self.items {
            return false;
        }
        self.icons = items.iter().map(|i| cx.image(i.icon.as_deref())).collect();
        self.items = items;
        true
    }
}
