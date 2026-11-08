//! The dock's window previews: one popup, reused while the pointer moves between apps, with a
//! cell per window at the window's own aspect ratio. A cell is a title bar (the app's icon, the
//! window's title, a close button on hover) over DWM's live thumbnail, flush to its sides.
use super::look::Tokens;
use anyhow::Result;
use glaze_bars::{
    apps::Item,
    preview::{Previews, SourceSize},
};
use glaze_ui::{
    Align, DisplayList, Frame, Image, Interaction, Kind, Op, Point, Rect, Rgba, Role, Surface,
    SurfaceId, TextRole, Ui, icons::Icon,
};
use std::sync::Arc;

pub struct Preview {
    pub surface: Surface,
    /// Click-through, over `surface`: the thumbnails' rounding masks ([`masks`]).
    corners: Surface,
    pub key: String,
    pub item: Item,
    thumbnails: Previews,
    /// The registered thumbnails' source sizes, from window `start` on.
    sizes: Vec<SourceSize>,
    cells: Vec<Cell>,
    start: usize,
    size: (u32, u32),
    origin: (i32, i32),
    tokens: Tokens,
    scale: f32,
    anchor: Rect,
    vertical: bool,
    icon: Option<Arc<Image>>,
    hover: Option<u32>,
}

/// One window's cell in popup px: the whole rounded cell, and under its title bar the
/// thumbnail's exact box (flush with the cell's sides and bottom, or centred across when the
/// title bar needs the cell wider than the thumbnail).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cell {
    rect: Rect,
    thumb: Rect,
}

const TITLE: f32 = 30.0;

/// Cells for windows of `aspects` (width / height): each thumbnail at a common height (a vertical
/// dock: a common width), capped for very wide (tall) windows, and each cell exactly as wide as
/// its thumbnail unless the title bar needs more room. Rows fit `room` with even padding; returns
/// the cells that fit (at least one) and the popup's size.
fn layout(
    aspects: &[f32],
    room: (f32, f32),
    scale: f32,
    vertical: bool,
) -> (Vec<Cell>, (u32, u32)) {
    let px = |v: f32| (v * scale).round();
    let (pad, gap, title) = (px(8.0), px(8.0), px(TITLE));
    let (mut x, mut y, mut row, mut right) = (pad, pad, 0.0_f32, pad);
    let mut cells = Vec::new();
    for &aspect in aspects {
        let aspect = aspect.clamp(0.1, 10.0);
        // The thumbnail's size, then its cell's width.
        let ((tw, th), w) = if vertical {
            let (w, max) = (px(220.0), px(180.0));
            let h = (w / aspect).min(max);
            (((h * aspect).min(w).round(), h.round()), w)
        } else {
            let (h, max, min) = (px(120.0), px(300.0), px(150.0));
            let w = (h * aspect).min(max);
            let thumb = (w.round(), (w / aspect).round());
            (thumb, thumb.0.max(min))
        };
        if !cells.is_empty() && (vertical || x + w + pad > room.0) {
            y += row + gap;
            x = pad;
            row = 0.0;
        }
        if !cells.is_empty() && y + title + th + pad > room.1 {
            break;
        }
        cells.push(Cell {
            rect: Rect::new(x, y, w, title + th),
            thumb: Rect::new(x + ((w - tw) / 2.0).round(), y + title, tw, th),
        });
        right = right.max(x + w);
        row = row.max(title + th);
        x += w + gap;
    }
    let size = ((right + pad).ceil() as u32, (y + row + pad).ceil() as u32);
    (cells, size)
}

/// A cell's corner radius: concentric with the plate.
fn corner(tokens: &Tokens, scale: f32) -> f32 {
    (tokens.radius * scale - (8.0 * scale).round()).max((6.0 * scale).round())
}

