//! The app-window surface kind ([`Kind::App`](crate::Kind::App)): a normal, resizable,
//! activated window with a taskbar button (Studio). Unlike the layered kinds it is opaque: the
//! software target's dirty rectangle is copied straight to the window, with no per-pixel alpha
//! and no third copy of the pixels (no DIB section).
//!
//! The app draws its own title area. It tells the window which rectangles are the caption and
//! the caption buttons with [`Surface::set_chrome`], and `WM_NCHITTEST` answers from them, so
//! Windows' drag, double-click, Aero Snap and the Windows 11 snap layouts flyout (over the
//! maximize button) all work. The top resize edge is answered there too; the others stay in the
//! invisible frame Windows keeps.
//!
//! Windows runs a modal loop while the user drags the frame, and the app's own loop stops.
//! [`set_modal_hook`] gives the window a way to let the app render during it.

use std::cell::{Cell, RefCell};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateSolidBrush, DIB_RGB_COLORS,
    DeleteObject, EndPaint, FillRect, GetDC, PAINTSTRUCT, ReleaseDC, ScreenToClient,
    SetDIBitsToDevice,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, GetClientRect, HCURSOR,
    HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT, ICON_BIG,
    ICON_SMALL, IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENWSE, IDC_SIZEWE, IsZoomed,
    LR_DEFAULTCOLOR, LoadCursorW, MINMAXINFO, NCCALCSIZE_PARAMS, PostMessageW, RegisterClassExW,
    SC_MINIMIZE, SM_CXPADDEDBORDER, SM_CYFRAME, SW_MAXIMIZE, SW_RESTORE, SW_SHOWMAXIMIZED,
    SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, WM_CLOSE,
    WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_GETMINMAXINFO,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE,
    WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_THEMECHANGED, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::geom::{PixelRect, Point, Rect, Rgba};
use crate::render::{Canvas, Ui};
use crate::scene::DisplayList;
use crate::surface::{Event, Pointer, Surface, SurfaceId, push};

const CLASS: PCWSTR = w!("mypaper.ui.app");
/// Height of the top resize band, in design pixels.
const TOP_EDGE: f32 = 5.0;

/// What a rectangle of the app's own title area is, for `WM_NCHITTEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// Drags the window (and double-click maximizes), wherever no hit item is under the pointer.
    Caption,
    Minimize,
    /// Also where Windows 11 shows its snap layouts flyout.
    Maximize,
    Close,
}

/// The pointer shape over the client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Arrow,
    Text,
    Hand,
    Move,
    Resize,
    ResizeHorizontal,
}

/// What the window procedure of an app window shares with its [`Surface`].
pub(crate) struct AppShared {
    /// Minimum client size in design pixels.
    min: (u32, u32),
    chrome: RefCell<Vec<(Rect, Chrome)>>,
    /// The shown hit rectangles: the caption never takes a click meant for a control.
    pub(crate) hits: RefCell<Vec<Rect>>,
    /// The pixels on screen, kept to repaint what Windows uncovers.
    canvas: RefCell<Option<Canvas>>,
    size: Cell<(u32, u32)>,
    /// Fills what the canvas doesn't cover yet (a frame mid-resize).
    background: Cell<Rgba>,
    cursor: Cell<Cursor>,
    /// Inside Windows' modal size/move loop.
    sizing: Cell<bool>,
    /// The caption button pressed (an `HT*` code), until it's released.
    pressed: Cell<u32>,
}

impl AppShared {
    /// Drop retained software pixels while Windows keeps the last presented frame visible.
    pub(crate) fn release_canvas(&self) {
        self.canvas.borrow_mut().take();
    }

    pub(crate) fn new(min: (u32, u32)) -> Self {
        Self {
            min,
            chrome: RefCell::new(Vec::new()),
            hits: RefCell::new(Vec::new()),
            canvas: RefCell::new(None),
            size: Cell::new((0, 0)),
            background: Cell::new(Rgba::new(0.04, 0.04, 0.04, 1.0)),
            cursor: Cell::new(Cursor::Arrow),
            sizing: Cell::new(false),
            pressed: Cell::new(0),
        }
    }
}

