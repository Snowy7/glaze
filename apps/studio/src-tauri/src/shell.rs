//! Window chrome: the Mica backdrop, when this Windows version has it.

use serde::Serialize;
use tauri::WebviewWindow;
use tauri::window::{Effect, EffectsBuilder};
use winreg::RegKey;
use winreg::enums::HKEY_LOCAL_MACHINE;

/// First Windows 11 build with the public Mica backdrop API.
const MICA_MIN_BUILD: u32 = 22621;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ShellInfo {
    /// Mica is active, so the page should let the backdrop show through.
    pub mica: bool,
}

fn windows_build() -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber"))
        .ok()?
        .parse()
        .ok()
}

/// The window is created transparent but the page stays opaque unless this
/// succeeds, so Windows 10 never shows a see-through window.
pub fn apply_backdrop(window: &WebviewWindow) -> ShellInfo {
    let supported = windows_build().is_some_and(|build| build >= MICA_MIN_BUILD);
    let mica = supported && window.set_effects(EffectsBuilder::new().effect(Effect::Mica).build()).is_ok();
    ShellInfo { mica }
}
