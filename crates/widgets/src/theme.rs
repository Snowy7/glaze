//! `theme`: the active theme's name and, when Studio has extracted one, the
//! wallpaper palette (`%LOCALAPPDATA%\mypaper\palette.json`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Palette>,
}

/// `#rrggbb` colors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub dominant: String,
    pub accent: String,
}

/// Reads the config, the theme's `theme.json` and the palette file now.
/// Anything missing degrades: no palette, or the configured name as is.
pub fn current() -> ThemeInfo {
    let configured = mypaper_config::Config::load()
        .map(|c| c.theme)
        .unwrap_or_default();
    let name = mypaper_theme::locate(Some(&configured))
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join(mypaper_theme::MANIFEST)).ok())
        .and_then(|text| theme_name(&text))
        .unwrap_or(configured);
    let palette = crate::files::local("palette.json")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| parse_palette(&text));
    ThemeInfo { name, palette }
}

fn theme_name(theme_json: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    serde_json::from_str::<Named>(theme_json)
        .ok()
        .map(|t| t.name)
}

/// Lenient: other fields are ignored, but both colors must be `#rrggbb`.
fn parse_palette(json: &str) -> Option<Palette> {
    let palette: Palette = serde_json::from_str(json).ok()?;
    let ok = |c: &str| {
        c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit())
    };
    (ok(&palette.dominant) && ok(&palette.accent)).then_some(palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_need_two_hex_colors() {
        let palette =
            parse_palette(r##"{ "dominant": "#102030", "accent": "#AABBCC", "extra": [1] }"##);
        assert_eq!(palette.map(|p| p.accent), Some("#AABBCC".into()));
        assert_eq!(
            parse_palette(r##"{ "dominant": "red", "accent": "#aabbcc" }"##),
            None
        );
        assert_eq!(parse_palette(r##"{ "dominant": "#102030" }"##), None);
    }

    #[test]
    fn theme_names_come_from_theme_json() {
        assert_eq!(
            theme_name(r#"{ "name": "Aurora", "wallpaper": {} }"#),
            Some("Aurora".into())
        );
        assert_eq!(theme_name("{}"), None);
    }
}
