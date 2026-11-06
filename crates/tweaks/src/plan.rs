//! Turns a theme's `windows` section (plus the wallpaper palette) into
//! concrete values: what to set on windows, and what to write to the
//! registry. Pure, so `"auto"` resolution is tested.

use mypaper_theme::{Backdrop, Color, Corners, Paint, WindowsStyle};

use crate::color::{accent_abgr, accent_palette, colorization_argb};
use crate::palette::Palette;
use crate::undo::{Value, Write};

const DWM_KEY: &str = r"Software\Microsoft\Windows\DWM";
const ACCENT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
pub(crate) const PERSONALIZE_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
/// 0 = apps use the dark theme.
pub(crate) const APPS_THEME: &str = "AppsUseLightTheme";
const SYSTEM_THEME: &str = "SystemUsesLightTheme";

/// A style with every `"auto"` resolved. `None` means "leave it alone"
/// (including an `"auto"` whose palette has not arrived yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowStyle {
    pub border: Option<Color>,
    pub caption: Option<Color>,
    pub caption_text: Option<Color>,
    pub corners: Option<Corners>,
    pub backdrop: Option<Backdrop>,
    pub dark_mode: Option<bool>,
}

impl WindowStyle {
    pub fn resolve(style: &WindowsStyle, palette: Option<&Palette>) -> Self {
        Self {
            border: resolve(style.border, palette),
            caption: style.caption,
            caption_text: style.caption_text,
            corners: style.corners,
            backdrop: style.backdrop,
            dark_mode: style.dark_mode,
        }
    }
}

/// `"auto"` is the wallpaper's accent color; a fixed color is itself.
fn resolve(paint: Option<Paint>, palette: Option<&Palette>) -> Option<Color> {
    match paint? {
        Paint::Color(color) => Some(color),
        Paint::Auto => palette.map(|p| p.accent),
    }
}

/// The per-user registry values that give the system its accent and theme.
pub fn system_writes(style: &WindowsStyle, palette: Option<&Palette>) -> Vec<Write> {
    let mut writes = Vec::new();
    if let Some(accent) = resolve(style.accent, palette) {
        let abgr = Value::Dword(accent_abgr(accent));
        writes.extend([
            Write {
                key: DWM_KEY,
                name: "AccentColor",
                value: abgr.clone(),
            },
            Write {
                key: DWM_KEY,
                name: "ColorizationColor",
                value: Value::Dword(colorization_argb(accent)),
            },
            Write {
                key: ACCENT_KEY,
                name: "AccentColorMenu",
                value: abgr,
            },
            Write {
                key: ACCENT_KEY,
                name: "AccentPalette",
                value: Value::Binary(accent_palette(accent).to_vec()),
            },
        ]);
    }
    if let Some(dark) = style.dark_mode {
        let light = Value::Dword(u32::from(!dark));
        for name in [APPS_THEME, SYSTEM_THEME] {
            writes.push(Write {
                key: PERSONALIZE_KEY,
                name,
                value: light.clone(),
            });
        }
    }
    for (key, enabled) in [
        (PERSONALIZE_KEY, style.accent_on_taskbar),
        (DWM_KEY, style.accent_on_title_bars),
    ] {
        if let Some(enabled) = enabled {
            writes.push(Write {
                key,
                name: "ColorPrevalence",
                value: Value::Dword(u32::from(enabled)),
            });
        }
    }
    writes
}

