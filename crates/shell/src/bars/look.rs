//! The bars' tokens: one kit [`Theme`] per bar (every colour, type role, radius and icon set it
//! draws with), plus what only a bar has: its config's corner radius, an explicit background
//! colour, the engine glass under it, and its font overrides. CSS-era decoration is accepted on
//! load and ignored with one warning.
//!
//! **Styles.** `look.style` picks the kit style; liquid follows the family the bar's menus and
//! flyouts are drawn after:
//!
//! | `look.style` | family | kit style |
//! |---|---|---|
//! | `liquid` | `macos`, `mono` | Liquid |
//! | `liquid` | `fluent` | Fluent |
//! | `liquid` | `material` | One UI |
//! | `solid` | any | Solid |
//! | `mono` | any | Mono |
//!
//! `look.glyphs: fluent` draws the module icons from Segoe Fluent Icons in any style.
use crate::glass::Material;
use mypaper_bars::{
    accent,
    style::{ColorRef, Family, Fill, Glyphs, Look, NativeStyle, Scheme},
};
use mypaper_theme::{Appearance, LoadedTheme, TextTone};
use mypaper_ui::{Rgba, Style, TextRole, TextStyle, Theme, icons::IconSet};

#[derive(Clone)]
pub struct Tokens {
    /// The OS family of the menus and flyouts the bar opens (Fluent folds network and volume
    /// into quick settings).
    pub family: Family,
    pub theme: Theme,
    /// The bar's text in design px: the theme's body role under `look.font`.
    pub font: TextStyle,
    /// `look.background` as a solid colour; `None` takes the theme's surface (or glass).
    pub fill: Option<Rgba>,
    /// Corner radius of the bar, its islands and the dock plate, design px.
    pub radius: f32,
    /// Engine glass under the body (liquid); `None` draws the theme's surface.
    pub material: Option<Material>,
}

/// The kit style of a bar's look (see the module docs).
pub fn style(native: NativeStyle, family: Family) -> Style {
    match native {
        NativeStyle::Solid => Style::Solid,
        NativeStyle::Mono => Style::Mono,
        NativeStyle::Liquid => match family {
            Family::Fluent => Style::Fluent,
            Family::Material => Style::OneUi,
            Family::Macos | Family::Mono => Style::Liquid,
        },
    }
}

impl Tokens {
    pub fn resolve(look: &Look, appearance: &Appearance, system: &accent::System) -> Self {
        let native = look.style.unwrap_or_default();
        let family = look.family.unwrap_or_default();
        let dark = match look.scheme {
            Some(Scheme::Dark) => true,
            Some(Scheme::Light) => false,
            Some(Scheme::Auto) => system.scheme == accent::Scheme::Dark,
            None => appearance.text.is_none_or(|t| t == TextTone::Light),
        };
        let accent = match appearance.accent {
            Some(mypaper_theme::Paint::Color(c)) => Rgba::rgb8(c.0, 1.0),
            _ => hex(&system.accent[0]).unwrap_or(mypaper_ui::theme::SYSTEM_BLUE),
        };
        let scheme = if dark {
            mypaper_ui::Scheme::Dark
        } else {
            mypaper_ui::Scheme::Light
        };
        let mut theme = Theme::new(style(native, family), scheme, accent);
        if look.glyphs == Some(Glyphs::Fluent) {
            theme.icons = IconSet::Font(mypaper_ui::icons::SYSTEM_FAMILY.into());
        }
        let fill = match &look.background {
            Some(Fill::Solid { color }) => {
                Some(resolve_color(color, theme.color.label, theme.color.accent))
            }
            _ => None,
        };
        // The glass's tint is the theme's (appearance) tint, light: the bar is small glass, as
        // clear as macOS's menu bar.
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [23, 24, 28] } else { [243, 243, 247] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let material = (native == NativeStyle::Liquid && fill.is_none_or(|f| f.a < 0.999))
            .then_some(Material {
                clarity: look
                    .glass
                    .clarity
                    .unwrap_or_else(|| 1.0 - look.glass.tint.or(look.tint).unwrap_or(0.12)),
                blur: look.glass.blur.unwrap_or(12.0),
                refraction: look.glass.refraction.unwrap_or(0.6),
                highlight: look.glass.highlight.unwrap_or(0.75),
                tint: tint.with_alpha(
                    look.glass
                        .tint
                        .or(look.tint)
                        .or(appearance.opacity)
                        .unwrap_or(0.12),
                ),
            });
        let body = theme.font.style(TextRole::Body, 1.0);
        let size = look.font.size.unwrap_or(body.size);
        // A family other than the system face replaces the variable font; Segoe UI in any
        // spelling (or SF Pro, which Windows doesn't have) keeps it.
        let named = look.font.family.as_deref().and_then(custom_family);
        let font = TextStyle {
            family: named.map_or(body.family, Into::into),
            size,
            weight: look.font.weight.unwrap_or(body.weight),
            tracking: body.tracking,
            optical: size * 0.75,
        };
        Self {
            family,
            font,
            fill,
            radius: look.radius.or(appearance.radius).unwrap_or(12.0),
            material,
            theme,
        }
    }

