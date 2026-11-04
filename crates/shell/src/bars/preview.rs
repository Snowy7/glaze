//! A native dock preview: software card frames, DWM's live thumbnails and typed window actions.
use super::look::Tokens;
use anyhow::Result;
use mypaper_bars::{apps::Item, preview::Previews};
use mypaper_ui::{
    DisplayList, Hit, Kind, Op, Point, Rect, Rgba, Role, Surface, SurfaceId, TextStyle, Ui,
};

pub struct Preview {
    pub surface: Surface,
    pub key: String,
    pub item: Item,
    _thumbnails: Previews,
}
pub fn view(item: &Item, tokens: &Tokens, scale: f32, ui: &mut Ui) -> (DisplayList, (u32, u32)) {
    let count = item.windows.len().clamp(1, 8);
    let (w, h) = (212.0 * scale, 160.0 * scale);
    let mut list = DisplayList::default();
    for (i, window) in item.windows.iter().take(count).enumerate() {
        let rect = Rect::new(
            i as f32 * w + 4.0 * scale,
            4.0 * scale,
            w - 8.0 * scale,
            h - 8.0 * scale,
        );
        list.ops.extend([
            Op::Fill {
                rect,
                radius: tokens.radius * scale,
                color: Rgba::HIT,
            },
            Op::Fill {
                rect,
                radius: tokens.radius * scale,
                color: tokens.fill.with_alpha(0.96),
            },
        ]);
        let title = window.title.chars().take(24).collect::<String>();
        let font = tokens.font.scaled(scale);
        let (tw, th) = ui.text.measure(&title, &font);
        list.ops.push(Op::Text {
            rect: Rect::new(rect.left + 8.0 * scale, rect.top + 5.0 * scale, tw, th),
            text: title.into(),
            style: font,
            color: tokens.ink,
        });
        let close = Rect::new(
            rect.right - 25.0 * scale,
            rect.top,
            25.0 * scale,
            25.0 * scale,
        );
        list.ops.push(Op::Text {
            rect: close,
            text: "×".into(),
            style: TextStyle::new("Segoe UI", 16.0 * scale),
            color: tokens.ink,
        });
        list.hits.push(Hit {
            id: i as u32 * 2 + 1,
            rect,
            role: Role::Button,
            label: window.title.clone().into(),
            enabled: true,
            checked: None,
        });
        list.hits.push(Hit {
            id: i as u32 * 2 + 2,
            rect: close,
            role: Role::Button,
            label: format!("Close {}", window.title).into(),
            enabled: true,
            checked: None,
        });
    }
    (list, ((w * count as f32).ceil() as u32, h.ceil() as u32))
}
impl Preview {
    pub fn open(
        ui: &mut Ui,
        item: Item,
        anchor: Rect,
        tokens: &Tokens,
        scale: f32,
    ) -> Result<Self> {
        let mut surface = Surface::new(
            Kind::Popup { activate: false },
            "Window previews",
            Role::Toolbar,
        )?;
        let (list, size) = view(&item, tokens, scale, ui);
        let screen = mypaper_ui::screen_at(anchor.left as i32, anchor.top as i32);
        let x = ((anchor.left + anchor.right - size.0 as f32) / 2.0).round() as i32;
        let y = if anchor.top > (screen.work.1 + screen.work.3) as f32 / 2.0 {
            anchor.top as i32 - size.1 as i32 - 8
        } else {
            anchor.bottom as i32 + 8
        };
        let origin = (
            x.clamp(
                screen.work.0,
                (screen.work.2 - size.0 as i32).max(screen.work.0),
            ),
            y.clamp(
                screen.work.1,
                (screen.work.3 - size.1 as i32).max(screen.work.1),
            ),
        );
        surface.present(ui, origin, size, list)?;
        let mut thumbnails = Previews::default();
        let hwnds: Vec<_> = item.windows.iter().take(8).map(|w| w.hwnd).collect();
        let sizes = thumbnails.open(surface.hwnd(), &hwnds);
        let placements: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let factor = (196.0 * scale / s.width as f32).min(116.0 * scale / s.height as f32);
                (
                    s.hwnd,
                    [
                        (i as f32 * 212.0 * scale + 8.0 * scale) as f64,
                        (32.0 * scale) as f64,
                        (s.width as f32 * factor) as f64,
                        (s.height as f32 * factor) as f64,
                    ],
                )
            })
            .collect();
        thumbnails.place(&placements, 1.0);
        surface.show();
        Ok(Self {
            key: item.key.clone(),
            item,
            surface,
            _thumbnails: thumbnails,
        })
    }
    pub fn owns(&self, id: SurfaceId) -> bool {
        self.surface.id() == id
    }
    pub fn hit(&self, p: Point) -> Option<(isize, bool)> {
        let id = self.surface.shown().hit_at(p)?.id;
        self.item
            .windows
            .get((id as usize - 1) / 2)
            .map(|w| (w.hwnd, id % 2 == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_cards_and_close_buttons_take_clicks() {
        let mut ui = Ui::new().unwrap();
        let app = Item {
            key: "app".into(),
            name: "App".into(),
            path: "app.exe".into(),
            icon: None,
            pinned: true,
            windows: vec![mypaper_bars::apps::ItemWindow {
                hwnd: 1,
                title: "A document".into(),
                active: true,
                minimized: false,
                attention: false,
            }],
            aumid: None,
            exe: None,
        };
        let tokens = Tokens::resolve(
            &Default::default(),
            &Default::default(),
            &mypaper_bars::accent::read(),
        );
        let (list, size) = view(&app, &tokens, 1.0, &mut ui);
        let image = mypaper_ui::render(&mut ui, &list, size.0, size.1).unwrap();
        for hit in &list.hits {
            let p = Point::new(
                (hit.rect.left + hit.rect.right) / 2.0,
                (hit.rect.top + hit.rect.bottom) / 2.0,
            );
            assert!(list.hit_at(p).is_some());
            assert!(image.pixel(p.x as u32, p.y as u32)[3] > 0);
        }
    }
}