/// Does the user currently have apps set to the dark theme?
pub fn system_prefers_dark() -> bool {
    use crate::registry::Registry;
    use crate::undo::Store;
    matches!(
        Registry.read(PERSONALIZE_KEY, APPS_THEME),
        Ok(Some(Value::Dword(0)))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO_STYLE: &str = r##"{ "accent": "auto", "border": "auto", "caption": "#101010" }"##;

    fn style(json: &str) -> WindowsStyle {
        serde_json::from_str(json).unwrap()
    }

    fn palette() -> Palette {
        Palette {
            dominant: Color([1, 2, 3]),
            accent: Color([0x33, 0xaa, 0xff]),
        }
    }

    #[test]
    fn auto_follows_the_palette_accent_and_waits_without_one() {
        let style = style(AUTO_STYLE);
        let with = WindowStyle::resolve(&style, Some(&palette()));
        assert_eq!(with.border, Some(Color([0x33, 0xaa, 0xff])));
        assert_eq!(with.caption, Some(Color([0x10, 0x10, 0x10])));

        let without = WindowStyle::resolve(&style, None);
        assert_eq!(without.border, None);
        assert_eq!(without.caption, with.caption);
        assert!(system_writes(&style, None).is_empty());
    }

    #[test]
    fn a_fixed_color_ignores_the_palette() {
        let style = style(r##"{ "accent": "#ff0000" }"##);
        let writes = system_writes(&style, Some(&palette()));
        let accent = writes.iter().find(|w| w.name == "AccentColorMenu").unwrap();
        assert_eq!(accent.value, Value::Dword(0xFF00_00FF));
    }

    #[test]
    fn dark_mode_writes_both_light_theme_flags_inverted() {
        for (dark, expected) in [(true, 0), (false, 1)] {
            let writes = system_writes(&style(&format!(r#"{{ "dark_mode": {dark} }}"#)), None);
            let names: Vec<_> = writes.iter().map(|w| w.name).collect();
            assert_eq!(names, ["AppsUseLightTheme", "SystemUsesLightTheme"]);
            assert!(writes.iter().all(|w| w.value == Value::Dword(expected)));
        }
    }

    #[test]
    fn accent_visibility_writes_distinct_keys_even_without_a_palette() {
        for (taskbar, titles) in [(true, false), (false, true)] {
            let style = WindowsStyle {
                accent_on_taskbar: Some(taskbar),
                accent_on_title_bars: Some(titles),
                ..Default::default()
            };
            let writes = system_writes(&style, None);
            assert_eq!(writes.len(), 2);
            assert_eq!(
                (writes[0].key, writes[0].name),
                (PERSONALIZE_KEY, "ColorPrevalence")
            );
            assert_eq!(writes[0].value, Value::Dword(u32::from(taskbar)));
            assert_eq!(
                (writes[1].key, writes[1].name),
                (DWM_KEY, "ColorPrevalence")
            );
            assert_eq!(writes[1].value, Value::Dword(u32::from(titles)));
        }
        assert!(system_writes(&WindowsStyle::default(), None).is_empty());
    }

    #[test]
    fn five_style_transitions_and_palette_updates_restore_the_first_registry_originals() {
        use crate::undo::tests::{Memory, Scratch};
        use crate::undo::{Store, Undo};
        use mypaper_config::look::{Scheme, Style};
        use std::time::Duration;

        let scratch = Scratch::new("style-sync-roundtrip");
        let mut store = Memory::default();
        // Include an absent value and a non-DWORD original: undo owns exact originals,
        // irrespective of the type Windows normally uses for the desired setting.
        store
            .write(DWM_KEY, "ColorPrevalence", &Value::Binary(vec![3, 9]))
            .unwrap();
        store
            .write(PERSONALIZE_KEY, APPS_THEME, &Value::Dword(1))
            .unwrap();
        let mut undo = Undo::new(store.clone(), scratch.journal(), Duration::ZERO);
        let mut touched = Vec::new();
        for style in Style::ALL {
            for appearance in [Scheme::Dark, Scheme::Light] {
                let look = mypaper_config::Look {
                    style: Some(style),
                    appearance: Some(appearance),
                    ..Default::default()
                };
                let raw = mypaper_theme::windows::resolve(&look, None, Some(palette()), false)
                    .windows
                    .unwrap();
                let writes = system_writes(&raw, Some(&palette()));
                assert_eq!(writes.len(), 8);
                undo.apply(&writes).unwrap();
                assert!(!undo.apply(&writes).unwrap(), "identical events do no work");
                for w in &writes {
                    assert_eq!(store.read(w.key, w.name).unwrap(), Some(w.value.clone()));
                }
                touched = writes;
            }
        }
        let changed = Palette {
            accent: Color([190, 20, 100]),
            ..palette()
        };
        undo.apply(&system_writes(&style(AUTO_STYLE), Some(&changed)))
            .unwrap();
        // A fresh controller models recovery after a hard kill.
        let mut recovered = Undo::new(store.clone(), scratch.journal(), Duration::ZERO);
        recovered.restore(|| {}).unwrap();
        assert!(!recovered.pending());
        for w in &touched {
            let expected = match (w.key, w.name) {
                (DWM_KEY, "ColorPrevalence") => Some(Value::Binary(vec![3, 9])),
                (PERSONALIZE_KEY, APPS_THEME) => Some(Value::Dword(1)),
                _ => None,
            };
            assert_eq!(store.read(w.key, w.name).unwrap(), expected);
        }
    }
}
