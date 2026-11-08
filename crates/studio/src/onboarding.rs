//! The full-screen first run's windows. Every display fades to black behind plain "curtain"
//! windows, Studio covers the main display above everything while onboarding runs, and at the
//! end the main display fades to black again, Studio goes back to a normal window and the
//! curtains fade away to show it on the desktop.
//!
//! Curtains are layered GDI windows with a black class brush: no drawing of our own, and DWM
//! does the fading. They never take focus and are gone when onboarding ends.

use std::time::{Duration, Instant};

use mypaper_desktop::Monitor;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_NOTOPMOST,
    HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    TranslateMessage, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

const FADE_IN: Duration = Duration::from_millis(450);
const COVER: Duration = Duration::from_millis(280);
const FADE_OUT: Duration = Duration::from_millis(500);

/// An onboarding in progress: one curtain per display, `main` the one under Studio.
pub struct Session {
    curtains: Vec<HWND>,
    main: usize,
}

impl Session {
    /// Fades every display to black, then places `studio` over the main display, topmost and
    /// square-cornered. Studio's first frame is near-black too, so nothing flashes.
    pub fn begin(studio: HWND, monitors: &[Monitor]) -> Self {
        let curtains: Vec<HWND> = monitors.iter().filter_map(curtain).collect();
        let main = monitors.iter().position(|m| m.primary).unwrap_or(0).min(curtains.len().saturating_sub(1));
        fade(&curtains, 0.0, 1.0, FADE_IN);
        if let Some(m) = monitors.get(main) {
            let r = m.rect;
            // SAFETY: our own window; documented, correctly sized DWM attributes.
            unsafe {
                let _ = SetWindowPos(studio, Some(HWND_TOPMOST), r.x, r.y, r.width, r.height, SWP_NOACTIVATE);
                let corner = DWMWCP_DONOTROUND;
                let _ = DwmSetWindowAttribute(studio, DWMWA_WINDOW_CORNER_PREFERENCE, (&raw const corner).cast(), size_of_val(&corner) as u32);
                // DWMWA_COLOR_NONE: no 1 px border around the full-screen window.
                let none = COLORREF(0xFFFF_FFFE);
                let _ = DwmSetWindowAttribute(studio, DWMWA_BORDER_COLOR, (&raw const none).cast(), size_of::<COLORREF>() as u32);
            }
        }
        Self { curtains, main }
    }

    /// Fades the main display to black over Studio and returns Studio to a normal window at
    /// `bounds`; [`Session::reveal`] then shows the desktop again.
    pub fn cover(&self, studio: HWND, bounds: (i32, i32, i32, i32)) {
        let main: Vec<HWND> = self.curtains.get(self.main).copied().into_iter().collect();
        set_alpha(&main, 0.0);
        // SAFETY: our own windows; z-order and placement only.
        unsafe {
            for &curtain in &main {
                let _ = SetWindowPos(curtain, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        fade(&main, 0.0, 1.0, COVER);
        // SAFETY: our own window.
        unsafe {
            let _ = SetWindowPos(studio, Some(HWND_NOTOPMOST), bounds.0, bounds.1, bounds.2, bounds.3, SWP_NOACTIVATE);
        }
        crate::panel::round_window(studio);
    }

    /// Fades every curtain away, showing the desktop with Studio on it, and closes them.
    pub fn reveal(self) {
        fade(&self.curtains, 1.0, 0.0, FADE_OUT);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        for &curtain in &self.curtains {
            // SAFETY: windows this session created on this thread.
            let _ = unsafe { DestroyWindow(curtain) };
        }
    }
}

unsafe extern "system" fn curtain_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the default handling for a window that only paints its black class brush.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A black, click-through-free, never-activated window over `monitor`, fully transparent.
fn curtain(monitor: &Monitor) -> Option<HWND> {
    let r = monitor.rect;
    // SAFETY: registering a plain class (a second registration fails harmlessly) and creating
    // a popup owned by this thread.
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(curtain_proc),
            hInstance: instance.into(),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            lpszClassName: w!("mypaper.studio.curtain"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            w!("mypaper.studio.curtain"),
            w!(""),
            WS_POPUP,
            r.x,
            r.y,
            r.width,
            r.height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Some(hwnd)
    }
}

fn set_alpha(curtains: &[HWND], alpha: f32) {
    let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    for &curtain in curtains {
        // SAFETY: our own layered windows.
        let _ = unsafe { SetLayeredWindowAttributes(curtain, COLORREF(0), alpha, LWA_ALPHA) };
    }
}

/// Eases the curtains from `from` to `to` opacity over `length`, keeping this thread's windows
/// answering (they paint their brush) while it runs. Short, and only at onboarding's edges.
fn fade(curtains: &[HWND], from: f32, to: f32, length: Duration) {
    let start = Instant::now();
    loop {
        let x = (start.elapsed().as_secs_f32() / length.as_secs_f32()).min(1.0);
        let eased = x * x * (3.0 - 2.0 * x);
        set_alpha(curtains, from + (to - from) * eased);
        let mut msg = MSG::default();
        // SAFETY: the standard pump on the thread that owns these windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if x >= 1.0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(12));
    }
}
