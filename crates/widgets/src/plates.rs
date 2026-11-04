//! Geometry reported by legacy shells. Current shells draw their own snapshot-backed plates.
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub instance: String,
    pub rect: [f64; 4],
    pub radius: f64,
}

/// Clears an older host's plates when upgrading without restarting the wallpaper engine.
pub fn retire() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW,
    };
    use windows::core::{HSTRING, PCWSTR, w};
    if let Err(e) = mypaper_theme::glass::remove("widgets") {
        log::warn!("retiring widget plates: {e:#}");
    }
    // SAFETY: message-only window lookup and a registered notification, with no pointers.
    unsafe {
        if let Ok(hwnd) = FindWindowExW(
            Some(HWND_MESSAGE),
            None,
            w!("mypaper.wallpaper.control"),
            PCWSTR::null(),
        ) {
            let message = RegisterWindowMessageW(&HSTRING::from(mypaper_theme::glass::MESSAGE));
            let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0));
        }
    }
}
