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
    start: usize,
    capacity: usize,
    tokens: Tokens,
    scale: f32,
    anchor: Rect,
    vertical: bool,
}

#[derive(Clone, Copy, Debug)]
struct Grid {
    cols: usize,
    rows: usize,
    width: f32,
    height: f32,
    capacity: usize,
}
impl Grid {
    fn fit(count: usize, room: (f32, f32), scale: f32, vertical: bool) -> Self {
        let cols = if vertical {
            1
        } else {
            (room.0 / (160.0 * scale)).floor().max(1.0) as usize
        }
        .min(count.max(1));
        let max_rows = (room.1 / (112.0 * scale)).floor().max(1.0) as usize;
        let rows = count.max(1).div_ceil(cols).min(max_rows);
        Self {
            cols,
            rows,
            width: (212.0 * scale).min(room.0 / cols as f32).max(1.0),
            height: (160.0 * scale).min(room.1 / rows as f32).max(1.0),
            capacity: cols * rows,
        }
    }
}

fn glass(tokens: &Tokens, scale: f32, size: (u32, u32)) -> Option<mypaper_ui::Glass> {
    tokens
        .material
        .filter(|_| mypaper_ui::glass::available())
        .map(|m| mypaper_ui::Glass {
            body: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
            radius: tokens.radius * scale,
            material: mypaper_ui::GlassMaterial {
                clarity: m.clarity,
                blur: m.blur * scale,
                refraction: m.refraction,
                highlight: m.highlight,
                tint: m.tint,
            },
        })
}
#[cfg(test)]
pub fn view(item: &Item, tokens: &Tokens, scale: f32, ui: &mut Ui) -> (DisplayList, (u32, u32)) {
    let grid = Grid::fit(
        item.windows.len().clamp(1, 8),
        (2000.0 * scale, 160.0 * scale),
        scale,
        false,
    );
    view_grid(item, tokens, scale, ui, grid, 0)
}
fn view_grid(
    item: &Item,
    tokens: &Tokens,
    scale: f32,
    ui: &mut Ui,
    grid: Grid,
    start: usize,
) -> (DisplayList, (u32, u32)) {
    let (w, h) = (grid.width, grid.height);
    let size = (
        (w * grid.cols as f32).ceil() as u32,
        (h * grid.rows as f32).ceil() as u32,
    );
    let mut list = DisplayList::default();
    list.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        radius: tokens.radius * scale,
        color: Rgba::HIT,
    });
    for (i, window) in item
        .windows
        .iter()
        .enumerate()
        .skip(start)
        .take(grid.capacity)
    {
        let local = i - start;
        let rect = Rect::new(
            (local % grid.cols) as f32 * w + 4.0 * scale,
            (local / grid.cols) as f32 * h + 4.0 * scale,
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
                color: if glass(tokens, scale, size).is_some() {
                    tokens.fill.with_alpha(0.04)
                } else {
                    tokens.fill.with_alpha(0.96)
                },
            },
        ]);
        let mut title = window.title.clone();
        let font = tokens.font.scaled(scale);
        while title.chars().count() > 1
            && ui.text.measure(&title, &font).0 > (rect.width() - 40.0 * scale).max(1.0)
        {
            title.pop();
        }
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
    (list, size)
}
impl Preview {
    pub fn open(
        ui: &mut Ui,
        item: Item,
        anchor: Rect,
        tokens: &Tokens,
        scale: f32,
        vertical: bool,
    ) -> Result<Self> {
        Self::open_at(ui, item, anchor, tokens, scale, 0, vertical)
    }
    fn open_at(
        ui: &mut Ui,
        item: Item,
        anchor: Rect,
        tokens: &Tokens,
        scale: f32,
        start: usize,
        vertical: bool,
    ) -> Result<Self> {
        let screen = mypaper_ui::screen_at(anchor.left as i32, anchor.top as i32);
        let room = (
            (screen.work.2 - screen.work.0) as f32,
            (screen.work.3 - screen.work.1) as f32,
        );
        let grid = Grid::fit(
            item.windows.len(),
            (room.0 - 16.0, room.1 - 16.0),
            scale,
            vertical,
        );
        let mut surface = Surface::new(
            Kind::Popup { activate: false },
            "Window previews",
            Role::Toolbar,
        )?;
        let (list, size) = view_grid(&item, tokens, scale, ui, grid, start);
        let screen = mypaper_ui::screen_at(anchor.left as i32, anchor.top as i32);
        let x = if vertical {
            if anchor.left < (screen.work.0 + screen.work.2) as f32 / 2.0 {
                anchor.right as i32 + 8
            } else {
                anchor.left as i32 - size.0 as i32 - 8
            }
        } else {
            ((anchor.left + anchor.right - size.0 as f32) / 2.0).round() as i32
        };
        let y = if vertical {
            ((anchor.top + anchor.bottom - size.1 as f32) / 2.0) as i32
        } else if anchor.top > (screen.work.1 + screen.work.3) as f32 / 2.0 {
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
        surface.set_glass(glass(tokens, scale, size));
        surface.present(ui, origin, size, list)?;
        let mut thumbnails = Previews::default();
        let hwnds: Vec<_> = item
            .windows
            .iter()
            .skip(start)
            .take(grid.capacity)
            .map(|w| w.hwnd)
            .collect();
        let sizes = thumbnails.open(surface.hwnd(), &hwnds);
        let placements: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let factor = ((grid.width - 16.0 * scale).max(1.0) / s.width as f32)
                    .min((grid.height - 44.0 * scale).max(1.0) / s.height as f32);
                (
                    s.hwnd,
                    [
                        ((i % grid.cols) as f32 * grid.width + 8.0 * scale) as f64,
                        ((i / grid.cols) as f32 * grid.height + 32.0 * scale) as f64,
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
            start,
            capacity: grid.capacity,
            tokens: tokens.clone(),
            scale,
            anchor,
            vertical,
        })
    }
    /// Long window groups page by wheel; each page stays inside its monitor.
    pub fn scroll(&mut self, ui: &mut Ui, delta: i16) {
        let start = if delta > 0 {
            self.start.saturating_sub(self.capacity)
        } else {
            (self.start + self.capacity)
                .min(self.item.windows.len().saturating_sub(1) / self.capacity * self.capacity)
        };
        if start == self.start {
            return;
        }
        self.surface.hide();
        if let Ok(next) = Self::open_at(
            ui,
            self.item.clone(),
            self.anchor,
            &self.tokens,
            self.scale,
            start,
            self.vertical,
        ) {
            *self = next;
        }
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
    fn many_window_groups_fit_and_page_on_narrow_and_vertical_monitors() {
        for scale in [1.0, 1.5, 2.0] {
            for vertical in [false, true] {
                let grid = Grid::fit(75, (640.0, 480.0), scale, vertical);
                assert!(grid.width * grid.cols as f32 <= 640.0);
                assert!(grid.height * grid.rows as f32 <= 480.0);
                assert!(grid.capacity > 0 && grid.capacity < 75);
                if vertical {
                    assert_eq!(grid.cols, 1);
                }
            }
        }
    }
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
