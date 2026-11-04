//! One bar on one monitor: a topmost, transparent top-level window holding a WebView2 with the
//! page. Docked bars register as app bars and reserve their band; an auto-hiding dock reserves
//! nothing and gets a 1 px edge trigger instead. The window is the band plus [`ROOM`] toward the
//! middle of the screen for popovers and menus; the page's region keeps the rest click-through.
//!
//! The window procedures hold no state: they forward what the main loop cares about as thread
//! messages (`WM_BARS_*`).

use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use mypaper_desktop::{Monitor, Rect};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::Shell::{ABM_ACTIVATE, ABM_WINDOWPOSCHANGED, APPBARDATA, SHAppBarMessage};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GA_ROOT, GetAncestor, GetCursorPos,
    HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, PBT_APMPOWERSTATUSCHANGE,
    PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW, SPI_SETWORKAREA, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    WA_ACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMECHANGE,
    WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WindowFromPoint,
};
use windows::core::{PCWSTR, w};

use crate::appbar::{AppBar, Edge, settle, window_rect};
use crate::bridge::BarKind;
use crate::web::Web;

/// Extra room every bar window has beyond its band, for flyouts, menus and magnify (DIPs). It's
/// the most a flyout can extend past its bar; raise it (or make it per bar) for bigger ones.
pub const ROOM: f64 = 360.0;

const BAR_CLASS: PCWSTR = w!("mypaper.bars.bar");
const EDGE_CLASS: PCWSTR = w!("mypaper.bars.edge");
const BROADCAST_CLASS: PCWSTR = w!("mypaper.bars.broadcast");

/// Sent by Explorer to a registered app bar (`uCallbackMessage`).
const WM_APPBAR_CALLBACK: u32 = WM_APP + 20;
/// Posted to the main thread. `wParam`: the `ABN_*` code; `lParam`: the bar's window.
pub const WM_BARS_APPBAR: u32 = WM_APP + 21;
/// Posted to the main thread: the pointer touched an auto-hiding dock's edge (`lParam`: the edge
/// window).
pub const WM_BARS_EDGE: u32 = WM_APP + 22;
/// Posted to the main thread: displays or their scale changed.
pub const WM_BARS_DISPLAY: u32 = WM_APP + 23;
/// Posted to the main thread: Explorer (re)started (`TaskbarCreated`).
pub const WM_BARS_TASKBAR: u32 = WM_APP + 24;
/// Posted to the main thread: the battery or power source changed.
pub const WM_BARS_POWER: u32 = WM_APP + 25;
/// Posted to the main thread: the system clock or time zone changed.
pub const WM_BARS_TIME: u32 = WM_APP + 26;
/// Posted to the main thread: a bar was activated without a click (`lParam`: its window).
pub const WM_BARS_ACTIVATED: u32 = WM_APP + 27;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn post(message: u32, wparam: usize, lparam: isize) {
    // SAFETY: window procedures run on the main thread, which pumps these.
    let _ = unsafe {
        PostThreadMessageW(
            GetCurrentThreadId(),
            message,
            WPARAM(wparam),
            LPARAM(lparam),
        )
    };
}

/// Registers the window classes. Call once, on the main thread.
pub fn register_classes() -> Result<()> {
    // SAFETY: plain Win32 registration with 'static window procedures and class names.
    unsafe {
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
        let instance = GetModuleHandleW(None)?.into();
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        for (class, proc) in [
            (BAR_CLASS, bar_proc as WndProc),
            (BROADCAST_CLASS, bar_proc),
            (EDGE_CLASS, edge_proc),
        ] {
            let info = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: instance,
                hCursor: cursor,
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassExW(&info) == 0 {
                return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
            }
        }
    }
    Ok(())
}

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// A hidden top-level window that hears system broadcasts (`TaskbarCreated`, display, power
/// and time changes) even while no bar is up. Message-only windows don't get broadcasts.
pub fn broadcast_window() -> Result<HWND> {
    // SAFETY: the class is registered; the window is never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            BROADCAST_CLASS,
            w!("mypaper bars"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )?
    };
    Ok(hwnd)
}