/// What lies under a thumbnail's bottom corners, opaque: the popup's body (on glass, its tint
/// over the surface color), under the cell's fill when the thumbnail is narrower than its cell.
fn under(tokens: &Tokens, cell: &Cell) -> Rgba {
    use glaze_ui::theme::over;
    let c = tokens.theme.color;
    let black = Rgba::new(0.0, 0.0, 0.0, 1.0);
    let body = match tokens.material.filter(|_| glaze_ui::glass::available()) {
        Some(m) => over(m.tint, over(c.surface, black)),
        None => over(c.surface, black),
    };
    if cell.thumb.width() < cell.rect.width() {
        over(c.fill, body)
    } else {
        body
    }
}

/// The masks that round DWM's square thumbnails: they composite above the popup's own pixels, so
/// these draw on a click-through surface stacked over the popup. Per bottom corner, a ring whose
/// inner edge is the thumbnail's rounded outline (wide enough to reach the square corner),
/// clipped to the corner's square.
fn masks(cells: &[Cell], tokens: &Tokens, scale: f32) -> DisplayList {
    let r = corner(tokens, scale);
    let mut list = DisplayList::default();
    for cell in cells {
        let t = cell.thumb;
        let ring: Arc<[Op]> = Arc::from(vec![Op::Stroke {
            rect: t.inflate(r / 2.0),
            radius: r * 1.5,
            width: r,
            color: under(tokens, cell),
        }]);
        for left in [t.left, t.right - r] {
            list.ops.push(Op::Clipped {
                rect: Rect::new(left, t.bottom - r, r, r),
                ops: ring.clone(),
            });
        }
    }
    list
}

fn glass(tokens: &Tokens, scale: f32, size: (u32, u32)) -> Option<glaze_ui::Glass> {
    tokens
        .material
        .filter(|_| glaze_ui::glass::available())
        .map(|m| glaze_ui::Glass {
            body: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
            radius: (tokens.radius * scale).round(),
            material: glaze_ui::GlassMaterial {
                clarity: m.clarity,
                blur: m.blur * scale,
                refraction: m.refraction,
                highlight: m.highlight,
                tint: m.tint,
            },
        })
}

/// The popup's display list: the plate, then each cell (window `start + k` for cell `k`). Window
/// `i`'s cell is hit `2i + 1`, its close button `2i + 2`, drawn only while the cell is hovered.
#[allow(clippy::too_many_arguments)]
fn view(
    item: &Item,
    cells: &[Cell],
    start: usize,
    size: (u32, u32),
    tokens: &Tokens,
    scale: f32,
    ui: &mut Ui,
    icon: Option<&Arc<Image>>,
    hover: Option<u32>,
) -> DisplayList {
    let input = Interaction {
        hover,
        ..Default::default()
    };
    let mut f = Frame::new(&mut ui.text, &tokens.theme, scale, &input);
    let body = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let radius = f.px(tokens.radius);
    f.fill(body, radius, Rgba::HIT);
    let has_glass = glass(tokens, scale, size).is_some();
    f.surface_shaped(body, radius, has_glass);
    if has_glass && let Some(color) = tokens.theme.border {
        f.push(Op::Stroke {
            rect: body.inflate(-0.5),
            radius: (radius - 0.5).max(0.0),
            width: 1.0,
            color,
        });
    }
    let c = tokens.theme.color;
    let corner = corner(tokens, scale);
    for (k, cell) in cells.iter().enumerate() {
        let i = start + k;
        let Some(window) = item.windows.get(i) else {
            break;
        };
        let id = i as u32 * 2 + 1;
        let hovered = hover == Some(id) || hover == Some(id + 1);
        // One rounded fill: the title bar is its top; the thumbnail covers the rest.
        f.fill(cell.rect, corner, c.fill);
        if hovered {
            f.fill(cell.rect, corner, c.hover);
        }
        // DWM preserves the source's alpha (Files glass, Mica/acrylic apps).
        // Its 255 opacity is only a multiplier, so give every thumbnail an opaque
        // theme surface before DWM composites it; the overlay still rounds its corners.
        f.fill(cell.thumb, 0.0, c.surface.with_alpha(1.0));
        let (bar, _) = cell.rect.take_top(f.px(TITLE), 0.0);
        let (close, rest) = bar.take_right(bar.height(), 0.0);
        let glyph = f.px(16.0);
        let mut text = rest;
        if let Some(icon) = icon {
            let slot = Rect::new(
                rest.left + f.px(8.0),
                (rest.top + (rest.height() - glyph) / 2.0).round(),
                glyph,
                glyph,
            );
            f.push(Op::Image {
                rect: slot,
                image: icon.clone(),
                opacity: 1.0,
                source: None,
            });
            text = Rect::new(slot.right, rest.top, rest.right - slot.right, rest.height());
        }
        f.hit(id, cell.rect, Role::Button, &window.title, true, None);
        let ink = if hovered || window.active {
            c.label
        } else {
            c.secondary
        };
        f.text_in(
            Rect::new(
                text.left + f.px(8.0),
                text.top,
                (text.width() - f.px(8.0)).max(0.0),
                text.height(),
            ),
            &window.title,
            TextRole::Subheadline,
            ink,
            Align::Start,
        );
        if hovered {
            f.icon_button(
                id + 1,
                close,
                Icon::Close,
                20.0,
                &format!("Close {}", window.title),
                false,
            );
        }
    }
    glaze_ui::geom::continuous::surface(&mut f.list, body, tokens.glass_style.smoothing);
    f.list
}

