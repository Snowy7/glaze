//! The bars' tokens: one kit [`Theme`] per bar (every colour, type role, radius and icon set it
//! draws with), plus what only a bar has: its config's corner radius, an explicit background
//! colour, the engine glass under it, and its font overrides. CSS-era decoration is accepted on
//! load and ignored with one warning.
//!
//! **Styles.** `look.style` is the kit style one to one (`liquid`, `oneui`, `fluent`, `solid`,
//! `mono`); a look without one takes its family's (Fluent → Fluent, Material → One UI, else
//! Liquid). The style alone picks the material (`mypaper_ui::theme::material`): Liquid Glass,
//! frosted One UI, Fluent acrylic, or no glass for Solid and Mono. `look.glass` tunes it.
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
    /// `look.background: none`: no body at all, the items straight on the wallpaper.
    pub bare: bool,
}

/// The kit style of a bar's style (see the module docs).
pub fn style(native: NativeStyle) -> Style {
    match native {
        NativeStyle::Liquid => Style::Liquid,
        NativeStyle::OneUi => Style::OneUi,
        NativeStyle::Fluent => Style::Fluent,
        NativeStyle::Solid => Style::Solid,
        NativeStyle::Mono => Style::Mono,
    }
}

/// The engine material of `look` in `style`, tinted `tint`: the style's base
/// (`theme::material`) with the look's knobs over it. Only Liquid takes refraction and a
/// highlight; One UI and Fluent stay flat and frosted however the knobs are set (a clear
/// acrylic is just glass), so their clarity is capped.
pub fn material(look: &Look, style: Style, tint: Rgba, opacity: Option<f32>) -> Option<Material> {
    let tinted = look.glass.tint.or(look.tint);
    let base = mypaper_ui::theme::material(
        style,
        tint.with_alpha(tinted.or(opacity).unwrap_or(0.12)),
    )?;
    let lensed = mypaper_ui::theme::lensed(style);
    // The bar is small glass: as clear as macOS's menu bar unless the look tints it.
    let clarity = look.glass.clarity.unwrap_or(match tinted {
        Some(t) => 1.0 - t,
        None if lensed => 0.88,
        None => base.clarity,
    });
    Some(if lensed {
        Material {
            clarity,
            blur: look.glass.blur.unwrap_or(base.blur),
            refraction: look.glass.refraction.unwrap_or(base.refraction),
            highlight: look.glass.highlight.unwrap_or(base.highlight),
            tint: base.tint,
        }
    } else {
        Material {
            clarity: clarity.min(0.6),
            blur: look.glass.blur.unwrap_or(base.blur).max(base.blur * 0.66),
            refraction: 0.0,
            highlight: 0.0,
            tint: base.tint,
        }
    })
}

impl Tokens {
    pub fn resolve(look: &Look, appearance: &Appearance, system: &accent::System) -> Self {
        let native = look.native_style();
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
        let kit = style(native);
        let mut theme = Theme::new(kit, scheme, accent);
        if look.glyphs == Some(Glyphs::Fluent) {
            theme.icons = IconSet::Font(mypaper_ui::icons::SYSTEM_FAMILY.into());
        }
        // A solid background colour only where the style draws its own body: a glass style
        // keeps its glass (a design's tint colour is no reason to drop it).
        let fill = match &look.background {
            Some(Fill::Solid { color }) if theme.glass.is_none() => {
                Some(resolve_color(color, theme.color.label, theme.color.accent))
            }
            _ => None,
        };
        // The glass's tint is the theme's (appearance) tint.
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [23, 24, 28] } else { [243, 243, 247] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let material = material(look, kit, tint, appearance.opacity);
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
        // The style shapes the corners too: Fluent's are small (4-8 px), One UI's rounder.
        let radius = look.radius.or(appearance.radius).unwrap_or(12.0);
        let radius = match kit {
            Style::Fluent => radius.min(8.0),
            Style::OneUi => radius * 1.25,
            _ => radius,
        };
        let bare = matches!(look.background, Some(Fill::None));
        Self {
            family,
            font,
            fill,
            radius,
            material: material.filter(|_| !bare),
            bare,
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
        let seen: std::collections::BTreeSet<_> =
            NativeStyle::ALL.into_iter().map(|s| style(s).name()).collect();
        assert_eq!(seen.len(), Style::ALL.len(), "{seen:?}");
    }

    /// The style alone decides the material: only Liquid bends and lights its rim; One UI and
    /// Fluent are flat frosted glass even when a design asks for refraction; Solid and Mono have
    /// none, and Mono stays near-opaque.
    #[test]
    fn styles_pick_distinct_materials() {
        let system = crate::flyouts::look::fixed_system();
        let design = Look {
            glass: mypaper_bars::style::GlassStyle {
                refraction: Some(0.6),
                highlight: Some(0.5),
                ..Default::default()
            },
            ..Default::default()
        };
        let tokens = |native| {
            Tokens::resolve(
                &Look {
                    style: Some(native),
                    ..design.clone()
                },
                &Appearance::default(),
                &system,
            )
        };
        let liquid = tokens(NativeStyle::Liquid).material.expect("liquid glass");
        assert!(liquid.refraction > 0.0 && liquid.highlight > 0.0);
        for frosted in [NativeStyle::OneUi, NativeStyle::Fluent] {
            let m = tokens(frosted).material.expect("frosted glass");
            assert_eq!((m.refraction, m.highlight), (0.0, 0.0), "{frosted:?}");
            assert!(m.clarity <= 0.6 && m.blur > liquid.blur, "{frosted:?}: {m:?}");
        }
        for flat in [NativeStyle::Solid, NativeStyle::Mono] {
            let t = tokens(flat);
            assert!(t.material.is_none(), "{flat:?}");
            assert!(t.theme.color.surface.a >= 0.9, "{flat:?}: near-opaque");
        }
        assert!(tokens(NativeStyle::Fluent).radius <= 8.0);
        // A look without a style takes its family's.
        let fluent = Look {
            family: Some(Family::Fluent),
            ..Default::default()
        };
        assert_eq!(fluent.native_style(), NativeStyle::Fluent);
    }

    #[test]
    fn the_system_face_keeps_the_variable_font_and_others_replace_it() {
        let macos = r#""SF Pro Text", "SF Pro", "Segoe UI Variable Text", "Segoe UI", system-ui"#;
        assert_eq!(custom_family(macos), None);
        assert_eq!(custom_family("Inter, sans-serif").as_deref(), Some("Inter"));
    }
}