unsafe extern "system" fn bar_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let appbar_notify = |message: u32| {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            ..Default::default()
        };
        // SAFETY: tells Explorer about our own window; ignored for windows that aren't app bars.
        unsafe { SHAppBarMessage(message, &mut data) };
    };
    match msg {
        WM_ERASEBKGND => return LRESULT(1), // the WebView paints everything
        WM_APPBAR_CALLBACK => {
            post(WM_BARS_APPBAR, wparam.0, hwnd.0 as isize);
            return LRESULT(0);
        }
        // App bars must report these (SHAppBarMessage docs).
        WM_ACTIVATE => {
            appbar_notify(ABM_ACTIVATE);
            // Activated, but not by a click: the main loop gives the foreground back. A click on
            // the page arrives as WA_ACTIVE too (the WebView's input window takes focus on mouse
            // down), so it counts as a click while a button is held or the pointer is over this
            // bar's visible part. When Windows picks us after an app minimized or closed, or
            // WebView2 takes focus as it starts, the pointer is elsewhere.
            if (wparam.0 & 0xFFFF) as u32 == WA_ACTIVE
                && !mouse_button_down()
                && !pointer_over(hwnd)
            {
                post(WM_BARS_ACTIVATED, 0, hwnd.0 as isize);
            }
        }
        WM_WINDOWPOSCHANGED => appbar_notify(ABM_WINDOWPOSCHANGED),
        WM_DISPLAYCHANGE => post(WM_BARS_DISPLAY, 0, 0),
        // The work area moved (an app bar came or went, the taskbar changed): an auto-hiding dock
        // sits at its edge.
        WM_SETTINGCHANGE if wparam.0 as u32 == SPI_SETWORKAREA.0 => post(WM_BARS_DISPLAY, 0, 0),
        // The host places and scales bars itself; don't take Windows' suggested rect.
        WM_DPICHANGED => {
            post(WM_BARS_DISPLAY, 0, 0);
            return LRESULT(0);
        }
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMPOWERSTATUSCHANGE => {
            post(WM_BARS_POWER, 0, 0)
        }
        WM_TIMECHANGE => post(WM_BARS_TIME, 0, 0),
        // Alt+F4 on a focused bar must not close it.
        WM_CLOSE => return LRESULT(0),
        _ if msg == TASKBAR_CREATED.load(Ordering::Relaxed) && msg != 0 => {
            post(WM_BARS_TASKBAR, 0, 0);
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Is any mouse button held (physically or by injected input) right now?
fn mouse_button_down() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        // SAFETY: no preconditions.
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0)
}

/// Is the pointer over the visible part of `bar` (the window under it, region respected, is the
/// bar or its WebView)?
fn pointer_over(bar: HWND) -> bool {
    let mut point = POINT::default();
    // SAFETY: read-only queries; `point` outlives the call.
    unsafe {
        GetCursorPos(&mut point).is_ok() && GetAncestor(WindowFromPoint(point), GA_ROOT) == bar
    }
}

unsafe extern "system" fn edge_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            post(WM_BARS_EDGE, 0, hwnd.0 as isize);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        // SAFETY: default handling of a message we were called with.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Where a bar sits and how thick it is, from `bars.json`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub edge: Edge,
    /// The band's thickness in DIPs.
    pub thickness: f64,
    /// Reserve the band as an app bar (everything but an auto-hiding dock).
    pub reserve: bool,
}

/// One bar's windows and WebView.
pub struct Bar {
    pub id: u32,
    pub kind: BarKind,
    pub monitor: Monitor,
    pub geometry: Geometry,
    hwnd: HWND,
    edge: Option<HWND>,
    appbar: Option<AppBar>,
    pub web: Option<Web>,
    /// Asked the environment for a WebView and waiting for it.
    pub creating: bool,
    /// The page said `hello`: events may flow.
    pub ready: bool,
    /// Hidden for a fullscreen app, or suspended while the session is locked.
    pub hidden: bool,
    /// The window's rect on screen, in px.
    pub rect: Rect,
    /// The page's visible parts, in DIPs from the window's top-left.
    region: Vec<[f64; 4]>,
}

impl Bar {
    pub fn new(id: u32, kind: BarKind, monitor: Monitor, geometry: Geometry) -> Result<Self> {
        // SAFETY: the class is registered; the window is ours.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
                BAR_CLASS,
                match kind {
                    BarKind::Top => w!("mypaper top bar"),
                    BarKind::Dock => w!("mypaper dock"),
                },
                WS_POPUP | WS_CLIPCHILDREN,
                monitor.rect.x,
                monitor.rect.y,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        };
        let mut bar = Self {
            id,
            kind,
            monitor,
            geometry,
            hwnd,
            edge: None,
            appbar: None,
            web: None,
            creating: false,
            ready: false,
            hidden: false,
            rect: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            region: Vec::new(),
        };
        bar.apply_region();
        bar.place();
        Ok(bar)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn edge_hwnd(&self) -> Option<HWND> {
        self.edge
    }

    pub fn scale(&self) -> f64 {
        f64::from(self.monitor.dpi) / 96.0
    }

    /// The window's size in px.
    pub fn size(&self) -> (i32, i32) {
        (self.rect.width, self.rect.height)
    }

    /// Follows a changed monitor or config. Returns whether its geometry changed (the work areas
    /// may then change too).
    pub fn update(&mut self, monitor: Monitor, geometry: Geometry) -> bool {
        if monitor == self.monitor && geometry == self.geometry {
            return false;
        }
        let moved = geometry != self.geometry || monitor.rect != self.monitor.rect;
        if geometry.edge != self.geometry.edge || !geometry.reserve {
            self.appbar = None; // re-registered on its new edge below, if it reserves
        }
        self.monitor = monitor;
        self.geometry = geometry;
        self.place();
        moved
    }

    /// Explorer restarted: it forgot every app bar.
    pub fn reregister(&mut self) {
        self.appbar = None;
        self.place();
    }

