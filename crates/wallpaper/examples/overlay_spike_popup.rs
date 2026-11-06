//! The client side of the overlay spike (`overlay_spike.rs` starts it): a stand-in for a shell
//! popup or widget, drawn with GDI into a per-pixel-alpha layered window. Its whole rounded shape
//! is filled at alpha 1/255 (invisible, but hit), with white text, a magenta marker square (for
//! the exclusion checks) and a hover row on top; the engine's glass shows through the rest.
//!
//! ```text
//! overlay_spike_popup popup|desktop X Y W H RADIUS   # a client surface, driven by stdin
//! overlay_spike_popup animate X Y W H [FPS]          # an animated window (the "video", 60 fps)
//! overlay_spike_popup static X Y W H                 # a still, colourful window (a backdrop)
//! ```
//!
//! stdin commands (one per line): `open`, `close`, `move X Y` (sends `Place`, then moves),
//! `path MS` (circles for MS ms at 60 Hz, sending `Place` each step), `hover on|off` (repaints at
//! 10 Hz), `clock on|off` (repaints every second), `material CLARITY BLUR`, `second` (opens a
//! second popup beside this one), `quit`. stdout reports `ready`, `moved I BEFORE SENT MOVED`
//! (QPC: before sending `Place`, once the engine took it, once this popup moved),
//! `click X Y`, and errors.

#![allow(clippy::too_many_arguments)]

use std::io::BufRead;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

#[path = "../src/overlay/request.rs"]
// The spike uses only the request types from this shared production module.
#[allow(dead_code, unused_imports)]
mod request;

use request::{Layer, Material, Reply, Request};

/// Posted from the stdin thread: a command line waits in `COMMANDS`.
const WM_COMMAND_LINE: u32 = WM_APP + 1;

static COMMANDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn main() -> Result<()> {
    mypaper_desktop::enable_dpi_awareness();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| -> Result<i32> {
        args.get(i)
            .context("missing argument")?
            .parse()
            .context("not a number")
    };
    let rect = Rect {
        x: num(1)?,
        y: num(2)?,
        width: num(3)?,
        height: num(4)?,
    };
    match args.first().map(String::as_str) {
        Some("animate") => animate(rect, args.get(5).and_then(|f| f.parse().ok()).unwrap_or(60)),
        Some("static") => still(rect, args.get(5).is_some_and(|a| a == "cover")),
        Some(kind @ ("popup" | "desktop")) => {
            let layer = if kind == "popup" {
                Layer::Popup
            } else {
                Layer::Desktop
            };
            Client::run(layer, rect, num(5)? as f32)
        }
        _ => bail!("usage: overlay_spike_popup popup|desktop|animate X Y W H [RADIUS]"),
    }
}

/// One popup and what it shows.
struct Surface {
    hwnd: HWND,
    rect: Rect,
    radius: f32,
    hover: bool,
    tick: u32,
}

struct Client {
    engine: HWND,
    layer: Layer,
    surfaces: Vec<Surface>,
    material: Material,
    hover: bool,
    clock: bool,
    moves: u32,
}