impl Preview {
    /// Opens the previews of `item`'s windows above `anchor` (its dock icon, in screen px).
    pub fn open(
        ui: &mut Ui,
        item: Item,
        anchor: Rect,
        tokens: &Tokens,
        scale: f32,
        vertical: bool,
        icon: Option<Arc<Image>>,
    ) -> Result<Self> {
        let surface = Surface::new(
            Kind::Popup { activate: false },
            "Window previews",
            Role::Toolbar,
        )?;
        let corners = Surface::new(
            Kind::Popup { activate: false },
            "Window preview corners",
            Role::Group,
        )?;
        corners.set_click_through(true);
        let mut preview = Self {
            surface,
            corners,
            key: item.key.clone(),
            item,
            thumbnails: Previews::default(),
            sizes: vec![],
            cells: vec![],
            start: 0,
            size: (1, 1),
            origin: (0, 0),
            tokens: tokens.clone(),
            scale,
            anchor,
            vertical,
            icon,
            hover: None,
        };
        preview.lay_out(ui)?;
        preview.surface.show();
        // Shown after the popup, so stacked over it and its thumbnails.
        preview.corners.show();
        Ok(preview)
    }

    /// Shows another app's windows in the same popup, at once (no delay, no new window).
    pub fn switch(
        &mut self,
        ui: &mut Ui,
        item: Item,
        anchor: Rect,
        icon: Option<Arc<Image>>,
    ) -> Result<()> {
        self.key = item.key.clone();
        self.item = item;
        self.anchor = anchor;
        self.icon = icon;
        self.start = 0;
        self.hover = None;
        self.lay_out(ui)
    }