thread_local! {
    static MODAL: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// While the user drags an app window's frame, Windows runs its own loop and the app's stops.
/// `hook` runs after each resize step inside that loop: it should handle the queued events and
/// present the new size. It must not assume it can borrow app state (use `try_borrow_mut`).
pub fn set_modal_hook(hook: Option<Box<dyn FnMut()>>) {
    MODAL.with(|m| *m.borrow_mut() = hook);
}

fn run_modal_hook() {
    // Taken out while it runs, so a nested resize can't run it twice.
    let hook = MODAL.with(|m| m.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook();
        MODAL.with(|m| {
            let mut slot = m.borrow_mut();
            if slot.is_none() {
                *slot = Some(hook);
            }
        });
    }
}

/// Creates the (hidden) window of an app surface.
pub(crate) fn create(
    name: &str,
    wndproc: windows::Win32::UI::WindowsAndMessaging::WNDPROC,
) -> Result<HWND> {
    // SAFETY: plain Win32 registration with a 'static window procedure, then a window of it.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: wndproc,
            style: CS_DBLCLKS,
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
        CreateWindowExW(
            Default::default(),
            CLASS,
            &HSTRING::from(name),
            WS_OVERLAPPEDWINDOW,
            windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            1280,
            800,
            None,
            None,
            None,
            None,
        )
        .context("creating an app window")
    }
}

