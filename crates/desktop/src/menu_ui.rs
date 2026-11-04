//! The desktop menu's window: custom drawn (Direct2D and DirectWrite into a
//! 32-bit bitmap shown with `UpdateLayeredWindow`), so it appears within a few
//! milliseconds and needs no browser. One topmost popup per panel: the root
//! menu, and at most one submenu beside it. What to draw and where is decided
//! by [`menu_model`](crate::menu_model); this file draws it and routes input.
//!
//! The root panel is shown without activating, then activated so keys work and
//! clicking elsewhere closes it (`WM_ACTIVATE`). Submenus never activate, so
//! clicking them does not deactivate the root.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SIZE,
    WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, HICON,
    IDC_ARROW, LoadCursorW, RegisterClassExW, SW_SHOWNOACTIVATE, SetForegroundWindow,
    SetWindowLongPtrW, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WA_INACTIVE, WM_ACTIVATE,
    WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY, WM_RBUTTONUP,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    MA_NOACTIVATE,
};
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;

use crate::menu_model::{self, Command, Item, Layout, Metrics};

const CLASS: PCWSTR = w!("mypaper.desktopmenu");
/// Transparent room around a panel for its shadow, in device pixels at 100%.
const SHADOW: f32 = 14.0;

/// How the menu looks: the appearance variables of the theme (`bars` skin),
/// resolved by the host. `blur` has no counterpart here: the panel is drawn
/// as a nearly opaque tint instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// Light text on a dark tint (`text: "light"`).
    pub dark: bool,
    pub tint: [u8; 3],
    pub opacity: f32,
    pub radius: f32,
    pub border: bool,
    pub accent: [u8; 3],
    /// A CSS font-family list; the first family that is not generic is used.
    pub font: String,
    pub scale: f32,
    pub shadow: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            dark: true,
            tint: [0x16, 0x18, 0x1d],
            opacity: 0.94,
            radius: 10.0,
            border: true,
            accent: [0xff, 0xff, 0xff],
            font: "Segoe UI Variable Text, Segoe UI".into(),
            scale: 1.0,
            shadow: true,
        }
    }
}

/// The first usable family of a CSS `font-family` list.
pub fn font_family(css: &str) -> String {
    const GENERIC: [&str; 6] = ["system-ui", "sans-serif", "serif", "monospace", "ui-sans-serif", "cursive"];
    css.split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).trim())
        .find(|f| !f.is_empty() && !GENERIC.iter().any(|g| g.eq_ignore_ascii_case(f)))
        .unwrap_or("Segoe UI")
        .to_owned()
}

/// Direct2D and DirectWrite objects, made once.
struct Gfx {
    dwrite: IDWriteFactory,
    target: ID2D1DCRenderTarget,
}

impl Gfx {
    fn new() -> Result<Self> {
        // SAFETY: documented factory calls; the objects are owned by this struct.
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .context("Direct2D")?;
            let dwrite: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).context("DirectWrite")?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = d2d.CreateDCRenderTarget(&properties).context("Direct2D DC target")?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self { dwrite, target })
        }
    }

    fn format(&self, family: &str, size: f32) -> Result<IDWriteTextFormat> {
        // SAFETY: documented calls; the strings outlive them.
        unsafe {
            let family = windows::core::HSTRING::from(family);
            let format = self.dwrite.CreateTextFormat(
                &family,
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(format)
        }
    }

    fn text(&self, text: &str, format: &IDWriteTextFormat) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: documented call; the slice is copied by DirectWrite.
        unsafe { self.dwrite.CreateTextLayout(&wide, format, 4096.0, 4096.0) }.ok()
    }
}

fn width_of(layout: &IDWriteTextLayout) -> f32 {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: the out pointer is a valid local.
    let _ = unsafe { layout.GetMetrics(&mut metrics) };
    metrics.widthIncludingTrailingWhitespace
}

struct Panel {
    hwnd: HWND,
    items: Vec<Item>,
    labels: Vec<Option<IDWriteTextLayout>>,
    layout: Layout,
    /// Top-left of the panel itself (not its shadow margin), screen pixels.
    origin: (i32, i32),
    hover: Option<usize>,
    /// The row of the parent panel that opened this one.
    opened_by: Option<usize>,
}