    /// The menus this bar opens (dock and tray), drawn with its theme.
    pub fn menu(&self, text: &mypaper_ui::Text) -> mypaper_ui::Tokens {
        mypaper_ui::Tokens {
            icons: mypaper_ui::icons::system_family(text),
            ..self.theme.tokens()
        }
    }
}

/// The menu tokens of the top bar's look from `bars.json` and the theme now: the desktop and
/// tray menus look like the bar's own.
pub fn menu_tokens(text: &mypaper_ui::Text) -> mypaper_ui::Tokens {
    let bars = mypaper_bars::config::path()
        .and_then(|p| mypaper_bars::BarsConfig::load(&p))
        .map(|mut c| {
            c.resolve(Vec::new);
            c
        })
        .inspect_err(|e| log::debug!("menus: bars.json: {e:#}"))
        .unwrap_or_default();
    let loaded = mypaper_config::Config::load()
        .and_then(|c| mypaper_theme::locate(Some(&c.theme)))
        .and_then(LoadedTheme::load)
        .ok();
    let appearance = theme_appearance(loaded.as_ref());
    Tokens::resolve(
        &bars.top.look,
        &bars.appearance.over(&appearance),
        &accent::read(),
    )
    .menu(text)
}

/// The theme's `bars` appearance, its `auto` accent taken from the wallpaper's palette.
pub fn theme_appearance(loaded: Option<&LoadedTheme>) -> Appearance {
    let mut appearance = loaded
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|s| s.appearance.as_ref())
        .cloned()
        .unwrap_or_default();
    if let Some(t) = loaded
        && matches!(appearance.accent, Some(mypaper_theme::Paint::Auto))
        && let Some(snapshot) = mypaper_tweaks::palette::read().filter(|p| p.is_for(&t.dir))
    {
        appearance.accent = Some(mypaper_theme::Paint::Color(snapshot.palette.accent));
    }
    appearance
}

pub fn resolve_color(color: &ColorRef, ink: Rgba, accent: Rgba) -> Rgba {
    match color.as_str() {
        "text" => ink,
        "accent" | "system-accent" => accent,
        "transparent" => Rgba::TRANSPARENT,
        s => hex(s).unwrap_or(ink),
    }
}

pub fn hex(value: &str) -> Option<Rgba> {
    let s = value.strip_prefix('#')?;
    let full = if s.len() <= 4 {
        s.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        s.into()
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    let (rgb, a) = if full.len() == 8 {
        (v >> 8, (v & 255) as f32 / 255.0)
    } else {
        (v, 1.0)
    };
    Some(Rgba::rgb8(
        [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8],
        a,
    ))
}

pub fn warn_dropped(value: &serde_json::Value) {
    fn dropped(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
                (matches!(k.as_str(), "css" | "noise" | "glow")
                    && !v.is_null()
                    && !v.as_object().is_some_and(|m| m.is_empty())
                    && v.as_f64() != Some(0.0))
                    || (k == "type" && matches!(v.as_str(), Some("gradient" | "image")))
                    || dropped(v)
            }),
            serde_json::Value::Array(a) => a.iter().any(dropped),
            serde_json::Value::String(s) => matches!(
                s.as_str(),
                "neon" | "neon-glass" | "bevel" | "scoop" | "notch"
            ),
            _ => false,
        }
    }
    if dropped(value) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            log::warn!(
                "native bars ignore CSS, gradient/image fills, noise, glow, neon and shaped corners"
            )
        });
    }
}

/// The first family of a CSS list that isn't the system face (Segoe UI in any spelling, SF Pro,
/// or a generic), if any.
fn custom_family(list: &str) -> Option<String> {
    list.split(',')
        .map(|s| s.trim().trim_matches(['\"', '\'']).trim())
        .find(|f| {
            !f.is_empty()
                && !f.starts_with("Segoe UI")
                && !f.starts_with("SF Pro")
                && !matches!(
                    f.to_ascii_lowercase().as_str(),
                    "system-ui" | "sans-serif" | "serif" | "monospace" | "ui-sans-serif" | "-apple-system"
                )
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bar_style_maps_to_a_kit_style() {
        let mut seen = std::collections::BTreeSet::new();
        for native in [NativeStyle::Liquid, NativeStyle::Solid, NativeStyle::Mono] {
            for family in [Family::Mono, Family::Fluent, Family::Macos, Family::Material] {
                seen.insert(style(native, family).name());
            }
        }
        assert_eq!(seen.len(), Style::ALL.len(), "{seen:?}");
    }

    #[test]
    fn the_system_face_keeps_the_variable_font_and_others_replace_it() {
        let macos = r#""SF Pro Text", "SF Pro", "Segoe UI Variable Text", "Segoe UI", system-ui"#;
        assert_eq!(custom_family(macos), None);
        assert_eq!(custom_family("Inter, sans-serif").as_deref(), Some("Inter"));
    }
}
