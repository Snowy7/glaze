//! The flyouts' look: the top bar's native style from `bars.json` over the theme's bars
//! appearance (the same layers the bars use), and the glass every popup (flyouts and menus)
//! puts under its body. Blurred styles get engine glass; solid and mono draw their own fill.

use std::sync::Arc;

use glaze_bars::accent::{self, System};
use glaze_bars::config::{self, BarsConfig};
use glaze_bars::style::{Family, Look as BarLook, NativeStyle};
use glaze_config::Config;
use glaze_theme::{Appearance, LoadedTheme};
use glaze_ui::theme::{Knobs, Radii};
use glaze_ui::{GlassMaterial, Rgba, Style, Text, Theme, icons};

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
    pub glass_style: glaze_ui::glass::GlassStyle,
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
    /// An explicit surface radius is shared with menus, instead of the style's inset default.
    pub menu_radius: Option<f32>,
    resolved: Theme,
    pub opening: glaze_ui::transition::Timing,
    pub hover_motion: glaze_ui::transition::Timing,
    pub press_motion: glaze_ui::transition::Timing,
    pub opening_style: glaze_bars::style::MotionStyle,
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
        let desktop = Config::load().unwrap_or_default().look;
        let loaded = Config::load()
            .and_then(|c| glaze_theme::locate(Some(&c.theme)))
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
        crate::bars::look::appearance_layer(&bars, "top", &mut bar_look, &appearance);
        crate::bars::look::popup_font(&bars, "top", &mut bar_look);
        crate::bars::look::glass_layer(
            &bars,
            "top",
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
        let system = accent::read();
        crate::look::settings::bar(&desktop, crate::glass::Family::Menus, &bars, "top", &mut bar_look, &mut appearance, &system, text);
        let mut look = Self::resolve_settings(&bar_look, &appearance, &system, &knobs, &desktop, text);
        let performance = bars.performance.active(glaze_desktop::on_battery());
        look.opening = crate::bars::motion::resolve(
            &bars.top.animation,
            false,
            crate::bars::motion::Leaf::Open,
            system.animations,
            performance,
        );
        look.hover_motion = crate::bars::motion::resolve(
            &bars.top.animation,
            false,
            crate::bars::motion::Leaf::Hover,
            system.animations,
            performance,
        );
        look.press_motion = crate::bars::motion::resolve(
            &bars.top.animation,
            false,
            crate::bars::motion::Leaf::Press,
            system.animations,
            performance,
        );
        look.opening_style = crate::bars::motion::presentation(
            &bars.top.animation,
            false,
            crate::bars::motion::Leaf::Open,
        );
        crate::bars::look::glass_fade(&mut look.glass_style, look.opening, look.opening_style);
        if performance {
            look.glass = None;
            look.resolved.glass = None;
        }
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

    pub fn install(&self) {
        glaze_ui::glass::set_style(self.glass);
        glaze_ui::menu::set_style(Some(self.menu_tokens()));
        glaze_ui::menu::set_interaction_motion(Some((self.hover_motion, self.press_motion)));
        glaze_ui::menu::set_open_motion(Some(glaze_ui::menu::MenuMotion {
            timing: self.opening,
            style: crate::bars::motion::reveal(self.opening_style),
        }));
    }

    /// The shared desktop controls, applied once on reload to both flyouts and their menus.
    pub fn apply_settings(&mut self, desktop: &glaze_config::Look, text: &Text) {
        self.font = crate::look::settings::font(&self.font, |f| text.has_family(f)).into();
        self.resolved.font.family = self.font.clone();
        if let Some(set) = crate::look::settings::icons(desktop, text) {
            if let glaze_ui::icons::IconSet::Font(family) = &set { self.icons = family.clone(); }
            self.resolved.icons = set;
        }
        if let Some(edge) = desktop.shape.edge {
            self.resolved.border = edge.then_some(self.resolved.color.separator);
        }
        if desktop.shape.corners.is_some() {
            self.resolved = self.resolved.clone().with_surface_radius(self.radius);
            self.menu_radius = Some(self.radius);
        }
    }

    /// Runtime tokens, preserving literal family names (including Segoe's installed cuts).
    pub fn resolve_settings(bar: &BarLook, appearance: &Appearance, system: &System, knobs: &Knobs, desktop: &glaze_config::Look, text: &Text) -> Self {
        let mut look = Self::resolve_with(bar, appearance, system, knobs);
        if let Some(name) = bar.font.family.as_deref().or(appearance.font.as_deref()) {
            look.font = crate::look::settings::font(name, |f| text.has_family(f)).into();
        }
        look.apply_settings(desktop, text);
        look
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
        let glass_tint = appearance.tint.unwrap_or(glaze_widgets::look::DEFAULT_TINT);
        // Start with the widgets' material. A bar's surface fill is not a popup tint colour.
        let liquid = crate::glass::liquid_material(glass_tint);
        let glass = glaze_ui::theme::material(kit, liquid.tint)
            .filter(|_| {
                bar.glass.style != Some(glaze_bars::style::GlassKind::None)
                    && appearance.surface != Some(glaze_theme::Surface::Solid)
            })
            .map(|base| {
                let own = if glaze_ui::theme::lensed(kit) {
                    liquid
                } else {
                    base
                };
                let bar_knobs = Knobs {
                    clarity: bar.flyouts.tint.map(|t| 1.0 - t.clamp(0.0, 1.0)).or(bar.glass.clarity),
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
                glass.blur += match &bar.background {
                    Some(glaze_bars::style::Fill::Wallpaper(fill)) => fill.blur,
                    Some(glaze_bars::style::Fill::Blur(fill)) => fill.blur,
                    _ => 0.0,
                };
                if bar.glass.style == Some(glaze_bars::style::GlassKind::Frosted) {
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
        let mut resolved = tokens.theme.clone();
        resolved.glass = glass;
        if appearance.tint.is_some() || appearance.opacity.is_some() || tokens.fill.is_some() {
            resolved.color.surface = fill;
        }
        resolved.radius.surface = radius;
        resolved.radius.card = Radii::concentric(radius, resolved.metrics.pad, 6.0);
        resolved.font.family = tokens.font.family.clone();
        if let Some(weight) = bar.font.weight {
            for role in &mut resolved.font.roles {
                role.weight = weight;
            }
        }
        if let Some(size) = bar.font.size {
            let factor = size / resolved.font.font(glaze_ui::TextRole::Body).size;
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
            glass_style: tokens.glass_style,
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
            menu_radius: None,
            resolved,
            opening: crate::bars::motion::resolve(
                &Default::default(),
                false,
                crate::bars::motion::Leaf::Open,
                system.animations,
                false,
            ),
            hover_motion: crate::bars::motion::resolve(
                &Default::default(),
                false,
                crate::bars::motion::Leaf::Hover,
                system.animations,
                false,
            ),
            press_motion: crate::bars::motion::resolve(
                &Default::default(),
                false,
                crate::bars::motion::Leaf::Press,
                system.animations,
                false,
            ),
            opening_style: glaze_bars::style::MotionStyle::Slide,
        }
    }

    /// The kit's theme for the flyouts (docs/ui.md "The kit"): the bar's style (the same as the
    /// bar's own, see bars/look.rs), the ink the scheme, and the accent carries over. The body's
    /// radius is the look's (the user's `radius`, else the kit's), cards concentric inside it.
    pub fn theme(&self) -> Theme {
        self.resolved.clone()
    }

    /// Menus share the active family, font, colors and material with the panels.
    pub fn menu_tokens(&self) -> glaze_ui::Tokens {
        glaze_ui::Tokens {
            radius: self.menu_radius.unwrap_or_else(|| Radii::concentric(self.radius, 6.0, 8.0)),
            icons: self.icons.clone(),
            icon_set: Some(self.resolved.icons.clone()),
            scale: self.scale,
            shadow: self.shadow,
            glass_style: self.glass_style,
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
    use glaze_bars::style::NativeStyle;

    #[test]
    fn only_the_blurred_style_has_glass() {
        let system = fixed_system();
        let liquid = Look::resolve(&BarLook::default(), &Appearance::default(), &system);
        let glass = liquid.glass.expect("liquid is the default and has glass");
        assert_eq!(glass, crate::glass::liquid_material(glaze_widgets::look::DEFAULT_TINT));
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
    /// floor) and clarity 1 is clear, untinted glass, in every glass style.
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
        let mut bar = BarLook::default();
        bar.flyouts.tint = Some(0.35);
        let resolve = |knobs: &Knobs| Look::resolve_with(&bar, &Appearance::default(), &fixed_system(), knobs).glass.unwrap();
        let own = resolve(&Knobs::default());
        assert_eq!((own.clarity, own.tint.a), (0.65, 0.35));
        let global = resolve(&Knobs { clarity: Some(0.8), ..Default::default() });
        assert!((global.clarity - 0.8).abs() < 1e-6 && (global.tint.a - 0.2).abs() < 1e-6);
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