/// After the window holds its shared state: recompute the frame without the caption.
pub(crate) fn created(hwnd: HWND) {
    // SAFETY: our own window; a frame recalculation only.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn scale_of(hwnd: HWND) -> f32 {
    // SAFETY: a query on our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

fn client_point(hwnd: HWND, lparam: LPARAM) -> Point {
    let mut p = POINT {
        x: i32::from(lparam.0 as i16),
        y: i32::from((lparam.0 >> 16) as i16),
    };
    // SAFETY: our own window and a local point.
    let _ = unsafe { ScreenToClient(hwnd, &mut p) };
    Point::new(p.x as f32, p.y as f32)
}

fn pointer(hwnd: HWND, lparam: LPARAM) -> Pointer {
    let at = client_point(hwnd, lparam);
    Pointer {
        at,
        screen: (
            i32::from(lparam.0 as i16),
            i32::from((lparam.0 >> 16) as i16),
        ),
    }
}

fn zoomed(hwnd: HWND) -> bool {
    // SAFETY: a query on our own window.
    unsafe { IsZoomed(hwnd) }.as_bool()
}

/// `WM_NCHITTEST` for a client point.
fn hit_test(app: &AppShared, hwnd: HWND, p: Point) -> u32 {
    let scale = scale_of(hwnd);
    let (width, _) = app.size.get();
    if !zoomed(hwnd) && p.y >= 0.0 && p.y < TOP_EDGE * scale {
        let corner = 16.0 * scale;
        return if p.x < corner {
            HTTOPLEFT
        } else if p.x > width as f32 - corner {
            HTTOPRIGHT
        } else {
            HTTOP
        };
    }
    let chrome = app.chrome.borrow();
    for (rect, kind) in chrome.iter().rev() {
        if !rect.contains(p) {
            continue;
        }
        return match kind {
            Chrome::Minimize => HTMINBUTTON,
            Chrome::Maximize => HTMAXBUTTON,
            Chrome::Close => HTCLOSE,
            Chrome::Caption if app.hits.borrow().iter().any(|h| h.contains(p)) => HTCLIENT,
            Chrome::Caption => HTCAPTION,
        };
    }
    HTCLIENT
}

/// The app window's own messages. `None` passes the message on to the shared handling.
pub(crate) fn message(
    id: SurfaceId,
    app: &AppShared,
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match msg {
        WM_NCCALCSIZE if wparam.0 != 0 => {
            // SAFETY: with wparam TRUE, lparam is a live NCCALCSIZE_PARAMS. Windows computes
            // the standard frame; we take back the caption (the top stays where it was).
            unsafe {
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                let _ = DefWindowProcW(hwnd, msg, wparam, lparam);
                params.rgrc[0].top = top;
                if zoomed(hwnd) {
                    // A maximized window hangs its frame off the screen: keep the content on it.
                    let dpi = GetDpiForWindow(hwnd);
                    params.rgrc[0].top += GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                }
            }
            Some(LRESULT(0))
        }
        WM_NCHITTEST => {
            // SAFETY: default handling first, for the side and bottom resize edges.
            let default = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            if default.0 as u32 != HTCLIENT {
                return Some(default);
            }
            Some(LRESULT(
                hit_test(app, hwnd, client_point(hwnd, lparam)) as isize
            ))
        }
        WM_NCMOUSEMOVE => {
            // The caption buttons are non-client: the app still draws their hover.
            // SAFETY: tracks our own window's non-client leave.
            unsafe {
                let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                });
            }
            push(id, Event::Move(pointer(hwnd, lparam)));
            None
        }
        WM_NCMOUSELEAVE => {
            push(id, Event::Leave);
            None
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
            if matches!(wparam.0 as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) =>
        {
            // Never to DefWindowProc: it would draw the classic buttons.
            app.pressed.set(wparam.0 as u32);
            Some(LRESULT(0))
        }
        WM_NCLBUTTONUP if matches!(wparam.0 as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) => {
            let code = wparam.0 as u32;
            if app.pressed.replace(0) == code {
                match code {
                    // SAFETY: standard commands to our own window.
                    HTMINBUTTON => unsafe {
                        let _ = PostMessageW(
                            Some(hwnd),
                            WM_SYSCOMMAND,
                            WPARAM(SC_MINIMIZE as usize),
                            LPARAM(0),
                        );
                    },
                    HTMAXBUTTON => toggle_maximize(hwnd),
                    _ => push(id, Event::Close),
                }
            }
            Some(LRESULT(0))
        }
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
            let shape = match app.cursor.get() {
                Cursor::Arrow => IDC_ARROW,
                Cursor::Text => IDC_IBEAM,
                Cursor::Hand => IDC_HAND,
                Cursor::Move => IDC_SIZEALL,
                Cursor::Resize => IDC_SIZENWSE,
                Cursor::ResizeHorizontal => IDC_SIZEWE,
            };
            // SAFETY: a shared system cursor.
            unsafe {
                if let Ok(cursor) = LoadCursorW(None, shape) {
                    SetCursor(Some(HCURSOR(cursor.0)));
                }
            }
            Some(LRESULT(1))
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_PAINT => {
            paint(app, hwnd);
            Some(LRESULT(0))
        }
        WM_GETMINMAXINFO => {
            let scale = scale_of(hwnd);
            // SAFETY: lparam is a live MINMAXINFO for this message.
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize.x = (app.min.0 as f32 * scale) as i32;
                info.ptMinTrackSize.y = (app.min.1 as f32 * scale) as i32;
            }
            Some(LRESULT(0))
        }
        WM_SIZE => {
            let (w, h) = (
                (lparam.0 & 0xffff) as u32,
                ((lparam.0 >> 16) & 0xffff) as u32,
            );
            if w > 0 && h > 0 {
                push(id, Event::Resized(w, h));
                if app.sizing.get() {
                    run_modal_hook();
                }
            }
            None
        }
        WM_ENTERSIZEMOVE => {
            app.sizing.set(true);
            None
        }
        WM_EXITSIZEMOVE => {
            app.sizing.set(false);
            None
        }
        WM_DPICHANGED => {
            // SAFETY: lparam is the suggested RECT for the new DPI.
            unsafe {
                let r = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            None
        }
        WM_CLOSE => {
            push(id, Event::Close);
            Some(LRESULT(0))
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            push(id, Event::SettingChanged);
            None
        }
        WM_SYSKEYDOWN => {
            push(id, Event::Key(wparam.0 as u16));
            // Alt+F4 and Alt+Space keep their system meaning; other Alt chords are the app's
            // (and must not beep for a menu the window doesn't have).
            if matches!(wparam.0 as u16, 0x73 | 0x20) {
                None
            } else {
                Some(LRESULT(0))
            }
        }
        _ => None,
    }
}