impl Client {
    fn run(layer: Layer, rect: Rect, radius: f32) -> Result<()> {
        register()?;
        let engine = request::find_engine().context("no overlay engine running")?;
        let first = Surface::create(layer, rect, radius)?;
        let mut client = Self {
            engine,
            layer,
            surfaces: vec![first],
            material: Material::CLEAR,
            hover: false,
            clock: false,
            moves: 0,
        };
        client.surfaces[0].paint();
        // SAFETY: the main thread's id, for the stdin thread to post to.
        let thread = unsafe { GetCurrentThreadId() };
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                COMMANDS.lock().unwrap().push(line);
                // SAFETY: posting to our own main thread.
                let _ =
                    unsafe { PostThreadMessageW(thread, WM_COMMAND_LINE, WPARAM(0), LPARAM(0)) };
            }
        });
        println!("ready {}", client.surfaces[0].hwnd.0 as usize);
        let mut next_hover = Instant::now();
        let mut next_clock = Instant::now();
        loop {
            // SAFETY: a plain message pump with a 15 ms wait.
            unsafe {
                MsgWaitForMultipleObjects(None, false, 15, QS_ALLINPUT);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_COMMAND_LINE {
                        let lines = std::mem::take(&mut *COMMANDS.lock().unwrap());
                        for line in lines {
                            if !client.command(&line)? {
                                return Ok(());
                            }
                        }
                        continue;
                    }
                    if msg.message == WM_LBUTTONDOWN {
                        let x = (msg.lParam.0 & 0xFFFF) as i16;
                        let y = ((msg.lParam.0 >> 16) & 0xFFFF) as i16;
                        println!("click {x} {y}");
                    }
                    DispatchMessageW(&msg);
                }
            }
            let now = Instant::now();
            if client.hover && now >= next_hover {
                next_hover = now + Duration::from_millis(100);
                for s in &mut client.surfaces {
                    s.hover = !s.hover;
                    s.paint();
                }
            }
            if client.clock && now >= next_clock {
                next_clock = now + Duration::from_secs(1);
                for s in &mut client.surfaces {
                    s.tick += 1;
                    s.paint();
                }
            }
        }
    }

    /// Runs one stdin command; `false` to quit.
    fn command(&mut self, line: &str) -> Result<bool> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let int = |i: usize| {
            words
                .get(i)
                .and_then(|w| w.parse::<i32>().ok())
                .unwrap_or(0)
        };
        match words.first().copied() {
            Some("open") => {
                for i in 0..self.surfaces.len() {
                    self.open(i);
                }
            }
            Some("second") => {
                let first = &self.surfaces[0];
                let rect = Rect {
                    x: first.rect.x + first.rect.width,
                    ..first.rect
                };
                let surface = Surface::create(self.layer, rect, first.radius)?;
                surface.paint();
                self.surfaces.push(surface);
                self.open(self.surfaces.len() - 1);
            }
            Some("close") => {
                for s in &self.surfaces {
                    self.send(&Request::Close { client: s.hwnd });
                }
            }
            Some("move") => self.move_to(int(1), int(2)),
            Some("path") => {
                let (x0, y0) = (self.surfaces[0].rect.x, self.surfaces[0].rect.y);
                let end = Instant::now() + Duration::from_millis(int(1) as u64);
                let start = Instant::now();
                while Instant::now() < end {
                    let t = start.elapsed().as_secs_f32();
                    let (dx, dy) = ((1.0 - (t * 2.0).cos()) * 120.0, (t * 2.0).sin() * 80.0);
                    self.move_to(x0 + dx as i32, y0 + dy as i32);
                    // SAFETY: drains this thread's messages between steps.
                    unsafe {
                        let mut msg = MSG::default();
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            DispatchMessageW(&msg);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
                self.move_to(x0, y0);
                println!("path done");
            }
            Some("hover") => self.hover = words.get(1) == Some(&"on"),
            Some("clock") => self.clock = words.get(1) == Some(&"on"),
            Some("material") => {
                self.material.clarity = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(1.0);
                self.material.blur = words.get(2).and_then(|w| w.parse().ok()).unwrap_or(0.0);
                self.material.tint = [24, 24, 28, 0];
                for s in &self.surfaces {
                    self.send(&Request::Material {
                        client: s.hwnd,
                        material: self.material,
                    });
                }
            }
            Some("quit") => return Ok(false),
            _ => println!("error unknown command {line:?}"),
        }
        Ok(true)
    }

    fn open(&self, i: usize) {
        let s = &self.surfaces[i];
        self.send(&Request::Open {
            client: s.hwnd,
            layer: self.layer,
            rect: s.rect,
            radius: s.radius,
            material: self.material,
        });
    }

    /// Sends `Place` for the first surface, then moves it: both land in the same frame when the
    /// engine takes the request quickly.
    fn move_to(&mut self, x: i32, y: i32) {
        let s = &mut self.surfaces[0];
        s.rect.x = x;
        s.rect.y = y;
        let (client, rect, radius) = (s.hwnd, s.rect, s.radius);
        let before = now();
        self.send(&Request::Place {
            client,
            rect,
            radius,
        });
        let sent = now();
        self.surfaces[0].paint();
        let moved = now();
        println!("moved {} {before} {sent} {moved}", self.moves);
        self.moves += 1;
    }

    fn send(&self, request: &Request) {
        match request::send(self.engine, request) {
            Reply::Taken => {}
            Reply::TakenExcludeYourself => {
                for s in &self.surfaces {
                    // SAFETY: our own window.
                    let _ = unsafe { SetWindowDisplayAffinity(s.hwnd, WDA_EXCLUDEFROMCAPTURE) };
                }
                println!("excluding myself (WDA)");
            }
            Reply::Rejected => println!("error rejected {request:?}"),
        }
    }
}

