//! The flyouts' look: the top bar's native style from `bars.json` over the theme's bars
//! appearance (the same layers the bars use), and the glass every popup (flyouts and menus)
//! puts under its body. Blurred styles get engine glass; solid and mono draw their own fill.

use std::sync::Arc;

use mypaper_bars::accent::{self, System};
use mypaper_bars::config::{self, BarsConfig};
use mypaper_bars::style::{Family, Look as BarLook, NativeStyle};
use mypaper_config::Config;
use mypaper_theme::{Appearance, LoadedTheme};
use mypaper_ui::theme::{Knobs, Radii};
use mypaper_ui::{GlassMaterial, Rgba, Style, Text, Theme, icons};

#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub ink: Rgba,
    pub accent: Rgba,
    /// Text and glyphs on an accent fill.
    pub on_accent: Rgba,
    /// The body when there is no glass (solid, mono, or no engine).
    pub fill: Rgba,
    /// The glass under the body, for the blurred styles.
    pub glass: Option<GlassMaterial>,
    /// The kit style: the bar's `look.style`, else its family's.
    pub style: Style,
    /// Corner radius in design pixels.
    pub radius: f32,
    pub font: Arc<str>,
    pub icons: Arc<str>,
    /// How quick-settings modules group (Fluent folds network and volume into it).
    pub family: Family,
    /// `launcher.winKey`: the Windows key opens the launcher (and Win+N, Win+A ours).
    pub win_key: bool,
    /// `calendar.firstDay`, Monday = 1, Sunday = 0; `None` follows the locale.
    pub first_day: Option<u32>,
    /// `audio.switchOutput`: the mixer may change the default output.
    pub switch_output: bool,
    pub scale: f32,
    pub shadow: bool,
    resolved: Theme,
}

impl Look {
    /// Reads `bars.json` and the theme now (the shell calls this when either changes).
    pub fn load(text: &Text) -> Self {
        let bars = config::path()
            .and_then(|p| BarsConfig::load(&p))
            .map(|mut c| {
                c.resolve(Vec::new);
                c
            })
            .inspect_err(|e| log::debug!("flyouts: bars.json: {e:#}"))
            .unwrap_or_default();
        let loaded = Config::load()
            .and_then(|c| mypaper_theme::locate(Some(&c.theme)))
            .and_then(LoadedTheme::load)
            .ok();
        let appearance = crate::bars::look::theme_appearance(loaded.as_ref());
        // The user's material controls for menus and panels, read from `config.json` itself
        // (not their projection into `bars.json`, which may land a moment later).
        let knobs = Config::load()
            .map(|c| crate::glass::knobs(&c.look, crate::glass::Family::Menus))
            .unwrap_or_default();
        // Plugin `panel` style patches (docs/plugins.md, "Styling"), under the user's own fields.
        let (mut bar_look, mut appearance) =
            (bars.top.look.clone(), bars.appearance.over(&appearance));
        crate::bars::look::theme_glass(
            &mut bar_look,
            loaded
                .as_ref()
                .and_then(|t| t.theme.bars.as_ref())
                .and_then(|s| s.glass),
        );
        let colors = crate::bars::plugin_style::patch_look(
            "panel",
            "top",
            bars.raw(),
            &mut bar_look,
            &mut appearance,
        );
        let mut look = Self::resolve_with(&bar_look, &appearance, &accent::read(), &knobs);
        look.patch_colors(&colors);
        look.icons = icons::system_family(text);
        look.win_key = bars.launcher.win_key;
        look.first_day = bars.calendar.first_day.map(|d| match d {
            config::Weekday::Sunday => 0,
            config::Weekday::Monday => 1,
            config::Weekday::Saturday => 6,
        });
        look.switch_output = bars.audio.switch_output;
        look
    }

    /// A plugin `panel` patch's colour roles: `label` (the ink), `accent`, and `surface` or
    /// `background` (the body without glass). Other roles and tones are ignored here.
    pub fn patch_colors(&mut self, colors: &[(String, crate::bars::plugin_style::TokenValue)]) {
        use crate::bars::plugin_style::{TokenValue, hex};
        for (name, value) in colors {
            let TokenValue::Color(text) = value else {
                continue;
            };
            let Some(color) = hex(text) else { continue };
            match name.as_str() {
                "label" => {
                    self.ink = color;
                    self.resolved.color.label = color;
                }
                "accent" => {
                    self.accent = color;
                    self.resolved.color.accent = color;
                }
                "surface" | "background" => {
                    self.fill = color;
                    self.resolved.color.surface = color;
                }
                _ => {}
            }
        }
    }

