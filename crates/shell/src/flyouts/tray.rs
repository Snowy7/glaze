//! Tray overflow: the notification-area icons the bar doesn't show, four to a row. Clicks go
//! to each icon's app exactly as from the bar (the tray host's versioned callbacks, with the
//! icon's screen rectangle as the anchor), so its own menus and windows open.

use std::sync::Arc;

use mypaper_bars::tray::OVERFLOW;
use mypaper_bars::tray_data::TrayItem;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{self, Align};
use mypaper_ui::{Button, HitId, Image, Op, Role};

use super::kit::Kit;
use super::{Cx, Effect, LIST, Panel};

const COLUMNS: usize = 4;
/// A cell, design px.
const CELL: f32 = 44.0;

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
        // Four cells and the kit's padding (12 px each side, Liquid's `metrics.pad`).
        COLUMNS as f32 * CELL + 2.0 * 12.0
    }

    fn view(&self, kit: &mut Kit) {
        kit.frame(|f, area| {
            if self.items.is_empty() {
                return area.top + f.empty(area, Some(Icon::Apps), "No more icons", None);
            }
            let c = f.theme.color;
            let cell = f.px(CELL);
            let gap = f.px(4.0);
            let size = f.px(20.0);
            let radius = f.px(f.theme.radius.small + 4.0);
            let rows = self.items.len().div_ceil(COLUMNS);
            for (i, item) in self.items.iter().enumerate() {
                let rect = layout::grid(area, COLUMNS, gap, cell - gap, i);
                let id = LIST + i as HitId;
                f.wash(id, rect, radius, true);
                match self.icons.get(i).cloned().flatten() {
                    Some(image) => f.push(Op::Image {
                        rect: rect.place(size, size, Align::Center, Align::Center),
                        image,
                        opacity: 1.0,
                        source: None,
                    }),
                    None => f.icon(rect, Icon::Apps, 16.0, c.secondary),
                }
                f.focus_ring(id, rect, radius);
                f.hit(id, rect, Role::Button, &item.name, true, None);
            }
            area.top + rows as f32 * cell - gap
        });
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.click(id, Button::Left, cx)
    }

    /// The app's own menu opens over the overflow, which stays open behind it (as Windows'
    /// does) until something else comes to the front.
    fn context(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        match self.click(id, Button::Right, cx) {
            Effect::None => Effect::HoldForMenu,
            effect => effect,
        }
    }
    fn middle(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.click(id, Button::Middle, cx)
    }
    fn double(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        if let Some(item) = id
            .checked_sub(LIST)
            .and_then(|i| self.items.get(i as usize))
        {
            cx.bars
                .double_click_tray(&item.id, (cx.hit.left as i32, cx.hit.top as i32));
        }
        Effect::None
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
