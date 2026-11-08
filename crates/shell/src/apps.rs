//! The rest of Glaze, as the desktop menu's "Change wallpaper" and "Edit widgets" reach it.
//! Core's tray does the same for its own menu (`crates/core/src/tray.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW,
};
use windows::core::{PCWSTR, w};

const STUDIO_EXE: &str = "glaze-studio.exe";

/// Starts Studio (it focuses itself if it is already running).
pub fn open_studio() {
    let studio = std::env::current_exe()
        .ok()
        .and_then(|exe| find_studio(&exe));
    match studio {
        Some(path) => {
            if let Err(e) = Command::new(&path).spawn() {
                log::error!("could not start {}: {e}", path.display());
            }
        }
        None => log::warn!("{STUDIO_EXE} not found"),
    }
}

/// Studio (`crates/studio`), built beside this exe like the other components.
fn find_studio(exe: &Path) -> Option<PathBuf> {
    Some(exe.with_file_name(STUDIO_EXE)).filter(|path| path.is_file())
}

/// Toggles the widget host's edit mode: its registered `glaze.widgets.edit` message
/// (wParam 2 = toggle) to its message-only window (docs/widgets.md).
pub fn toggle_widget_edit_mode() {
    // SAFETY: plain lookups and a post; a missing host is just logged.
    unsafe {
        let message = RegisterWindowMessageW(w!("glaze.widgets.edit"));
        match FindWindowExW(
            Some(HWND_MESSAGE),
            None,
            w!("glaze.widgets.host"),
            PCWSTR::null(),
        ) {
            Ok(host) if message != 0 => {
                let _ = PostMessageW(Some(host), message, WPARAM(2), LPARAM(0));
            }
            _ => log::warn!("widget host is not running; cannot toggle edit mode"),
        }
    }
}
