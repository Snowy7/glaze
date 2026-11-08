//! Hears about work-area and display changes.
//!
//! App bars (a top bar, a dock) shrink a monitor's work area, and displays come, go and resize.
//! Windows announces both with broadcast messages (`WM_SETTINGCHANGE` with `SPI_SETWORKAREA`,
//! `WM_DISPLAYCHANGE`), which only top-level windows receive: not our message-only control
//! window and not the child windows of the desktop. So this is a hidden top-level window that
//! turns those broadcasts into one thread message, [`WM_DISPLAY`], for the main loop.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostThreadMessageW, RegisterClassExW,
    SPI_ICONHORIZONTALSPACING, SPI_ICONVERTICALSPACING, SPI_SETICONMETRICS, SPI_SETWORKAREA,
    SYSTEM_PARAMETERS_INFO_ACTION, WM_APP, WM_DISPLAYCHANGE, WM_SETTINGCHANGE, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// Posted to the thread that created the [`DisplayWatcher`] when the work area or the
/// displays changed.
pub const WM_DISPLAY: u32 = WM_APP + 11;

const CLASS: PCWSTR = w!("glaze.widgets.display");

/// Does this window message mean monitors, their work areas or the icon spacing (the grid's
/// cell size) changed?
fn is_layout_change(msg: u32, wparam: usize) -> bool {
    const SETTINGS: [SYSTEM_PARAMETERS_INFO_ACTION; 4] = [
        SPI_SETWORKAREA,
        SPI_SETICONMETRICS,
        SPI_ICONHORIZONTALSPACING,
        SPI_ICONVERTICALSPACING,
    ];
    msg == WM_DISPLAYCHANGE
        || (msg == WM_SETTINGCHANGE && SETTINGS.iter().any(|spi| spi.0 as usize == wparam))
}

/// The hidden window; destroyed on drop. Create it on the thread that pumps messages.
pub struct DisplayWatcher {
    hwnd: HWND,
}

impl DisplayWatcher {
    pub fn new() -> Result<Self> {
        // SAFETY: plain Win32 registration with a 'static window procedure, then a window
        // that is never shown.
        let hwnd = unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            // A second registration (a restarted watcher) fails harmlessly: the class exists.
            RegisterClassExW(&class);
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("Glaze display watcher"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .context("creating the display watcher window")?
        };
        Ok(Self { hwnd })
    }
}

impl Drop for DisplayWatcher {
    fn drop(&mut self) {
        // SAFETY: our window, destroyed on its creating thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if is_layout_change(msg, wparam.0) {
        // SAFETY: posting to our own thread; failure means the loop is gone.
        let _ =
            unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_DISPLAY, WPARAM(0), LPARAM(0)) };
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_work_area_and_display_changes_count() {
        assert!(is_layout_change(WM_DISPLAYCHANGE, 0));
        assert!(is_layout_change(
            WM_SETTINGCHANGE,
            SPI_SETWORKAREA.0 as usize
        ));
        assert!(is_layout_change(
            WM_SETTINGCHANGE,
            SPI_SETICONMETRICS.0 as usize
        ));
        // Other settings (colors, wallpaper, ...) arrive as WM_SETTINGCHANGE too.
        assert!(!is_layout_change(WM_SETTINGCHANGE, 0));
        assert!(!is_layout_change(WM_SETTINGCHANGE, 0x14)); // SPI_SETDESKWALLPAPER
        assert!(!is_layout_change(WM_APP, SPI_SETWORKAREA.0 as usize));
    }
}