fn toggle_maximize(hwnd: HWND) {
    // SAFETY: our own window.
    let _ = unsafe {
        ShowWindow(
            hwnd,
            if zoomed(hwnd) {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            },
        )
    };
}

fn colorref(c: Rgba) -> COLORREF {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(b(c.r) | b(c.g) << 8 | b(c.b) << 16)
}

/// Copies `area` of the canvas to the window (opaque: the alpha byte is ignored).
fn blit(hwnd: HWND, canvas: &Canvas, area: PixelRect) -> Result<()> {
    // SAFETY: this is our window's DC. Each band supplies a complete top-down DIB;
    // the WIC read lock and its bounded packing buffer stay live through the call.
    unsafe {
        let dc = GetDC(Some(hwnd));
        let result = canvas.read_bands(area, 64 << 10, |area, pixels| {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: area.width as i32,
                    biHeight: -(area.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            SetDIBitsToDevice(
                dc,
                area.x as i32,
                area.y as i32,
                area.width,
                area.height,
                0,
                0,
                0,
                area.height,
                pixels.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        });
        ReleaseDC(Some(hwnd), dc);
        result
    }
}

/// `WM_PAINT`: what Windows uncovered, from the kept pixels (the background past them).
fn paint(app: &AppShared, hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: Begin/EndPaint pair on our own window; the brush is deleted after use.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        let r = ps.rcPaint;
        let (w, h) = app.size.get();
        let covered = Rect {
            left: r.left as f32,
            top: r.top as f32,
            right: r.right as f32,
            bottom: r.bottom as f32,
        };
        let drawn = app
            .canvas
            .try_borrow()
            .ok()
            .and_then(|canvas| {
                let canvas = canvas.as_ref()?;
                let area = covered.pixels(w, h)?;
                blit(hwnd, canvas, area).ok()
            })
            .is_some();
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        if !drawn || client.right as u32 > w || client.bottom as u32 > h {
            let brush = CreateSolidBrush(colorref(app.background.get()));
            for rect in [
                RECT {
                    left: (w as i32).max(r.left),
                    ..r
                },
                RECT {
                    top: (h as i32).max(r.top),
                    ..r
                },
            ] {
                if !drawn || (rect.right > rect.left && rect.bottom > rect.top) {
                    FillRect(dc, &if drawn { rect } else { r }, brush);
                }
            }
            let _ = DeleteObject(brush.into());
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

impl Surface {
    fn app(&self) -> &AppShared {
        self.shared
            .app
            .as_ref()
            .expect("an app-window method on a layered surface")
    }

    /// `present` for an app window: draw the damage, then copy it to the window.
    pub(crate) fn present_app(
        &mut self,
        ui: &mut Ui,
        size: (u32, u32),
        list: DisplayList,
    ) -> Result<bool> {
        let app = self.shared.app.as_ref().expect("app window");
        let resized = app.canvas.borrow().is_none() || size != app.size.get();
        if resized {
            *app.canvas.borrow_mut() = None; // the old pixels go before the new ones come
            *app.canvas.borrow_mut() = Some(Canvas::new(ui, size.0, size.1)?);
            app.size.set(size);
        }
        let dirty = if resized {
            Some(PixelRect::full(size.0, size.1))
        } else {
            list.damage(self.shown())
                .and_then(|rect| rect.pixels(size.0, size.1))
        };
        if let Some(area) = dirty {
            let canvas = app.canvas.borrow();
            let canvas = canvas.as_ref().expect("allocated above");
            canvas.draw(&mut ui.text, &list, area)?;
            blit(self.hwnd(), canvas, area)?;
        }
        self.set_shown(list);
        Ok(dirty.is_some())
    }

    /// Where the window's frame is, in screen pixels: `(x, y, width, height)`.
    pub fn place(&self, bounds: (i32, i32, i32, i32)) {
        // SAFETY: our own window.
        let _ = unsafe {
            SetWindowPos(
                self.hwnd(),
                None,
                bounds.0,
                bounds.1,
                bounds.2,
                bounds.3,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    /// Shows an app window and brings it to the front (`maximized` as it was left).
    pub fn show_active(&self, maximized: bool) {
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(
                self.hwnd(),
                if maximized {
                    SW_SHOWMAXIMIZED
                } else {
                    SW_SHOWNORMAL
                },
            );
            let _ = SetForegroundWindow(self.hwnd());
        }
    }

    /// The client area's size in pixels.
    pub fn client_size(&self) -> (u32, u32) {
        let mut r = RECT::default();
        // SAFETY: our own window and a local rect.
        let _ = unsafe { GetClientRect(self.hwnd(), &mut r) };
        (
            (r.right - r.left).max(0) as u32,
            (r.bottom - r.top).max(0) as u32,
        )
    }

    /// The optional alpha app canvas keeps native caption hit-testing in client pixels.
    pub(crate) fn record_layered_app_size(&self, size: (u32, u32)) {
        self.app().size.set(size);
        // The alpha canvas owns the pixels now. A later opaque present must allocate for the
        // current client size, including a resize that happened while this canvas was active.
        *self.app().canvas.borrow_mut() = None;
    }

    /// Device pixels per design pixel (the window's DPI / 96).
    pub fn scale(&self) -> f32 {
        scale_of(self.hwnd())
    }

    pub fn is_maximized(&self) -> bool {
        zoomed(self.hwnd())
    }

    pub fn minimize(&self) {
        // SAFETY: a standard command to our own window.
        let _ = unsafe {
            PostMessageW(
                Some(self.hwnd()),
                WM_SYSCOMMAND,
                WPARAM(SC_MINIMIZE as usize),
                LPARAM(0),
            )
        };
    }

    pub fn toggle_maximize(&self) {
        toggle_maximize(self.hwnd());
    }

    /// The title area's parts, in client pixels (see [`Chrome`]). Later entries win.
    pub fn set_chrome(&self, parts: &[(Rect, Chrome)]) {
        let mut chrome = self.app().chrome.borrow_mut();
        if chrome.as_slice() != parts {
            *chrome = parts.to_vec();
        }
    }

    /// What shows where the window has grown and the app hasn't drawn yet.
    pub fn set_background(&self, color: Rgba) {
        self.app().background.set(color);
    }

    pub fn set_cursor(&self, cursor: Cursor) {
        self.app().cursor.set(cursor);
    }

    /// Dark or light window frame (DWM), and the 1 px border's colour on Windows 11.
    pub fn set_frame(&self, dark: bool, border: Rgba) {
        let dark = windows::core::BOOL::from(dark);
        let border = colorref(border);
        // SAFETY: documented attributes with correctly sized values; failures (older
        // Windows) leave the default frame.
        unsafe {
            let _ = DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                size_of::<windows::core::BOOL>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_BORDER_COLOR,
                (&raw const border).cast(),
                size_of::<COLORREF>() as u32,
            );
        }
    }

    /// The taskbar and Alt+Tab icon, from PNG bytes (the window class has none).
    pub fn set_icon_png(&self, png: &[u8]) {
        for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
            // SAFETY: Windows parses the PNG resource bytes; the icon lives as long as the
            // window (one per size, for the process lifetime).
            unsafe {
                if let Ok(icon) =
                    CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
                {
                    SendMessageW(
                        self.hwnd(),
                        WM_SETICON,
                        Some(WPARAM(kind as usize)),
                        Some(LPARAM(icon.0 as isize)),
                    );
                }
            }
        }
    }
}
