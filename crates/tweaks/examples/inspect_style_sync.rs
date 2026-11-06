//! Read-only live probe: `inspect_style_sync [--check]`. Run from the repo root after
//! changing a COPY of config.json, under the desktop lock. Prints resolved expectations,
//! exact registry bytes and the DWM attributes of open Explorer/Settings windows.

use anyhow::{Result, ensure};
use mypaper_config::{Backdrop, Config, Corners};
use mypaper_theme::LoadedTheme;
use mypaper_tweaks::palette;
use mypaper_tweaks::plan::{self, WindowStyle};
use mypaper_tweaks::registry::Registry;
use mypaper_tweaks::undo::{Store, Undo};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible,
};
use windows::core::BOOL;

const VALUES: [(&str, &str); 8] = [
    (r"Software\Microsoft\Windows\DWM", "AccentColor"),
    (r"Software\Microsoft\Windows\DWM", "ColorizationColor"),
    (r"Software\Microsoft\Windows\DWM", "ColorPrevalence"),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
        "AccentColorMenu",
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
        "AccentPalette",
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "SystemUsesLightTheme",
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "ColorPrevalence",
    ),
];

fn main() -> Result<()> {
    let check = std::env::args().any(|arg| arg == "--check");
    let config = Config::load()?;
    let loaded = mypaper_theme::locate(Some(&config.theme))
        .and_then(LoadedTheme::load)
        .or_else(|_| {
            LoadedTheme::load(std::path::absolute(
                std::path::Path::new("themes").join(&config.theme),
            )?)
        })?;
    let palette = palette::read()
        .filter(|p| p.is_for(&loaded.dir))
        .map(|p| p.palette);
    let resolved = mypaper_theme::windows::resolve(
        &config.look,
        Some(&loaded.theme),
        palette,
        mypaper_tweaks::system_prefers_dark(),
    );
    let expected = resolved
        .windows
        .as_ref()
        .map(|s| plan::system_writes(s, palette.as_ref()))
        .unwrap_or_default();
    let mut failures = Vec::new();
    for write in &expected {
        if Registry.read(write.key, write.name)? != Some(write.value.clone()) {
            failures.push(format!("{}\\{}", write.key, write.name));
        }
    }
    let journal = Undo::new(
        Registry,
        palette::local_dir()?.join("undo.json"),
        std::time::Duration::ZERO,
    );
    let journal_mismatches: Vec<_> = VALUES
        .iter()
        .filter(|(key, name)| {
            journal.journaled(key, name)
                != expected.iter().any(|w| w.key == *key && w.name == *name)
        })
        .map(|(key, name)| format!("{key}\\{name}"))
        .collect();
    let registry = VALUES
        .into_iter()
        .map(|(key, name)| {
            Ok(json!({"key": key, "name": name, "value": Registry.read_exact(key, name)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let frame = resolved
        .windows
        .as_ref()
        .map(|w| WindowStyle::resolve(w, palette.as_ref()));
    let mut windows = Vec::<Value>::new();
    // SAFETY: the callback only reads windows and appends to this live Vec on this thread.
    unsafe { EnumWindows(Some(visit), LPARAM((&raw mut windows) as isize))? };
    let attributes = frame.map(expected_attributes).unwrap_or_default();
    let matching = windows
        .iter()
        .filter(|window| {
            attributes
                .iter()
                .filter(|(name, _)| !matches!(name.as_str(), "border" | "caption" | "text"))
                .all(|(name, expected)| window[name].as_u64() == Some(u64::from(*expected)))
        })
        .count();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "style": config.look.style,
            "appearance": config.look.appearance,
            "windows_style": resolved.windows,
            "accent": resolved.accent,
            "dominant": resolved.dominant,
            "registry": registry,
            "registry_mismatches": failures,
            "journal_mismatches": journal_mismatches,
        "expected_window_attributes": attributes.into_iter().map(|(k, v)| (k, json!(v))).collect::<serde_json::Map<_, _>>(),
            "windows": windows,
            "matching_windows": matching,
        "colour_readback": "DWM documents border/caption/text colours as set-only; inspect the captured pixels.",
        }))?
    );
    if check {
        ensure!(
            failures.is_empty(),
            "registry did not match the resolved style: {failures:?}"
        );
        ensure!(
            journal_mismatches.is_empty(),
            "core has not journaled/released the resolved fields yet: {journal_mismatches:?}"
        );
        if frame.is_some() {
            ensure!(
                matching > 0,
                "no Explorer/Settings window matched all managed DWM attributes"
            );
        }
    }
    Ok(())
}

fn expected_attributes(style: WindowStyle) -> Vec<(String, u32)> {
    let mut attrs = Vec::new();
    for (name, color) in [
        ("border", style.border),
        ("caption", style.caption),
        ("text", style.caption_text),
    ] {
        if let Some(color) = color {
            attrs.push((name.into(), mypaper_tweaks::color::colorref(color)));
        }
    }
    if let Some(corner) = style.corners {
        attrs.push((
            "corners".into(),
            match corner {
                Corners::Default => 0,
                Corners::Square => 1,
                Corners::Round => 2,
                Corners::Small => 3,
            },
        ));
    }
    if let Some(backdrop) = style.backdrop {
        attrs.push((
            "backdrop".into(),
            match backdrop {
                Backdrop::Auto => 0,
                Backdrop::None => 1,
                Backdrop::Mica => 2,
                Backdrop::Acrylic => 3,
                Backdrop::Tabbed => 4,
            },
        ));
    }
    if let Some(dark) = style.dark_mode {
        attrs.push(("dark".into(), u32::from(dark)));
    }
    attrs
}

unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: plain window queries; buffers carry their capacities.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let mut class = [0u16; 128];
        let class_len = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..class_len]);
        let mut title = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title) as usize;
        let title = String::from_utf16_lossy(&title[..title_len]);
        if class != "CabinetWClass" && title != "Settings" {
            return true.into();
        }
        let mut result = json!({ "hwnd": hwnd.0 as usize, "class": class, "title": title });
        for (name, attribute) in [("dark", 20), ("corners", 33), ("backdrop", 38)] {
            let mut value = 0u32;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(attribute),
                (&raw mut value).cast(),
                size_of::<u32>() as u32,
            )
            .is_ok()
            {
                result[name] = json!(value);
            }
        }
        (&mut *(data.0 as *mut Vec<Value>)).push(result);
    }
    true.into()
}