    /// Registers the thumbnails from window `start` on, lays their cells out at their windows'
    /// aspect ratios, presents the popup over the anchor and places the thumbnails.
    fn lay_out(&mut self, ui: &mut Ui) -> Result<()> {
        let hwnds: Vec<_> = self
            .item
            .windows
            .iter()
            .skip(self.start)
            .map(|w| w.hwnd)
            .collect();
        self.sizes = self.thumbnails.open(self.surface.hwnd(), &hwnds);
        let screen = glaze_ui::screen_at(self.anchor.left as i32, self.anchor.top as i32);
        let room = (
            (screen.work.2 - screen.work.0) as f32 - 16.0,
            (screen.work.3 - screen.work.1) as f32 / 2.0,
        );
        let aspects: Vec<_> = self
            .sizes
            .iter()
            .map(|s| s.width as f32 / s.height as f32)
            .collect();
        (self.cells, self.size) = layout(&aspects, room, self.scale, self.vertical);
        let (anchor, size) = (self.anchor, self.size);
        let x = if self.vertical {
            if anchor.left < (screen.work.0 + screen.work.2) as f32 / 2.0 {
                anchor.right as i32 + 8
            } else {
                anchor.left as i32 - size.0 as i32 - 8
            }
        } else {
            ((anchor.left + anchor.right - size.0 as f32) / 2.0).round() as i32
        };
        let y = if self.vertical {
            ((anchor.top + anchor.bottom - size.1 as f32) / 2.0) as i32
        } else if anchor.top > (screen.work.1 + screen.work.3) as f32 / 2.0 {
            anchor.top as i32 - size.1 as i32 - 8
        } else {
            anchor.bottom as i32 + 8
        };
        // Wholly on the anchor's monitor, inside its work area.
        let (work, margin) = (screen.work, 8);
        self.origin = (
            x.clamp(
                work.0 + margin,
                (work.2 - margin - size.0 as i32).max(work.0),
            ),
            y.clamp(
                work.1 + margin,
                (work.3 - margin - size.1 as i32).max(work.1),
            ),
        );
        self.surface.set_glass_style(self.tokens.glass_style);
        self.surface
            .set_glass(glass(&self.tokens, self.scale, size));
        self.present(ui)?;
        let placements: Vec<_> = self
            .sizes
            .iter()
            .zip(&self.cells)
            .map(|(s, cell)| {
                let r = cell.thumb;
                (
                    s.hwnd,
                    [r.left, r.top, r.width(), r.height()].map(f64::from),
                )
            })
            .collect();
        self.thumbnails.place(&placements, 1.0);
        let corners = masks(&self.cells, &self.tokens, self.scale);
        self.corners.present(ui, self.origin, size, corners)?;
        Ok(())
    }

    fn present(&mut self, ui: &mut Ui) -> Result<()> {
        let list = view(
            &self.item,
            &self.cells,
            self.start,
            self.size,
            &self.tokens,
            self.scale,
            ui,
            self.icon.as_ref(),
            self.hover,
        );
        self.surface.present(ui, self.origin, self.size, list)?;
        Ok(())
    }

    /// The pointer moved over the popup: redraws the hovered cell (and its close button).
    pub fn hover(&mut self, ui: &mut Ui, p: Option<Point>) {
        let hover = p.and_then(|p| self.surface.shown().hit_at(p)).map(|h| h.id);
        if hover != self.hover {
            self.hover = hover;
            let _ = self.present(ui);
        }
    }

