//! The bars' tokens: one kit [`Theme`] per bar (every colour, type role, radius and icon set it
//! draws with), plus what only a bar has: its config's corner radius, an explicit background
//! colour, the engine glass under it, and its font overrides. CSS-era decoration is accepted on
//! load and ignored with one warning.
//!
//! **Styles.** `look.style` is the kit style one to one (`liquid`, `oneui`, `fluent`, `solid`,
//! `mono`); a look without one takes its family's (Fluent → Fluent, Material → One UI, else
//! Liquid). The style alone picks the material (`mypaper_ui::theme::material`): Liquid Glass,
//! frosted One UI, Fluent acrylic, or no glass for Solid and Mono. `look.glass` tunes it by the
//! kit's one rule (`GlassMaterial::tuned`), the same as widgets and flyouts.
//!
//! `look.glyphs: fluent` draws the module icons from Segoe Fluent Icons in any style.
use crate::glass::Material;
use mypaper_bars::{
    accent,
    style::{ColorRef, Family, Fill, GlassKind, Glyphs, Look, NativeStyle, Scheme},
};
use mypaper_theme::{Appearance, LoadedTheme, TextTone};
use mypaper_ui::theme::Knobs;
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
    /// Engine glass under the body (blur in DIPs); `None` draws the theme's surface.
    pub material: Option<Material>,
    /// `look.transparent` (or `background: none`): no body at all, the items straight on the
    /// wallpaper with a soft shadow.
    pub bare: bool,
    pub scale: f32,
    pub text_shadow: bool,
    pub border: bool,
    pub shadow: bool,
    /// Windows' seven accent shades, resolved once with the look.
    pub system_colors: [Rgba; 7],
    /// Wallpaper backgrounds stay in the desktop plate; live blur always uses an overlay.
    pub wallpaper_only: bool,
    pub live_blur: bool,
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

/// The engine material of `look` in `style`, tinted `tint` (blur in DIPs): the style's base
/// (`theme::material`) at the bar's own default, under the look's controls by the kit's one rule
/// (`GlassMaterial::tuned`, the same as widgets and flyouts). `look.glass` holds the controls
/// (Studio's Look sliders write it); `glass.tint`, or the older `look.tint`, says how tinted the
/// glass is (1 - clarity). A bar is small glass: Liquid's default is as clear as macOS's menu bar.
pub fn material(look: &Look, style: Style, tint: Rgba, opacity: Option<f32>) -> Option<Material> {
    if look.glass.style == Some(GlassKind::None) {
        return None;
    }
    let base = mypaper_ui::theme::material(style, tint.with_alpha(opacity.unwrap_or(0.12)))?;
    let base = if mypaper_ui::theme::lensed(style) {
        Material {
            clarity: 0.88,
            ..base
        }
    } else {
        base
    };
    let tinted = look.glass.tint.or(look.tint);
    let m = base.tuned(
        style,
        &Knobs {
            clarity: look.glass.clarity.or(tinted.map(|t| 1.0 - t)),
            blur: look.glass.blur,
            refraction: look.glass.refraction,
            highlight: look.glass.highlight,
        },
    );
    // A tint set beside a clarity is its own strength (the two are independent in bars.json).
    let mut m = match tinted {
        Some(t) => Material {
            tint: m.tint.with_alpha(t),
            ..m
        },
        None => m,
    };
    let intensity = look.glass.intensity.unwrap_or(1.0);
    m.refraction *= intensity;
    m.highlight *= intensity;
    m.blur *= intensity;
    if look.glass.style == Some(GlassKind::Frosted) {
        m.refraction = 0.0;
        m.highlight = 0.0;
    }
    Some(m)
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
        // The glass's tint is the theme's (appearance) tint.
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [23, 24, 28] } else { [243, 243, 247] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let mut glass_look = look.clone();
        glass_look.glass.blur = glass_look.glass.blur.or(appearance.blur);
        let material = material(&glass_look, kit, tint, appearance.opacity);
        let system_colors =
            std::array::from_fn(|i| hex(&system.accent[i]).unwrap_or(theme.color.accent));
        let fill = match &look.background {
            Some(Fill::Solid { color }) => Some(color_with_system(
                color,
                theme.color.label,
                theme.color.accent,
                &system_colors,
            )),
            _ if material.is_none()
                || appearance.surface == Some(mypaper_theme::Surface::Solid) =>
            {
                Some(
                    tint.with_alpha(
                        look.tint
                            .or(appearance.opacity)
                            .unwrap_or(theme.color.surface.a),
                    ),
                )
            }
            _ => None,
        };
        theme.color.surface = tint.with_alpha(appearance.opacity.unwrap_or(theme.color.surface.a));
        let body = theme.font.style(TextRole::Body, 1.0);
        let size = look.font.size.unwrap_or(body.size);
        // A family other than the system face replaces the variable font; Segoe UI in any
        // spelling (or SF Pro, which Windows doesn't have) keeps it.
        let named = look
            .font
            .family
            .as_deref()
            .or(appearance.font.as_deref())
            .and_then(custom_family);
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
        let bare = look.transparent.unwrap_or_else(|| match look.background {
            Some(Fill::None) => true,
            Some(_) => false,
            None => appearance.surface == Some(mypaper_theme::Surface::None),
        });
        let material = material.filter(|_| !bare && fill.is_none());
        theme.glass = material;
        theme.font.family = font.family.clone();
        let border = appearance.border.unwrap_or(theme.border.is_some());
        theme.border = (border && material.is_none()).then_some(theme.color.separator);
        Self {
            family,
            font,
            fill,
            radius,
            material,
            bare,
            theme,
            scale: appearance.scale.unwrap_or(1.0),
            text_shadow: appearance.shadow.unwrap_or(bare),
            border,
            shadow: appearance.shadow.unwrap_or(true),
            system_colors,
            wallpaper_only: matches!(look.background, Some(Fill::Wallpaper(_))),
            live_blur: matches!(look.background, Some(Fill::Blur(_))),
        }
    }

    /// The menus this bar opens (dock and tray), drawn with its theme.
    pub fn menu(&self, text: &mypaper_ui::Text) -> mypaper_ui::Tokens {
        mypaper_ui::Tokens {
            icons: mypaper_ui::icons::system_family(text),
            font: self.font.clone(),
            scale: self.scale,
            shadow: self.shadow,
            border: self.border && self.material.is_none(),
            glass: self.material,
            ..self.theme.tokens()
        }
    }

    pub fn color(&self, color: &ColorRef) -> Rgba {
        color_with_system(
            color,
            self.theme.color.label,
            self.theme.color.accent,
            &self.system_colors,
        )
    }
}