    /// The look of `bar` over `appearance`, as the bars resolve theirs (bars/look.rs).
    pub fn resolve(bar: &BarLook, appearance: &Appearance, system: &System) -> Self {
        Self::resolve_with(bar, appearance, system, &Knobs::default())
    }

    /// [`Look::resolve`] under the user's material controls (`config.json` › `look.menus`,
    /// else `look`), which win over the bar's own `look.glass`.
    pub fn resolve_with(
        bar: &BarLook,
        appearance: &Appearance,
        system: &System,
        knobs: &Knobs,
    ) -> Self {
        let tokens = crate::bars::look::Tokens::resolve(bar, appearance, system);
        let style = bar.native_style();
        let ink = tokens.theme.color.label;
        let tint = tokens.theme.color.surface.with_alpha(1.0);
        let accent = tokens.theme.color.accent;
        let on_accent = tokens.theme.color.on_accent;
        let flyout_tint = bar.flyouts.tint.unwrap_or(0.62).clamp(0.0, 1.0);
        let kit = crate::bars::look::style(style);
        let fill = tokens.fill.unwrap_or_else(|| {
            tint.with_alpha(bar.tint.or(appearance.opacity).unwrap_or(
                if style == NativeStyle::Solid {
                    1.0
                } else {
                    0.92
                },
            ))
        });
        // The style's material (`theme::material`) under the user's controls by the kit's one
        // rule (`GlassMaterial::tuned`), as the bars and widgets. Flyouts sit over app windows,
        // so their own default (no control set) is more tinted and more frosted than the bars':
        // whatever is behind a flyout must not read through its text. Blur in DIPs.
        let glass = mypaper_ui::theme::material(kit, tint)
            .filter(|_| {
                bar.glass.style != Some(mypaper_bars::style::GlassKind::None)
                    && appearance.surface != Some(mypaper_theme::Surface::Solid)
            })
            .map(|base| {
                let own = GlassMaterial {
                    clarity: if mypaper_ui::theme::lensed(kit) {
                        1.0 - flyout_tint
                    } else {
                        (1.0 - flyout_tint).min(base.clarity)
                    },
                    blur: base.blur.max(28.0),
                    ..base
                };
                let bar_knobs = Knobs {
                    clarity: bar.glass.clarity,
                    blur: bar.glass.blur.or(appearance.blur),
                    refraction: bar.glass.refraction,
                    highlight: bar.glass.highlight,
                };
                let mut glass = own.tuned(kit, &knobs.over(bar_knobs));
                glass.tint.a = knobs
                    .clarity
                    .map(|c| 1.0 - c)
                    .or(bar.flyouts.tint)
                    .or(bar.glass.tint)
                    .unwrap_or(glass.tint.a);
                let intensity = bar.glass.intensity.unwrap_or(1.0);
                glass.refraction *= intensity;
                glass.highlight *= intensity;
                glass.blur *= intensity;
                if bar.glass.style == Some(mypaper_bars::style::GlassKind::Frosted) {
                    glass.refraction = 0.0;
                    glass.highlight = 0.0;
                }
                // `flyouts.blur: false` is this bar's own say for its flyouts.
                if bar.flyouts.blur == Some(false) {
                    GlassMaterial { blur: 0.0, ..glass }
                } else {
                    glass
                }
            });
        let radius = bar
            .radius
            .or(appearance.radius)
            .unwrap_or(tokens.theme.radius.surface);
        let radius = if kit == Style::Fluent {
            radius.min(8.0)
        } else {
            radius
        };
        let mut resolved = tokens.theme.clone();
        resolved.glass = glass;
        resolved.color.surface = fill;
        resolved.radius.surface = radius;
        resolved.radius.card = Radii::concentric(radius, resolved.metrics.pad, 6.0);
        resolved.font.family = tokens.font.family.clone();
        if let Some(weight) = bar.font.weight {
            for role in &mut resolved.font.roles {
                role.weight = weight;
            }
        }
        if let Some(size) = bar.font.size {
            let factor = size / resolved.font.font(mypaper_ui::TextRole::Body).size;
            for role in &mut resolved.font.roles {
                role.size *= factor;
                role.line *= factor;
            }
        }
        resolved.border = (tokens.border && glass.is_none()).then_some(resolved.color.separator);
        if !tokens.shadow {
            resolved.elevation.color = Rgba::TRANSPARENT;
        }
        Self {
            ink,
            accent,
            on_accent,
            fill,
            glass,
            style: kit,
            radius,
            font: tokens.font.family,
            icons: icons::SYSTEM_FAMILY.into(),
            family: bar.family.unwrap_or_default(),
            win_key: true,
            first_day: None,
            switch_output: false,
            scale: tokens.scale,
            shadow: tokens.shadow,
            resolved,
        }
    }