impl Panel {
    fn rect(&self) -> menu_model::Edges {
        (
            self.origin.0,
            self.origin.1,
            self.origin.0 + self.layout.width.ceil() as i32,
            self.origin.1 + self.layout.height.ceil() as i32,
        )
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        let (l, t, r, b) = self.rect();
        x >= l && y >= t && x < r && y < b
    }
}

/// Called (outside any borrow of the UI) when an item is chosen.
pub type OnCommand = Rc<dyn Fn(Command)>;

pub struct MenuUi {
    gfx: Gfx,
    look: Look,
    /// Device pixels per design pixel: monitor DPI times the look's scale.
    scale: f32,
    panels: Vec<Panel>,
    on_command: OnCommand,
}

thread_local! {
    static UI: RefCell<Option<MenuUi>> = const { RefCell::new(None) };
}

/// Runs `f` on the UI if it is not already borrowed (window messages that
/// arrive while we are inside `show` are ignored).
fn with_ui<R>(f: impl FnOnce(&mut MenuUi) -> R) -> Option<R> {
    UI.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().map(f)
    })
}

/// Creates the menu UI of this thread. `on_command` runs when an item is chosen.
pub fn install(on_command: OnCommand) -> Result<()> {
    register_class()?;
    let ui = MenuUi {
        gfx: Gfx::new()?,
        look: Look::default(),
        scale: 1.0,
        panels: Vec::new(),
        on_command,
    };
    UI.with(|cell| *cell.borrow_mut() = Some(ui));
    Ok(())
}

/// Drops the menu UI of this thread (closing it).
pub fn uninstall() {
    close();
    UI.with(|cell| *cell.borrow_mut() = None);
}

/// Opens the menu at `anchor` (screen pixels), on the monitor with `work` area
/// and `dpi`. A menu already open is replaced.
pub fn show(items: Vec<Item>, anchor: (i32, i32), work: menu_model::Edges, dpi: u32, look: &Look) {
    let shown = with_ui(|ui| ui.show(items, anchor, work, dpi, look));
    if let Some(Err(e)) = shown {
        log::warn!("desktop menu: {e:#}");
        close();
    }
}

