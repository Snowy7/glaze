//! Window chrome: the title bar and frame take the sidebar's gray (a Windows
//! 11 feature; older builds ignore the calls), so the frame and the app read
//! as one surface. These grays mirror the `--sidebar`, `--line` and `--ink-2`
//! tokens in `src/index.css`; the page itself is styled only from those tokens.

use std::ffi::c_void;

use tauri::{Theme, WebviewWindow, WindowEvent};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWINDOWATTRIBUTE,
    DwmSetWindowAttribute,
};

struct Chrome {
    caption: u32,
    text: u32,
    border: u32,
}

const DARK: Chrome = Chrome {
    caption: 0x14_14_14,
    text: 0xd4_d4_d4,
    border: 0x2e_2e_2e,
};
const LIGHT: Chrome = Chrome {
    caption: 0xe8_e8_e8,
    text: 0x2c_2c_2c,
    border: 0xdc_dc_dc,
};

/// `0xRRGGBB` as the `0x00BBGGRR` a `COLORREF` wants.
fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16))
}

fn set(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, rgb: u32) {
    let value = colorref(rgb);
    // SAFETY: `value` is a COLORREF-sized value that lives through the call.
    // Failure (Windows 10) just leaves the default chrome.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            std::ptr::from_ref(&value).cast::<c_void>(),
            size_of::<COLORREF>() as u32,
        )
    };
}

fn apply(window: &WebviewWindow, theme: Theme) {
    let Ok(handle) = window.hwnd() else { return };
    let hwnd = HWND(handle.0);
    let chrome = if theme == Theme::Light { LIGHT } else { DARK };
    set(hwnd, DWMWA_CAPTION_COLOR, chrome.caption);
    set(hwnd, DWMWA_TEXT_COLOR, chrome.text);
    set(hwnd, DWMWA_BORDER_COLOR, chrome.border);
}

/// Colors the frame now and again whenever the system switches light/dark.
pub fn match_chrome(window: &WebviewWindow) {
    apply(window, window.theme().unwrap_or(Theme::Dark));
    let window_for_events = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::ThemeChanged(theme) = event {
            apply(&window_for_events, *theme);
        }
    });
}