    /// Long window groups page by wheel; each page stays inside its monitor.
    pub fn scroll(&mut self, ui: &mut Ui, delta: i16) {
        let page = self.cells.len().max(1);
        let start = if delta > 0 {
            self.start.saturating_sub(page)
        } else {
            (self.start + page).min(self.item.windows.len().saturating_sub(1))
        };
        if start != self.start {
            self.start = start;
            self.hover = None;
            let _ = self.lay_out(ui);
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

    /// Opt-in live fixture: only our Files HWND is read, and both popups are nonactivating.
    /// Run under the desktop lock with GLAZE_PREVIEW_CAPTURE set to an output directory.
    #[test]
    #[ignore = "requires the desktop lock and a live glaze-files window"]
    fn capture_live_files_preview_without_input() {
        use windows::Win32::UI::{HiDpi::*, WindowsAndMessaging::*};
        struct Backdrop;
        impl glaze_ui::glass::Backdrop for Backdrop {
            fn available(&self) -> bool {
                true
            }
            fn open(
                &self,
                _: windows::Win32::Foundation::HWND,
                _: glaze_ui::glass::Shape,
                _: f32,
                _: &glaze_ui::GlassMaterial,
            ) -> bool {
                false
            }
            fn place(
                &self,
                _: windows::Win32::Foundation::HWND,
                _: glaze_ui::glass::Shape,
                _: f32,
            ) {
            }
            fn material(&self, _: windows::Win32::Foundation::HWND, _: &glaze_ui::GlassMaterial) {
            }
            fn close(&self, _: windows::Win32::Foundation::HWND) {}
        }
        let output = std::path::PathBuf::from(std::env::var_os("GLAZE_PREVIEW_CAPTURE").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        // SAFETY: DPI context is established before the fixture creates any HWND.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let mut apps = glaze_bars::apps::Apps::default();
        apps.refresh();
        let group = apps
            .groups
            .into_iter()
            .find(|g| {
                g.exe
                    .as_ref()
                    .is_some_and(|e| e.ends_with("\\glaze-files.exe"))
            })
            .expect("live Files window");
        let window = &group.windows[0];
        let item = Item {
            key: group.key.clone(),
            name: "Files".into(),
            path: group.exe.clone().unwrap(),
            icon: None,
            pinned: false,
            aumid: None,
            exe: group.exe.clone(),
            windows: vec![glaze_bars::apps::ItemWindow {
                hwnd: window.hwnd,
                title: window.title.clone(),
                active: false,
                minimized: window.minimized,
                attention: false,
            }],
        };
        let mut ui = Ui::new().unwrap();
        glaze_ui::glass::install(std::rc::Rc::new(Backdrop));
        let config = glaze_bars::config::path()
            .and_then(|p| glaze_bars::BarsConfig::load(&p))
            .unwrap_or_default();
        let tokens = Tokens::resolve(
            &config.dock.look,
            &config.appearance,
            &glaze_bars::accent::read(),
        );
        // Resolve through the dock's real worker and disk cache, rather than bypassing
        // executable lookup with a supplied bitmap.
        let (tx, rx) = std::sync::mpsc::channel();
        let mut icons = glaze_bars::icons::Icons::new(
            glaze_bars::wake::Waker::new(tx, 0),
            glaze_bars::files::local("icons").unwrap(),
        );
        icons.get(&glaze_bars::icons::Source::App {
            aumid: group.aumid.clone(),
            exe: group.exe.clone(),
        });
        let glaze_bars::wake::Msg::Described(_, described) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        else {
            panic!("icon worker did not describe Files");
        };
        let icon = described
            .icon
            .as_ref()
            .and_then(|p| Image::load(&ui, std::path::Path::new(p)).ok())
            .map(Arc::new);
        if let Some(icon) = &icon {
            glaze_ui::golden::write(icon, &output.join("dock-icon.png")).unwrap();
        }
        let preview = Preview::open(
            &mut ui,
            item,
            Rect::new(400.0, 600.0, 64.0, 64.0),
            &tokens,
            1.0,
            false,
            icon,
        )
        .unwrap();
        let image = glaze_ui::render(
            &mut ui,
            preview.surface.shown(),
            preview.size.0,
            preview.size.1,
        )
        .unwrap();
        glaze_ui::golden::write(&image, &output.join("backing.png")).unwrap();
        let thumb = preview.cells[0].thumb;
        std::fs::write(
            output.join("preview.json"),
            serde_json::json!({
                "hwnd": preview.surface.hwnd().0 as usize,
                "corners": preview.corners.hwnd().0 as usize,
                "source": window.hwnd,
                "icon_cache": described.icon,
                "origin": preview.origin, "size": preview.size,
                "thumb": [thumb.left, thumb.top, thumb.width(), thumb.height()],
                "center_alpha": image.pixel((thumb.left + thumb.width()/2.0) as u32,
                    (thumb.top + thumb.height()/2.0) as u32)[3],
            })
            .to_string(),
        )
        .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < until {
            let mut msg = MSG::default();
            // SAFETY: only this fixture thread's message queue is dispatched; no synthetic input.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn cells_keep_their_windows_aspect_between_the_width_limits_and_fit_the_room() {
        for scale in [1.0, 1.5, 2.0] {
            let (cells, size) = layout(
                &[16.0 / 9.0, 0.5, 4.0],
                (2000.0 * scale, 800.0),
                scale,
                false,
            );
            let [wide, tall, ultra] = [cells[0], cells[1], cells[2]].map(|c| c.thumb);
            assert!((wide.width() / wide.height() - 16.0 / 9.0).abs() < 0.02);
            // A common thumbnail height; the tall one's cell is only as wide as its title bar.
            assert_eq!(tall.height(), wide.height());
            assert!((tall.width() / tall.height() - 0.5).abs() < 0.02);
            assert_eq!(cells[1].rect.width(), (150.0 * scale).round());
            assert_eq!(cells[0].rect.width(), wide.width());
            assert!(ultra.width() <= 300.0 * scale + 1.0);
            // Thumbnails sit flush at their cells' bottoms; every cell is inside the popup with
            // the same padding.
            let pad = (8.0 * scale).round();
            for c in &cells {
                assert_eq!(c.thumb.bottom, c.rect.bottom);
                assert!(c.rect.left >= pad && c.rect.top >= pad);
                assert!(
                    c.rect.right <= size.0 as f32 - pad && c.rect.bottom <= size.1 as f32 - pad
                );
            }
            for vertical in [false, true] {
                let (cells, size) = layout(&[1.6; 75], (640.0, 480.0), scale, vertical);
                assert!(!cells.is_empty() && cells.len() < 75);
                assert!(size.0 as f32 <= 640.0 && size.1 as f32 <= 480.0);
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
            windows: vec![glaze_bars::apps::ItemWindow {
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
            &glaze_bars::accent::read(),
        );
        let (cells, size) = layout(&[1.6], (2000.0, 800.0), 1.0, false);
        // Hovered, so its close button is drawn too.
        let list = view(&app, &cells, 0, size, &tokens, 1.0, &mut ui, None, Some(1));
        let image = glaze_ui::render(&mut ui, &list, size.0, size.1).unwrap();
        assert_eq!(list.hits.len(), 2);
        for hit in &list.hits {
            let p = Point::new(
                (hit.rect.left + hit.rect.right) / 2.0,
                (hit.rect.top + hit.rect.bottom) / 2.0,
            );
            assert_eq!(list.hit_at(p).map(|h| h.id), Some(hit.id));
            assert!(image.pixel(p.x as u32, p.y as u32)[3] > 0);
        }
    }

    #[test]
    fn translucent_sources_have_an_opaque_thumbnail_backing() {
        let mut ui = Ui::new().unwrap();
        let app = Item {
            key: "app".into(),
            name: "App".into(),
            path: "app.exe".into(),
            windows: vec![glaze_bars::apps::ItemWindow {
                hwnd: 1,
                title: "Translucent window".into(),
                active: false,
                minimized: false,
                attention: false,
            }],
            icon: None,
            pinned: false,
            aumid: None,
            exe: None,
        };
        let mut tokens = Tokens::resolve(
            &Default::default(),
            &Default::default(),
            &glaze_bars::accent::read(),
        );
        // A theme can make both the popup and cell translucent. DWM preserves the
        // source window's alpha, so the destination must supply its own backing.
        tokens.theme.color.surface = Rgba::new(0.2, 0.3, 0.4, 0.15);
        tokens.theme.color.fill = Rgba::new(0.8, 0.8, 0.8, 0.05);
        for aspect in [1.6, 0.5, 4.0] {
            let (cells, size) = layout(&[aspect], (2000.0, 800.0), 1.0, false);
            for hover in [None, Some(1)] {
                let list = view(&app, &cells, 0, size, &tokens, 1.0, &mut ui, None, hover);
                let image = glaze_ui::render(&mut ui, &list, size.0, size.1).unwrap();
                let t = cells[0].thumb;
                for fraction in [0.1, 0.5, 0.9] {
                    assert_eq!(
                        image.pixel(
                            (t.left + t.width() * fraction) as u32,
                            (t.top + t.height() * fraction) as u32,
                        )[3],
                        255
                    );
                }
            }
        }
    }
}