fn color_with_system(color: &ColorRef, ink: Rgba, accent: Rgba, system: &[Rgba; 7]) -> Rgba {
    let index = match color.as_str() {
        "system-accent" => Some(0),
        "system-accent-light-1" => Some(1),
        "system-accent-light-2" => Some(2),
        "system-accent-light-3" => Some(3),
        "system-accent-dark-1" => Some(4),
        "system-accent-dark-2" => Some(5),
        "system-accent-dark-3" => Some(6),
        _ => None,
    };
    index.map_or_else(|| resolve_color(color, ink, accent), |i| system[i])
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
    let mut appearance = bars.appearance.over(&theme_appearance(loaded.as_ref()));
    // The menus follow the top bar's plugin patches, then the `menu` surface's own colours.
    let mut look = bars.top.look.clone();
    theme_glass(
        &mut look,
        loaded
            .as_ref()
            .and_then(|t| t.theme.bars.as_ref())
            .and_then(|s| s.glass),
    );
    let mut colors =
        super::plugin_style::patch_look("top-bar", "top", bars.raw(), &mut look, &mut appearance);
    colors.extend(super::plugin_style::tokens("menu"));
    let mut tokens = Tokens::resolve(&look, &appearance, &accent::read());
    super::plugin_style::colors(&colors, bars.raw(), &mut tokens.theme.color);
    tokens.menu(text)
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

/// A theme's partial glass layer, below fields supplied by the bar. Rebuilt on every config
/// change, so removing a theme field restores the style default rather than keeping old values.
pub fn theme_glass(look: &mut Look, glass: Option<mypaper_theme::Liquid>) {
    if let Some(glass) = glass {
        look.glass.intensity = look.glass.intensity.or(glass.intensity);
        look.glass.refraction = look.glass.refraction.or(glass.refraction);
        look.glass.highlight = look.glass.highlight.or(glass.highlight);
        look.glass.tint = look.glass.tint.or(glass.tint);
        look.glass.blur = look.glass.blur.or(glass.blur);
    }
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
                    "system-ui"
                        | "sans-serif"
                        | "serif"
                        | "monospace"
                        | "ui-sans-serif"
                        | "-apple-system"
                )
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bar_style_maps_to_a_kit_style() {
        let seen: std::collections::BTreeSet<_> = NativeStyle::ALL
            .into_iter()
            .map(|s| style(s).name())
            .collect();
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
            assert!(
                m.clarity <= 0.6 && m.blur > liquid.blur,
                "{frosted:?}: {m:?}"
            );
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

    /// The Look sliders reach the bar in every glass style, to both ends: no clamps, blur 0 is
    /// no frost and clarity 1 no tint.
    #[test]
    fn the_looks_controls_reach_the_bar_unclamped() {
        let system = crate::flyouts::look::fixed_system();
        for native in [NativeStyle::Liquid, NativeStyle::OneUi, NativeStyle::Fluent] {
            let at = |clarity: f32, blur: f32| {
                let look = Look {
                    style: Some(native),
                    glass: mypaper_bars::style::GlassStyle {
                        tint: Some(1.0 - clarity),
                        blur: Some(blur),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                Tokens::resolve(&look, &Appearance::default(), &system)
                    .material
                    .unwrap()
            };
            let clear = at(1.0, 0.0);
            assert_eq!(
                (clear.clarity, clear.blur, clear.tint.a),
                (1.0, 0.0, 0.0),
                "{native:?}"
            );
            let tinted = at(0.0, 32.0);
            assert_eq!((tinted.clarity, tinted.blur), (0.0, 32.0), "{native:?}");
            assert!(tinted.tint.a > 0.2, "{native:?}: {tinted:?}");
        }
    }

    #[test]
    fn the_system_face_keeps_the_variable_font_and_others_replace_it() {
        let macos = r#""SF Pro Text", "SF Pro", "Segoe UI Variable Text", "Segoe UI", system-ui"#;
        assert_eq!(custom_family(macos), None);
        assert_eq!(custom_family("Inter, sans-serif").as_deref(), Some("Inter"));
    }

    #[test]
    fn appearance_fields_and_explicit_backgrounds_reach_the_bar() {
        let appearance: Appearance = serde_json::from_value(serde_json::json!({
            "surface": "solid", "tint": "#123456", "opacity": 0.7,
            "font": "Inter", "scale": 1.25, "border": true, "shadow": false,
            "radius": 9, "text": "dark"
        }))
        .unwrap();
        let tokens = Tokens::resolve(
            &Look::default(),
            &appearance,
            &crate::flyouts::look::fixed_system(),
        );
        assert_eq!(tokens.material, None);
        assert_eq!(tokens.fill, Some(Rgba::rgb8([0x12, 0x34, 0x56], 0.7)));
        assert_eq!(tokens.scale, 1.25);
        assert_eq!(&*tokens.font.family, "Inter");
        assert!(tokens.theme.border.is_some());
        assert!(!tokens.text_shadow);
        let bare = Tokens::resolve(
            &Look::default(),
            &Appearance {
                surface: Some(mypaper_theme::Surface::None),
                ..Default::default()
            },
            &crate::flyouts::look::fixed_system(),
        );
        assert!(bare.bare && bare.material.is_none());
        let look: Look = serde_json::from_value(
            serde_json::json!({ "background": {"type":"solid", "color":"#fedcba"} }),
        )
        .unwrap();
        let solid = Tokens::resolve(
            &look,
            &Appearance::default(),
            &crate::flyouts::look::fixed_system(),
        );
        assert_eq!(solid.fill, Some(Rgba::rgb8([0xfe, 0xdc, 0xba], 1.0)));
        assert!(solid.material.is_none());
    }

    #[test]
    fn every_windows_palette_token_uses_its_own_shade() {
        let mut system = crate::flyouts::look::fixed_system();
        system.accent = std::array::from_fn(|i| format!("#{i:02x}0000"));
        let tokens = Tokens::resolve(&Look::default(), &Appearance::default(), &system);
        for (i, name) in mypaper_bars::style::COLOR_TOKENS[3..].iter().enumerate() {
            assert_eq!(
                tokens.color(&ColorRef::parse(name).unwrap()),
                Rgba::rgb8([i as u8, 0, 0], 1.0),
                "{name}"
            );
        }
    }

    #[test]
    fn removing_a_theme_glass_layer_restores_defaults() {
        let base = Look::default();
        let mut themed = base.clone();
        theme_glass(
            &mut themed,
            Some(mypaper_theme::Liquid {
                blur: Some(17.0),
                ..Default::default()
            }),
        );
        let system = crate::flyouts::look::fixed_system();
        assert_eq!(
            Tokens::resolve(&themed, &Appearance::default(), &system)
                .material
                .unwrap()
                .blur,
            17.0
        );
        let mut reset = base.clone();
        theme_glass(&mut reset, None);
        assert_eq!(
            Tokens::resolve(&reset, &Appearance::default(), &system).material,
            Tokens::resolve(&base, &Appearance::default(), &system).material
        );
    }
}
