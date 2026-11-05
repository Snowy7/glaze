//! A select's options as a native popup menu under the control (keyboard, screen readers and
//! edge flipping come with it). It returns the chosen option, or `None` when dismissed.

use mypaper_ui::Rect;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_CHECKED, MF_STRING, MF_UNCHECKED,
    SetForegroundWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN, TrackPopupMenuEx,
};
use windows::core::HSTRING;

/// Shows `options` (label, checked) under `rect` (client pixels of `hwnd`).
pub fn pick(hwnd: HWND, rect: Rect, options: &[(String, bool)]) -> Option<usize> {
    // SAFETY: a menu created, shown modally and destroyed here; our own window.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (i, (label, checked)) in options.iter().enumerate() {
            let flags = MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED };
            let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(label.as_str()));
        }
        let mut at = POINT {
            x: rect.left as i32,
            y: rect.bottom as i32 + 2,
        };
        let _ = ClientToScreen(hwnd, &mut at);
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN).0, at.x, at.y, hwnd, None);
        let _ = DestroyMenu(menu);
        (chosen.0 > 0).then(|| chosen.0 as usize - 1)
    }
}