/// Closes every panel.
pub fn close() {
    let hwnds: Vec<HWND> = with_ui(|ui| ui.panels.drain(..).map(|p| p.hwnd).collect()).unwrap_or_default();
    for hwnd in hwnds {
        // SAFETY: our own windows, destroyed on their creating thread.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

pub fn is_open() -> bool {
    with_ui(|ui| !ui.panels.is_empty()).unwrap_or(false)
}

/// Is the screen point on one of the panels? (For closing on an outside click.)
pub fn contains(x: i32, y: i32) -> bool {
    with_ui(|ui| ui.panels.iter().any(|p| p.contains(x, y))).unwrap_or(false)
}

fn register_class() -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: HICON::default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

impl MenuUi {
    fn show(
        &mut self,
        items: Vec<Item>,
        anchor: (i32, i32),
        work: menu_model::Edges,
        dpi: u32,
        look: &Look,
    ) -> Result<()> {
        self.close_panels();
        self.look = look.clone();
        self.scale = dpi.max(96) as f32 / 96.0 * look.scale.clamp(0.5, 2.0);
        let (origin, panel) = self.make_panel(items, None, |size| menu_model::place(size, anchor, work))?;
        let hwnd = panel.hwnd;
        self.panels.push(Panel { origin, ..panel });
        self.draw(0)?;
        // Visible at once without taking focus; then focus, so keys and click-away work.
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    fn close_panels(&mut self) {
        for panel in self.panels.drain(..) {
            // SAFETY: our own window, on its creating thread.
            let _ = unsafe { DestroyWindow(panel.hwnd) };
        }
    }

    /// Builds a panel (window created, not shown): measures the labels, lets
    /// `place` choose the origin from the panel's pixel size.
    fn make_panel(
        &self,
        items: Vec<Item>,
        opened_by: Option<usize>,
        place: impl FnOnce((i32, i32)) -> (i32, i32),
    ) -> Result<((i32, i32), Panel)> {
        let metrics = Metrics::scaled(self.scale);
        let format = self.gfx.format(&font_family(&self.look.font), 14.0 * self.scale)?;
        let labels: Vec<Option<IDWriteTextLayout>> = items
            .iter()
            .map(|item| match item {
                Item::Separator => None,
                other => self.gfx.text(other.label(), &format),
            })
            .collect();
        let widths: Vec<f32> = labels.iter().map(|l| l.as_ref().map_or(0.0, width_of)).collect();
        let layout = menu_model::layout(&items, &metrics, |text| {
            items
                .iter()
                .zip(&widths)
                .find(|(item, _)| item.label() == text)
                .map_or(0.0, |(_, w)| *w)
        });
        let size = (layout.width.ceil() as i32, layout.height.ceil() as i32);
        let origin = place(size);
        let margin = (SHADOW * self.scale).ceil() as i32;
        let extra = if self.look.shadow { margin } else { 0 };
        let mut ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        if opened_by.is_some() {
            ex |= WS_EX_NOACTIVATE;
        }
        // SAFETY: class registered by `install`; a popup without a parent.
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                CLASS,
                w!("mypaper menu"),
                WS_POPUP,
                origin.0 - extra,
                origin.1 - extra,
                size.0 + 2 * extra,
                size.1 + 2 * extra,
                None,
                None,
                None,
                None,
            )
        }
        .context("creating the menu window")?;
        Ok((
            origin,
            Panel {
                hwnd,
                items,
                labels,
                layout,
                origin,
                hover: None,
                opened_by,
            },
        ))
    }

    fn colors(&self) -> Colors {
        let [tr, tg, tb] = self.look.tint;
        let (text, edge) = if self.look.dark {
            ([0.94, 0.94, 0.96], [1.0, 1.0, 1.0])
        } else {
            ([0.09, 0.09, 0.11], [0.0, 0.0, 0.0])
        };
        let [ar, ag, ab] = self.look.accent;
        Colors {
            panel: rgba(tr, tg, tb, self.look.opacity.clamp(0.7, 1.0)),
            text: D2D1_COLOR_F { r: text[0], g: text[1], b: text[2], a: 1.0 },
            dim: D2D1_COLOR_F { r: text[0], g: text[1], b: text[2], a: 0.38 },
            hover: rgba(ar, ag, ab, if self.look.dark { 0.20 } else { 0.16 }),
            line: D2D1_COLOR_F { r: edge[0], g: edge[1], b: edge[2], a: 0.14 },
        }
    }

    /// Redraws panel `index` into its layered window.
    fn draw(&self, index: usize) -> Result<()> {
        let panel = &self.panels[index];
        let colors = self.colors();
        let margin = if self.look.shadow { (SHADOW * self.scale).ceil() as i32 } else { 0 };
        let (w, h) = (
            panel.layout.width.ceil() as i32 + 2 * margin,
            panel.layout.height.ceil() as i32 + 2 * margin,
        );
        let radius = (self.look.radius * self.scale).max(0.0);
        let rt = &self.gfx.target;
        // SAFETY: GDI and Direct2D calls on objects created and destroyed here, on the owning
        // thread; `bits` is the DIB section's pixel memory, which Direct2D writes through the DC.
        unsafe {
            let hdc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let previous = SelectObject(hdc, dib.into());
            let drawn = (|| -> Result<()> {
                rt.BindDC(hdc, &RECT { left: 0, top: 0, right: w, bottom: h })?;
                rt.BeginDraw();
                rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
                let brush = rt.CreateSolidColorBrush(&colors.panel, None)?;
                let m = margin as f32;
                let body = D2D_RECT_F {
                    left: m,
                    top: m,
                    right: m + panel.layout.width,
                    bottom: m + panel.layout.height,
                };
                if self.look.shadow {
                    for i in 0..margin / 2 {
                        let grow = (i * 2) as f32;
                        let alpha = 0.05 * (1.0 - i as f32 / (margin / 2) as f32);
                        brush.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: alpha });
                        fill_round(rt, &brush, grow_rect(body, grow, 2.0 * self.scale), radius + grow);
                    }
                }
                brush.SetColor(&colors.panel);
                fill_round(rt, &brush, body, radius);
                if self.look.border {
                    brush.SetColor(&colors.line);
                    let inset = grow_rect(body, -0.5, 0.0);
                    let rounded = D2D1_ROUNDED_RECT { rect: inset, radiusX: radius, radiusY: radius };
                    rt.DrawRoundedRectangle(&rounded, &brush, 1.0, None);
                }
                self.draw_rows(panel, &brush, &colors, m, radius)?;
                rt.EndDraw(None, None)?;
                Ok(())
            })();
            let shown = drawn.and_then(|()| {
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                UpdateLayeredWindow(
                    panel.hwnd,
                    None,
                    Some(&POINT { x: panel.origin.0 - margin, y: panel.origin.1 - margin }),
                    Some(&SIZE { cx: w, cy: h }),
                    Some(hdc),
                    Some(&POINT { x: 0, y: 0 }),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )?;
                Ok(())
            });
            SelectObject(hdc, previous);
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(hdc);
            shown
        }
    }

    /// The rows of one panel: hover highlight, check marks, labels, arrows, separators.
    ///
    /// # Safety
    /// Must run between `BeginDraw` and `EndDraw` of the shared target.
    unsafe fn draw_rows(
        &self,
        panel: &Panel,
        brush: &ID2D1SolidColorBrush,
        colors: &Colors,
        margin: f32,
        radius: f32,
    ) -> Result<()> {
        let rt = &self.gfx.target;
        let metrics = Metrics::scaled(self.scale);
        let inset = 4.0 * self.scale;
        // SAFETY: per this function's contract; all objects are alive for the calls.
        unsafe {
            for (index, (item, row)) in panel.items.iter().zip(&panel.layout.rows).enumerate() {
                let top = margin + row.top;
                let selected = panel.hover == Some(index) && item.selectable();
                if let Item::Separator = item {
                    brush.SetColor(&colors.line);
                    let y = top + row.height / 2.0;
                    let line = D2D_RECT_F {
                        left: margin + metrics.pad + inset,
                        top: y,
                        right: margin + panel.layout.width - metrics.pad - inset,
                        bottom: y + 1.0_f32.max(self.scale.floor()),
                    };
                    rt.FillRectangle(&line, brush);
                    continue;
                }
                if selected {
                    brush.SetColor(&colors.hover);
                    let cell = D2D_RECT_F {
                        left: margin + metrics.pad,
                        top: top + 1.0,
                        right: margin + panel.layout.width - metrics.pad,
                        bottom: top + row.height - 1.0,
                    };
                    fill_round(rt, brush, cell, (radius * 0.6).max(2.0));
                }
                brush.SetColor(if item.selectable() { &colors.text } else { &colors.dim });
                let center_y = top + row.height / 2.0;
                if let Item::Action { checked: true, .. } = item {
                    let s = self.scale;
                    let x = margin + metrics.pad + metrics.gutter / 2.0;
                    let stroke = 1.6 * s;
                    for (a, b) in [((-4.0, 0.5), (-1.5, 3.0)), ((-1.5, 3.0), (4.0, -3.5))] {
                        rt.DrawLine(
                            Vector2 { X: x + a.0 * s, Y: center_y + a.1 * s },
                            Vector2 { X: x + b.0 * s, Y: center_y + b.1 * s },
                            brush,
                            stroke,
                            None,
                        );
                    }
                }
                if let Some(label) = &panel.labels[index] {
                    let mut text_metrics = DWRITE_TEXT_METRICS::default();
                    let _ = label.GetMetrics(&mut text_metrics);
                    let origin = Vector2 {
                        X: margin + metrics.pad + metrics.gutter,
                        Y: center_y - text_metrics.height / 2.0,
                    };
                    rt.DrawTextLayout(origin, label, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
                if let Item::Submenu { .. } = item {
                    let s = self.scale;
                    let x = margin + panel.layout.width - metrics.pad - metrics.arrow / 2.0 - 1.0 * s;
                    for (a, b) in [((-2.0, -4.0), (2.0, 0.0)), ((2.0, 0.0), (-2.0, 4.0))] {
                        rt.DrawLine(
                            Vector2 { X: x + a.0 * s, Y: center_y + a.1 * s },
                            Vector2 { X: x + b.0 * s, Y: center_y + b.1 * s },
                            brush,
                            1.4 * s,
                            None,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn panel_at(&self, hwnd: HWND) -> Option<usize> {
        self.panels.iter().position(|p| p.hwnd == hwnd)
    }

    /// The pointer moved over `panel` at panel-relative `y` (device pixels).
    fn hover(&mut self, panel: usize, y: f32) {
        let margin = if self.look.shadow { SHADOW * self.scale } else { 0.0 };
        let target = {
            let p = &self.panels[panel];
            p.layout.item_at(&p.items, y - margin)
        };
        self.select(panel, target, false);
    }

    /// Makes `target` the selected row of `panel`; a submenu row opens its cascade
    /// (selecting its first item when `into` is set, for the keyboard).
    fn select(&mut self, panel: usize, target: Option<usize>, into: bool) {
        // A cascade of a row other than the new one goes away.
        if self.panels.len() > panel + 1 {
            let keep = self.panels[panel + 1].opened_by == target;
            if !keep {
                for gone in self.panels.drain(panel + 1..) {
                    // SAFETY: our own window, on its creating thread.
                    let _ = unsafe { DestroyWindow(gone.hwnd) };
                }
            }
        }
        if self.panels[panel].hover != target {
            self.panels[panel].hover = target;
            let _ = self.draw(panel);
        }
        let Some(row) = target else { return };
        if !matches!(self.panels[panel].items[row], Item::Submenu { .. }) {
            return;
        }
        if self.panels.len() == panel + 1 {
            self.open_submenu(panel, row);
        }
        if into && let Some(sub) = self.panels.get_mut(panel + 1) {
            sub.hover = menu_model::step(&sub.items, None, 1);
            let _ = self.draw(panel + 1);
        }
    }

    fn open_submenu(&mut self, parent: usize, row: usize) {
        let Item::Submenu { items, .. } = self.panels[parent].items[row].clone() else { return };
        let (parent_rect, row_top) = {
            let p = &self.panels[parent];
            (p.rect(), p.origin.1 + p.layout.rows[row].top.round() as i32)
        };
        let work = self.work_area_of(parent_rect);
        let made = self.make_panel(items, Some(row), |size| {
            menu_model::place_submenu(size, parent_rect, row_top, work)
        });
        match made {
            Ok((origin, panel)) => {
                let hwnd = panel.hwnd;
                self.panels.push(Panel { origin, ..panel });
                let index = self.panels.len() - 1;
                if self.draw(index).is_ok() {
                    // SAFETY: our own window.
                    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
                }
            }
            Err(e) => log::warn!("desktop menu: submenu: {e:#}"),
        }
    }

    /// The work area of the monitor holding `rect`'s top-left.
    fn work_area_of(&self, rect: menu_model::Edges) -> menu_model::Edges {
        crate::monitors::monitor_infos()
            .into_iter()
            .find(|m| m.rect.contains_point(rect.0, rect.1))
            .map_or((0, 0, i32::MAX, i32::MAX), |m| {
                (m.work.x, m.work.y, m.work.x + m.work.width, m.work.y + m.work.height)
            })
    }

    /// The command of the selected row of `panel`, if it is an action.
    fn chosen(&self, panel: usize) -> Option<Command> {
        let p = &self.panels[panel];
        match p.items.get(p.hover?)? {
            Item::Action { command, enabled: true, .. } => Some(command.clone()),
            _ => None,
        }
    }

    /// A key press; keys always drive the innermost open panel.
    fn key(&mut self, key: u16) -> KeyResult {
        let current = self.panels.len() - 1;
        let hover = self.panels[current].hover;
        match key {
            k if k == VK_DOWN.0 || k == VK_UP.0 => {
                let delta = if k == VK_DOWN.0 { 1 } else { -1 };
                let next = menu_model::step(&self.panels[current].items, hover, delta);
                self.select(current, next, false);
                KeyResult::Handled
            }
            k if k == VK_RIGHT.0 => {
                if let Some(row) = hover
                    && matches!(self.panels[current].items[row], Item::Submenu { .. })
                {
                    if self.panels.len() == current + 1 {
                        self.open_submenu(current, row);
                    }
                    if let Some(sub) = self.panels.get_mut(current + 1) {
                        sub.hover = menu_model::step(&sub.items, None, 1);
                        let _ = self.draw(current + 1);
                    }
                }
                KeyResult::Handled
            }
            k if k == VK_LEFT.0 => {
                if current > 0 {
                    self.close_innermost();
                    KeyResult::Handled
                } else {
                    KeyResult::Close
                }
            }
            k if k == VK_ESCAPE.0 => {
                if current > 0 {
                    self.close_innermost();
                    KeyResult::Handled
                } else {
                    KeyResult::Close
                }
            }
            k if k == VK_RETURN.0 => {
                if let Some(command) = self.chosen(current) {
                    KeyResult::Command(command)
                } else {
                    if let Some(row) = hover
                        && matches!(self.panels[current].items[row], Item::Submenu { .. })
                    {
                        self.select(current, Some(row), true);
                    }
                    KeyResult::Handled
                }
            }
            _ => KeyResult::Ignored,
        }
    }

    fn close_innermost(&mut self) {
        if let Some(gone) = self.panels.pop() {
            // SAFETY: our own window, on its creating thread.
            let _ = unsafe { DestroyWindow(gone.hwnd) };
        }
    }
}

enum KeyResult {
    Handled,
    Ignored,
    Close,
    Command(Command),
}

struct Colors {
    panel: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    dim: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    line: D2D1_COLOR_F,
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a,
    }
}

fn grow_rect(r: D2D_RECT_F, by: f32, drop: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.left - by,
        top: r.top - by + drop,
        right: r.right + by,
        bottom: r.bottom + by + drop,
    }
}

/// # Safety
/// Must run between `BeginDraw` and `EndDraw` of `rt`.
unsafe fn fill_round(rt: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush, rect: D2D_RECT_F, radius: f32) {
    let rounded = D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius };
    // SAFETY: per this function's contract.
    unsafe { rt.FillRoundedRectangle(&rounded, brush) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Submenus never take the focus, whoever clicks them.
        WM_MOUSEACTIVATE => {
            let is_cascade = with_ui(|ui| ui.panel_at(hwnd).is_some_and(|i| i > 0)).unwrap_or(false);
            if is_cascade {
                return LRESULT(MA_NOACTIVATE as isize);
            }
        }
        WM_MOUSEMOVE => {
            let y = f32::from((lparam.0 >> 16) as i16);
            with_ui(|ui| {
                if let Some(panel) = ui.panel_at(hwnd) {
                    ui.hover(panel, y);
                }
            });
            return LRESULT(0);
        }
        WM_LBUTTONUP | WM_RBUTTONUP => {
            let y = f32::from((lparam.0 >> 16) as i16);
            let outcome = with_ui(|ui| {
                let panel = ui.panel_at(hwnd)?;
                ui.hover(panel, y);
                ui.chosen(panel)
            })
            .flatten();
            if let Some(command) = outcome {
                choose(command);
            }
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let result = with_ui(|ui| {
                ui.panel_at(hwnd)?;
                Some(ui.key(wparam.0 as u16))
            })
            .flatten();
            match result {
                Some(KeyResult::Command(command)) => choose(command),
                Some(KeyResult::Close) => close(),
                Some(KeyResult::Handled) => {}
                Some(KeyResult::Ignored) | None => {}
            }
            return LRESULT(0);
        }
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            // Focus went elsewhere: the menu is done (unless a cascade is being opened).
            let is_root = with_ui(|ui| ui.panel_at(hwnd) == Some(0)).unwrap_or(false);
            if is_root {
                close();
            }
        }
        WM_NCDESTROY => {
            // SAFETY: clearing our own window's data.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        }
        _ => {}
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// An item was chosen: close the menu, then run its command outside any borrow.
fn choose(command: Command) {
    let callback = with_ui(|ui| ui.on_command.clone());
    close();
    if let Some(callback) = callback {
        callback(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_concrete_font_family_wins() {
        assert_eq!(font_family(r#""Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif"#), "Segoe UI Variable Text");
        assert_eq!(font_family("system-ui, 'Cascadia Code'"), "Cascadia Code");
        assert_eq!(font_family("sans-serif"), "Segoe UI");
        assert_eq!(font_family(""), "Segoe UI");
        assert_eq!(font_family("  Inter  ,serif"), "Inter");
    }
}