    /// The kit's theme for the flyouts (docs/ui.md "The kit"): the bar's style (the same as the
    /// bar's own, see bars/look.rs), the ink the scheme, and the accent carries over. The body's
    /// radius is the look's (the user's `radius`, else the kit's), cards concentric inside it.
    pub fn theme(&self) -> Theme {
        self.resolved.clone()
    }

    /// Menus share the active family, font, colors and material with the panels.
    pub fn menu_tokens(&self) -> mypaper_ui::Tokens {
        mypaper_ui::Tokens {
            radius: Radii::concentric(self.radius, 6.0, 8.0),
            icons: self.icons.clone(),
            scale: self.scale,
            shadow: self.shadow,
            ..self.resolved.tokens()
        }
    }
}

impl Default for Look {
    /// The built-in look on a fixed dark system (for tests and golden images).
    fn default() -> Self {
        Self::resolve(&BarLook::default(), &Appearance::default(), &fixed_system())
    }
}

/// A dark Windows with a blue accent, whatever this machine says.
pub fn fixed_system() -> System {
    System {
        accent: std::array::from_fn(|_| "#3b82f6".to_owned()),
        scheme: accent::Scheme::Dark,
        animations: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_bars::style::NativeStyle;

    #[test]
    fn only_the_blurred_style_has_glass() {
        let system = fixed_system();
        let liquid = Look::resolve(&BarLook::default(), &Appearance::default(), &system);
        let glass = liquid.glass.expect("liquid is the default and has glass");
        assert!((0.0..1.0).contains(&glass.clarity), "tinted enough to read");
        for style in [NativeStyle::Solid, NativeStyle::Mono] {
            let bar = BarLook {
                style: Some(style),
                ..Default::default()
            };
            let look = Look::resolve(&bar, &Appearance::default(), &system);
            assert_eq!(look.glass, None, "{style:?} draws its own fill");
            assert!(look.fill.a >= 0.85, "{style:?}: a readable fill");
        }
        // One UI and Fluent: flat frosted glass, and the flyout's theme is the bar's style.
        for (style, kit) in [
            (NativeStyle::OneUi, Style::OneUi),
            (NativeStyle::Fluent, Style::Fluent),
        ] {
            let bar = BarLook {
                style: Some(style),
                ..Default::default()
            };
            let look = Look::resolve(&bar, &Appearance::default(), &system);
            let glass = look.glass.expect("frosted glass");
            assert_eq!((glass.refraction, glass.highlight), (0.0, 0.0), "{style:?}");
            assert_eq!(look.theme().style, kit);
        }
        let solid = BarLook {
            style: Some(NativeStyle::Solid),
            ..Default::default()
        };
        assert_eq!(
            Look::resolve(&solid, &Appearance::default(), &system)
                .fill
                .a,
            1.0
        );
    }

    /// The user's controls reach the flyouts as they reach the bars: blur 0 is no frost (no
    /// 28 px floor) and clarity 1 is clear, untinted glass, in every glass style.
    #[test]
    fn the_users_controls_reach_the_flyouts() {
        for style in [NativeStyle::Liquid, NativeStyle::OneUi, NativeStyle::Fluent] {
            let bar = BarLook {
                style: Some(style),
                ..Default::default()
            };
            let knobs = Knobs {
                clarity: Some(1.0),
                blur: Some(0.0),
                ..Default::default()
            };
            let look = Look::resolve_with(&bar, &Appearance::default(), &fixed_system(), &knobs);
            let glass = look.glass.expect("glass");
            assert_eq!(
                (glass.clarity, glass.blur, glass.tint.a),
                (1.0, 0.0, 0.0),
                "{style:?}"
            );
        }
    }
}

#[cfg(test)]
mod plugin_style_tests {
    use super::*;
    use crate::bars::plugin_style::TokenValue;

    #[test]
    fn a_panel_patch_sets_ink_accent_and_fill() {
        let mut look = Look::resolve(&BarLook::default(), &Appearance::default(), &fixed_system());
        let before = look.ink;
        let token = |name: &str, value: TokenValue| (name.to_owned(), value);
        look.patch_colors(&[
            token("accent", TokenValue::Color("#ff0000".into())),
            token("surface", TokenValue::Color("#102030".into())),
            token("label", TokenValue::Tone("accent".into())),
        ]);
        assert_eq!(look.accent, Rgba::rgb8([255, 0, 0], 1.0));
        assert_eq!(look.fill, Rgba::rgb8([0x10, 0x20, 0x30], 1.0));
        assert_eq!(look.ink, before, "tones are left to the bars' resolver");
    }
}