/// QPC ticks now.
fn now() -> i64 {
    let mut qpc = 0;
    // SAFETY: a plain query into a local.
    let _ = unsafe { QueryPerformanceCounter(&mut qpc) };
    qpc
}

impl Surface {
    fn create(layer: Layer, rect: Rect, radius: f32) -> Result<Self> {
        let hwnd = match layer {
            // SAFETY: plain window creation; the class is registered.
            Layer::Popup => unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("overlay-spike-popup"),
                    w!("spike popup"),
                    WS_POPUP | WS_VISIBLE,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    None,
                    None,
                    None,
                    None,
                )?
            },
            Layer::Desktop => {
                // A widget: a top-level window kept just above the desktop (Progman), so over the
                // wallpaper and icons and under every app. (A child of the desktop can't be:
                // the capture can't exclude child windows, and under the raised desktop neither
                // GDI nor layered children show.)
                // SAFETY: plain window creation, then a z-order-only move of our own window.
                unsafe {
                    let hwnd = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                        w!("overlay-spike-popup"),
                        w!("spike widget"),
                        WS_POPUP | WS_VISIBLE,
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        None,
                        None,
                        None,
                        None,
                    )?;
                    let desktop = GetShellWindow();
                    if let Ok(above) = GetWindow(desktop, GW_HWNDPREV)
                        && above != hwnd
                    {
                        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                        SetWindowPos(hwnd, Some(above), 0, 0, 0, 0, flags)?;
                    }
                    hwnd
                }
            }
        };
        Ok(Self {
            hwnd,
            rect,
            radius,
            hover: false,
            tick: 0,
        })
    }

    /// Draws the surface: alpha 1/255 over its rounded shape, white text, a magenta marker.
    fn paint(&self) {
        let (w, h) = (self.rect.width, self.rect.height);
        let text = format!("mypaper glass  {}", self.tick);
        let r = self.radius;
        let hover = self.hover;
        let at = Some(POINT {
            x: self.rect.x,
            y: self.rect.y,
        });
        let hwnd = self.hwnd;
        draw(hwnd, at, w, h, |dc, px| {
            // SAFETY: GDI text into our own memory DC.
            unsafe {
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00FF_FFFF));
                let mut wide: Vec<u16> = text.encode_utf16().collect();
                let mut rc = RECT {
                    left: 48,
                    top: 18,
                    right: w,
                    bottom: 48,
                };
                DrawTextW(dc, &mut wide, &mut rc, DT_LEFT | DT_SINGLELINE);
            }
            // White text has equal channels: its coverage is its alpha.
            for p in px.iter_mut() {
                let c = *p & 0xFF;
                *p = if c > 0 {
                    c << 24 | c << 16 | c << 8 | c
                } else {
                    0
                };
            }
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let inside = rounded(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, r);
                    if !inside {
                        px[i] = 0;
                        continue;
                    }
                    if (18..34).contains(&x) && (18..34).contains(&y) {
                        px[i] = 0xFFFF_00FF; // the magenta marker
                    } else if hover && (60..92).contains(&y) {
                        px[i] = px[i].max(0x2828_2828);
                    } else if px[i] >> 24 == 0 {
                        px[i] = 0x0100_0000; // invisible, but hit
                    }
                }
            }
        });
    }
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
    let r = r.min(w / 2.0).min(h / 2.0);
    let qx = (x - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y - h / 2.0).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    outside <= 0.0
}

/// Fills a premultiplied 32-bit DIB of `w` x `h` with `fill` and shows it in `hwnd`
/// (`UpdateLayeredWindow`, at the screen point `at`).
fn draw(hwnd: HWND, at: Option<POINT>, w: i32, h: i32, fill: impl FnOnce(HDC, &mut [u32])) {
    // SAFETY: a DIB section we own, selected into a memory DC for one UpdateLayeredWindow.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            println!("error CreateDIBSection");
            return;
        };
        let old = SelectObject(dc, bmp.into());
        let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
        px.fill(0);
        fill(dc, px);
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,
        };
        if let Err(e) = UpdateLayeredWindow(
            hwnd,
            Some(screen),
            at.as_ref().map(std::ptr::from_ref),
            Some(&SIZE { cx: w, cy: h }),
            Some(dc),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        ) {
            println!("error UpdateLayeredWindow {e}");
        }
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
}

