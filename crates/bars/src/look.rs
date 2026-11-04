//! How the bars look (docs/bars.md, "Appearance"): the built-in defaults, under the active
//! theme's `bars` skin, under `bars.json` → `appearance`, resolved into plain values for the page,
//! plus the theme's CSS. `accent: auto` takes the wallpaper palette's accent.

use std::path::PathBuf;

use mypaper_theme::{Appearance, Color, LoadedTheme, Paint, Surface, TextTone};
use serde::{Deserialize, Serialize};

use crate::bridge::BarKind;

/// Every appearance field, resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Look {
    pub surface: Surface,
    pub tint: Color,
    pub opacity: f32,
    pub blur: f32,
    pub radius: f32,
    pub border: bool,
    pub text: TextTone,
    pub accent: Color,
    pub font: String,
    pub scale: f32,
    pub shadow: bool,
}

const WHITE: Color = Color([0xff; 3]);

/// The bars' own defaults: dark glass, square top bar, rounded dock.
pub fn defaults(kind: BarKind) -> Appearance {
    Appearance {
        surface: Some(Surface::Glass),
        tint: Some(Color([0x0e; 3])),
        opacity: Some(0.6),
        blur: Some(30.0),
        radius: Some(match kind {
            BarKind::Top => 0.0,
            BarKind::Dock => 16.0,
        }),
        border: Some(true),
        text: Some(TextTone::Light),
        accent: Some(Paint::Color(WHITE)),
        font: Some(r#""Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif"#.into()),
        scale: Some(1.0),
        shadow: Some(false),
        style: None,
    }
}

/// Layers the user's choice over the theme's over the defaults.
pub fn resolve(
    kind: BarKind,
    theme: Option<&Appearance>,
    user: &Appearance,
    palette_accent: Option<Color>,
) -> Look {
    let base = defaults(kind);
    let merged = match theme {
        Some(theme) => user.over(&theme.over(&base)),
        None => user.over(&base),
    };
    Look {
        surface: merged.surface.unwrap_or(Surface::Glass),
        tint: merged.tint.unwrap_or(Color([0x0e; 3])),
        opacity: merged.opacity.unwrap_or(0.6),
        blur: merged.blur.unwrap_or(30.0),
        radius: merged.radius.unwrap_or(0.0),
        border: merged.border.unwrap_or(true),
        text: merged.text.unwrap_or(TextTone::Light),
        accent: match merged.accent {
            Some(Paint::Color(color)) => color,
            Some(Paint::Auto) | None => palette_accent.unwrap_or(WHITE),
        },
        font: merged.font.unwrap_or_default(),
        scale: merged.scale.unwrap_or(1.0),
        shadow: merged.shadow.unwrap_or(false),
    }
}

/// The active theme's `bars` skin, read now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeSkin {
    /// The theme folder, watched for edits.
    pub dir: Option<PathBuf>,
    pub appearance: Option<Appearance>,
    pub css: Option<String>,
}

impl ThemeSkin {
    /// The theme `config.json` names. A theme that fails to load styles nothing (logged).
    pub fn load() -> Self {
        let name = mypaper_config::Config::load()
            .map(|c| c.theme)
            .unwrap_or_default();
        let loaded = mypaper_theme::locate(Some(&name)).and_then(LoadedTheme::load);
        match loaded {
            Ok(loaded) => {
                let skin = loaded.theme.bars.clone().unwrap_or_default();
                let css = skin.css.as_ref().and_then(|css| {
                    let path = loaded.asset(css);
                    std::fs::read_to_string(&path)
                        .inspect_err(|e| log::warn!("theme bars css {}: {e}", path.display()))
                        .ok()
                });
                Self {
                    dir: Some(loaded.dir),
                    appearance: skin.appearance,
                    css,
                }
            }
            Err(e) => {
                log::warn!("no theme look for the bars: {e:#}");
                Self::default()
            }
        }
    }
}

/// The wallpaper palette's accent (`%LOCALAPPDATA%\mypaper\palette.json`), if there is one.
pub fn palette_accent() -> Option<Color> {
    #[derive(Deserialize)]
    struct Palette {
        accent: String,
    }
    let path = mypaper_bars::files::local("palette.json").ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let palette: Palette = serde_json::from_str(&text).ok()?;
    Color::parse(&palette.accent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_resolve_per_bar() {
        let look = resolve(BarKind::Top, None, &Appearance::default(), None);
        assert_eq!(look.surface, Surface::Glass);
        assert_eq!(look.tint, Color([0x0e; 3]));
        assert_eq!((look.opacity, look.blur, look.radius), (0.6, 30.0, 0.0));
        assert_eq!(
            resolve(BarKind::Dock, None, &Appearance::default(), None).radius,
            16.0
        );

        let theme = Appearance {
            surface: Some(Surface::Solid),
            radius: Some(8.0),
            accent: Some(Paint::Auto),
            ..Appearance::default()
        };
        let user = Appearance {
            radius: Some(4.0),
            ..Appearance::default()
        };
        let look = resolve(BarKind::Dock, Some(&theme), &user, Some(Color([1, 2, 3])));
        assert_eq!(look.surface, Surface::Solid, "the theme over the defaults");
        assert_eq!(look.radius, 4.0, "the user over the theme");
        assert_eq!(look.accent, Color([1, 2, 3]), "auto takes the palette");
        let look = resolve(BarKind::Dock, Some(&theme), &user, None);
        assert_eq!(look.accent, WHITE, "auto without a palette is white");
    }

    #[test]
    fn serializes_as_plain_json() {
        let json = serde_json::to_value(resolve(BarKind::Top, None, &Appearance::default(), None))
            .unwrap();
        assert_eq!(json["surface"], "glass");
        assert_eq!(json["tint"], "#0e0e0e");
        assert_eq!(json["accent"], "#ffffff");
        assert_eq!(json["text"], "light");
    }
}