    /// Something moved other app bars: take our band again.
    pub fn appbar_moved(&mut self) {
        if let Some(appbar) = &mut self.appbar {
            appbar.invalidate();
        }
        self.place();
    }

    /// Reserves the band (if docked), moves the window and the edge trigger, and refits the
    /// WebView and the region.
    pub fn place(&mut self) {
        let scale = self.scale();
        let px = |dip: f64| (dip * scale).round() as i32;
        let Geometry {
            edge,
            thickness,
            reserve,
        } = self.geometry;
        let thickness = px(thickness);
        let band = if reserve {
            if self.appbar.is_none() {
                self.appbar = AppBar::register(self.hwnd, edge, WM_APPBAR_CALLBACK);
            }
            match &mut self.appbar {
                Some(appbar) => appbar.reserve(self.monitor.rect, thickness),
                None => settle(self.monitor.work, edge, thickness),
            }
        } else {
            self.appbar = None;
            // An auto-hiding dock sits at the work area's edge: the screen edge, unless the
            // native taskbar is kept there.
            settle(self.monitor.work, edge, thickness)
        };
        self.rect = window_rect(band, edge, px(ROOM), self.monitor.rect);
        let Rect {
            x,
            y,
            width,
            height,
        } = self.rect;
        // SAFETY: our own window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            if !self.hidden {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        if let Some(web) = &self.web {
            web.resize(scale, self.size());
        }
        self.place_edge(band);
        self.apply_region();
    }

    /// The 1 px trigger on an auto-hiding dock's screen edge.
    fn place_edge(&mut self, band: Rect) {
        if self.geometry.reserve {
            if let Some(edge) = self.edge.take() {
                // SAFETY: our own window.
                let _ = unsafe { DestroyWindow(edge) };
            }
            return;
        }
        let line = match self.geometry.edge {
            Edge::Bottom => Rect {
                y: band.y + band.height - 1,
                height: 1,
                ..band
            },
            Edge::Top => Rect { height: 1, ..band },
            Edge::Left => Rect { width: 1, ..band },
            Edge::Right => Rect {
                x: band.x + band.width - 1,
                width: 1,
                ..band
            },
        };
        let edge = match self.edge {
            Some(edge) => edge,
            // SAFETY: the class is registered; alpha 1 is invisible but still takes the pointer.
            None => match unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                    EDGE_CLASS,
                    w!("mypaper dock edge"),
                    WS_POPUP,
                    line.x,
                    line.y,
                    line.width,
                    line.height,
                    None,
                    None,
                    None,
                    None,
                )
            } {
                Ok(edge) => {
                    // SAFETY: our own layered window.
                    let _ = unsafe { SetLayeredWindowAttributes(edge, COLORREF(0), 1, LWA_ALPHA) };
                    self.edge = Some(edge);
                    edge
                }
                Err(e) => return log::warn!("no auto-hide edge trigger: {e}"),
            },
        };
        // SAFETY: our own window.
        unsafe {
            let _ = SetWindowPos(
                edge,
                Some(HWND_TOPMOST),
                line.x,
                line.y,
                line.width,
                line.height,
                SWP_NOACTIVATE,
            );
            let _ = ShowWindow(
                edge,
                if self.hidden {
                    SW_HIDE
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
        }
    }

    /// The page's visible parts (DIPs from the window's top-left); the rest clicks through.
    pub fn set_region(&mut self, rects: Vec<[f64; 4]>) {
        if rects != self.region {
            self.region = rects;
            self.apply_region();
        }
    }

    fn apply_region(&self) {
        let scale = self.scale();
        // SAFETY: GDI regions we create; SetWindowRgn takes ownership of `region`.
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            for [x, y, width, height] in &self.region {
                let px = |v: f64| (v * scale).round() as i32;
                let part = CreateRectRgn(px(*x), px(*y), px(x + width), px(y + height));
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
            SetWindowRgn(self.hwnd, Some(region), true);
        }
    }

    /// Hidden: the windows go away and the WebView is suspended. Shown: the reverse.
    pub fn set_hidden(&mut self, hidden: bool) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        if let Some(web) = &self.web {
            web.set_hidden(hidden);
        }
        let show = if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE };
        // SAFETY: our own windows.
        unsafe {
            let _ = ShowWindow(self.hwnd, show);
            if let Some(edge) = self.edge {
                let _ = ShowWindow(edge, show);
            }
        }
        if !hidden {
            self.place(); // back on top, where it belongs
        }
    }

    pub fn post(&self, json: &str) {
        if let Some(web) = &self.web {
            web.post(json);
        }
    }

    /// The WebView died: drop it so a new one can be attached.
    pub fn drop_web(&mut self) {
        self.web = None;
        self.creating = false;
        self.ready = false;
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        self.web = None;
        self.appbar = None; // ABM_REMOVE before the window goes
        // SAFETY: our own windows, destroyed on their creating thread.
        unsafe {
            if let Some(edge) = self.edge.take() {
                let _ = DestroyWindow(edge);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