fn register() -> Result<()> {
    // SAFETY: plain registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: w!("overlay-spike-popup"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&class);
    }
    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // SAFETY: the system's own message, forwarded unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// An ordinary opaque window (not layered, not topmost) painted with colourful stripes in
/// `WM_PAINT`, like an app window behind a popup, until stdin closes. With `cover` it is a plain
/// app window (not a tool window), so a monitor it covers counts as covered and mypaper's
/// wallpaper pauses there, as under a maximized app.
fn still(rect: Rect, cover: bool) -> Result<()> {
    extern "system" fn paint(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg != WM_PAINT {
            // SAFETY: the system's own message, forwarded unchanged.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        // SAFETY: a standard WM_PAINT with GDI brushes we delete again.
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let colours = [
                0x00E0_6030u32,
                0x0030_C0E0,
                0x0060_E040,
                0x00F0_F0F0,
                0x0040_30D0,
            ];
            for (i, x) in (0..client.right).step_by(28).enumerate() {
                let brush = CreateSolidBrush(COLORREF(colours[i % colours.len()]));
                let band = RECT {
                    left: x,
                    top: 0,
                    right: x + 28,
                    bottom: client.bottom,
                };
                FillRect(dc, &band, brush);
                let _ = DeleteObject(brush.into());
            }
            SetBkMode(dc, TRANSPARENT);
            let mut text: Vec<u16> = "an app window behind the popup".encode_utf16().collect();
            let mut at = RECT {
                left: 8,
                top: client.bottom - 26,
                right: client.right,
                bottom: client.bottom,
            };
            DrawTextW(dc, &mut text, &mut at, DT_LEFT | DT_SINGLELINE);
            let _ = EndPaint(hwnd, &ps);
        }
        LRESULT(0)
    }
    // SAFETY: plain registration and window creation.
    let hwnd = unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(paint),
            lpszClassName: w!("overlay-spike-still"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&class);
        let ex = if cover {
            Default::default()
        } else {
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE
        };
        CreateWindowExW(
            ex,
            w!("overlay-spike-still"),
            w!("spike app window"),
            WS_POPUP,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            None,
            None,
            None,
            None,
        )?
    };
    // SAFETY: shows our own window without activating it.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNA) };
    std::thread::spawn(|| {
        let _ = std::io::stdin().lock().lines().count();
        std::process::exit(0);
    });
    println!("ready {}", hwnd.0 as usize);
    // SAFETY: a plain message pump.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// A plain (not topmost) window showing a gradient moving at `fps` until stdin closes: what a
/// video playing there looks like to the capture.
fn animate(rect: Rect, fps: u64) -> Result<()> {
    register()?;
    // SAFETY: plain window creation.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("overlay-spike-popup"),
            w!("spike video"),
            WS_POPUP | WS_VISIBLE,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            None,
            None,
            None,
            None,
        )?
    };
    std::thread::spawn(|| {
        let _ = std::io::stdin().lock().lines().count();
        std::process::exit(0);
    });
    println!("ready {}", hwnd.0 as usize);
    let start = Instant::now();
    loop {
        let t = start.elapsed().as_secs_f32();
        let (w, h) = (rect.width, rect.height);
        draw(
            hwnd,
            Some(POINT {
                x: rect.x,
                y: rect.y,
            }),
            w,
            h,
            |_, px| {
                let shift = (t * 240.0) as i32;
                for y in 0..h {
                    for x in 0..w {
                        let v = ((x + y + shift) & 0xFF) as u32;
                        let band = ((x / 24 + y / 24) & 1) as u32 * 0x30;
                        px[(y * w + x) as usize] =
                            0xFF00_0000 | v << 16 | (255 - v) << 8 | (0x40 + band);
                    }
                }
            },
        );
        // SAFETY: drains this thread's messages between frames.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(1000 / fps.max(1)));
    }
}
