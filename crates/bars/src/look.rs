//! How the bars look (docs/bars.md, "Appearance"): the built-in defaults, under the active
//! theme's `bars` skin, under `bars.json` → `appearance`, resolved into plain values for the page,
//! plus the theme's CSS. `accent: auto` takes the wallpaper palette's accent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mypaper_bars::icon_pack::{self, IconPack};
use mypaper_bars::vars::VarDecl;
use mypaper_theme::{Appearance, Color, LoadedTheme, Paint, Surface, TextTone};
use serde::{Deserialize, Serialize};

use crate::bridge::BarKind;
use crate::web::PAGE_HOST;

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
    /// The appearance's design style by name (`liquid` makes the bars' glass liquid by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
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
        style: merged
            .style
            .and_then(|style| serde_json::to_value(style).ok())
            .and_then(|name| name.as_str().map(str::to_owned)),
    }
}

/// The active theme's `bars` skin, read now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeSkin {
    /// The theme folder, watched for edits.
    pub dir: Option<PathBuf>,
    pub appearance: Option<Appearance>,
    pub css: Option<String>,
    /// The variables its CSS declares (`@mypaper-vars`).
    pub vars: BTreeMap<String, VarDecl>,
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
                let vars = css
                    .as_deref()
                    .map(mypaper_bars::vars::declared)
                    .transpose()
                    .unwrap_or_else(|e| {
                        log::warn!("theme bars css variables: {e:#}");
                        None
                    })
                    .unwrap_or_default();
                Self {
                    dir: Some(loaded.dir),
                    appearance: skin.appearance,
                    css,
                    vars,
                }
            }
            Err(e) => {
                log::warn!("no theme look for the bars: {e:#}");
                Self::default()
            }
        }
    }
}

/// The virtual host `%APPDATA%\mypaper\icons\` (user icon packs) is mapped to.
pub const PACKS_HOST: &str = "packs.bars.mypaper";

/// An icon pack as the page gets it: the pack plus where its files are served.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pack {
    #[serde(flatten)]
    pub pack: IconPack,
    /// Ends with `/`.
    pub base: String,
}

/// `%APPDATA%\mypaper\icons\`.
pub fn packs_dir() -> Option<PathBuf> {
    mypaper_config::dir().ok().map(|dir| dir.join("icons"))
}

/// Loads pack `name`: the user's folder first, then the ones bundled with the page.
pub fn load_pack(name: &str, page: Option<&Path>) -> Option<Pack> {
    let user = packs_dir();
    let bundled = page.map(|page| page.join("icons"));
    let (dir, host) = [(user, PACKS_HOST), (bundled, PAGE_HOST)]
        .into_iter()
        .find_map(|(root, host)| {
            let dir = icon_pack::find(name, &[root?])?;
            Some((dir, host))
        })
        .or_else(|| {
            log::warn!("icon pack {name:?} not found");
            None
        })?;
    let base = if host == PAGE_HOST {
        format!("https://{host}/icons/{name}/")
    } else {
        format!("https://{host}/{name}/")
    };
    IconPack::load(&dir)
        .inspect_err(|e| log::warn!("icon pack {name:?}: {e:#}"))
        .ok()
        .map(|pack| Pack { pack, base })
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
