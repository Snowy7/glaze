//! The bars' tokens: one kit [`Theme`] per bar (every colour, type role, radius and icon set it
//! draws with), plus what only a bar has: its config's corner radius, an explicit background
//! colour, the engine glass under it, and its font overrides. CSS-era decoration is accepted on
//! load and ignored with one warning.
//!
//! **Styles.** `look.style` is the kit style one to one (`liquid`, `oneui`, `fluent`, `solid`,
//! `mono`); a look without one takes its family's (Fluent → Fluent, Material → One UI, else
//! Liquid). The style alone picks the material (`glaze_ui::theme::material`): Liquid Glass,
//! frosted One UI, Fluent acrylic, or no glass for Solid and Mono. `look.glass` tunes it by the
//! kit's one rule (`GlassMaterial::tuned`), the same as widgets and flyouts.
//!
//! `look.glyphs: fluent` draws the module icons from Segoe Fluent Icons in any style.
use crate::glass::Material;
use glaze_bars::{
    accent,
    style::{ColorRef, Family, Fill, GlassKind, Glyphs, Look, NativeStyle, Scheme},
};
use glaze_theme::{Appearance, LoadedTheme, TextTone};
use glaze_ui::theme::Knobs;
use glaze_ui::{Rgba, Style, TextRole, TextStyle, Theme, icons::IconSet};

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
    /// An explicit corner setting overrides the strip layout's square fallback.
    pub explicit_radius: bool,
    /// Engine glass under the body (blur in DIPs); `None` draws the theme's surface.
    pub material: Option<Material>,
    /// Optics resolved once with the look, copied to both plates and overlays.
    pub glass_style: glaze_ui::glass::GlassStyle,
    pub plate_style: glaze_glass_ipc::Style,
    /// Dock command menus resolve alongside the bar, never while publishing or opening.
    pub popup: Option<glaze_ui::Tokens>,
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
    pub styles: super::styling::Styles,
    /// Only items whose hover/press has been animated; no timers or state allocation at rest.
    pub interaction: std::collections::HashMap<u32, (f32, f32)>,
    colors: std::collections::HashMap<String, Rgba>,
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
    let base = glaze_ui::theme::material(style, tint.with_alpha(opacity.unwrap_or(0.12)))?;
    let base = if glaze_ui::theme::lensed(style) {
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
    m.blur += match &look.background {
        Some(Fill::Wallpaper(fill)) => fill.blur,
        Some(Fill::Blur(fill)) => fill.blur,
        _ => 0.0,
    };
    if look.glass.style == Some(GlassKind::Frosted) {
        m.refraction = 0.0;
        m.highlight = 0.0;
    }
    Some(m)
}

impl Tokens {
    /// Production resolution under config.json's Look; previews may keep using `resolve`.
    pub fn resolve_settings(look: &Look, appearance: &Appearance, system: &accent::System, desktop: &glaze_config::Look, text: &glaze_ui::Text) -> Self {
        let mut tokens = Self::resolve(look, appearance, system);
        if let Some(name) = look.font.family.as_deref().or(appearance.font.as_deref()) {
            tokens.font.family = crate::look::settings::font(name, |f| text.has_family(f)).into();
            tokens.theme.font.family = tokens.font.family.clone();
        }
        if let Some(set) = crate::look::settings::icons(desktop, text) {
            tokens.theme.icons = set;
        }
        if let Some(edge) = desktop.shape.edge {
            tokens.border = edge;
            tokens.theme.border = edge.then_some(tokens.theme.color.separator);
            tokens.styles = super::styling::Styles::resolve(look, &tokens.theme, |c| {
                color_with_system(c, &tokens.theme, &tokens.system_colors)
            });
        }
        tokens.theme = tokens.theme.with_surface_radius(tokens.radius);
        tokens.explicit_radius = desktop.shape.corners.is_some();
        tokens
    }

    /// Re-evaluate the label roles against the actual material on this monitor.
    pub fn adapt_ink(&mut self, look: &Look, rect: Option<glaze_ui::Rect>) {
        if self.bare { return; }
        let under = crate::glass::tone::backdrop(rect, self.theme.scheme == glaze_ui::Scheme::Dark);
        let surface = self.material.map_or_else(
            || glaze_ui::theme::over(self.fill.unwrap_or(self.theme.color.surface), under),
            |m| crate::glass::tone::face(&m, under, rect.map_or(40.0, |r| r.width().min(r.height()))),
        );
        crate::glass::tone::apply(&mut self.theme, surface);
        self.styles = super::styling::Styles::resolve(look, &self.theme, |c| {
            color_with_system(c, &self.theme, &self.system_colors)
        });
    }

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
            Some(glaze_theme::Paint::Color(c)) => Rgba::rgb8(c.0, 1.0),
            _ => hex(&system.accent[0]).unwrap_or(glaze_ui::theme::SYSTEM_BLUE),
        };
        let scheme = if dark {
            glaze_ui::Scheme::Dark
        } else {
            glaze_ui::Scheme::Light
        };
        let kit = style(native);
        let mut theme = Theme::new(kit, scheme, accent);
        if look.glyphs == Some(Glyphs::Fluent) {
            theme.icons = IconSet::Font(glaze_ui::icons::SYSTEM_FAMILY.into());
        }
        // A tint colour tunes glass; an explicit solid background replaces its material.
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [23, 24, 28] } else { [243, 243, 247] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let mut glass_look = look.clone();
        glass_look.glass.blur = glass_look.glass.blur.or(appearance.blur);
        let strength = appearance.opacity.or(appearance.tint.map(|_| 0.22));
        let material = material(&glass_look, kit, tint, strength);
        let system_colors =
            std::array::from_fn(|i| hex(&system.accent[i]).unwrap_or(theme.color.accent));
        let fill = match &look.background {
            Some(Fill::Solid { color }) => Some(color_with_system(color, &theme, &system_colors)),
            _ if appearance.surface == Some(glaze_theme::Surface::Solid)
                || (material.is_none()
                    && (appearance.tint.is_some()
                        || appearance.opacity.is_some()
                        || look.tint.is_some())) =>
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
        if appearance.tint.is_some() || appearance.opacity.is_some() {
            theme.color.surface =
                tint.with_alpha(appearance.opacity.unwrap_or(theme.color.surface.a));
        }
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
        let radius = look.radius.or(appearance.radius).unwrap_or(match kit {
            Style::Fluent => 8.0,
            Style::OneUi => 15.0,
            _ => 12.0,
        });
        let bare = look.transparent.unwrap_or_else(|| match look.background {
            Some(Fill::None) => true,
            Some(_) => false,
            None => appearance.surface == Some(glaze_theme::Surface::None),
        });
        let material = material.filter(|_| !bare && fill.is_none());
        theme.glass = material;
        theme.font.family = font.family.clone();
        let border = appearance.border.unwrap_or(theme.border.is_some());
        theme.border = (border && material.is_none()).then_some(theme.color.separator);
        let styles = super::styling::Styles::resolve(look, &theme, |c| {
            color_with_system(c, &theme, &system_colors)
        });
        let mut colors = std::collections::HashMap::new();
        for name in glaze_bars::style::COLOR_TOKENS.iter().copied().chain([
            "var(--fill-1)",
            "var(--fill-2)",
            "var(--fill-3)",
            "var(--edge)",
            "var(--t0)",
            "var(--t1)",
            "var(--t2)",
            "var(--t3)",
            "var(--accent)",
        ]) {
            if let Some(color) = ColorRef::parse(name) {
                colors.insert(
                    name.to_owned(),
                    color_with_system(&color, &theme, &system_colors),
                );
            }
        }
        cache_colors(
            &serde_json::to_value(look).unwrap_or_default(),
            &mut colors,
            &theme,
            &system_colors,
        );
        let glass_style = glass_style(look);
        Self {
            family,
            font,
            fill,
            radius,
            explicit_radius: false,
            material,
            glass_style,
            plate_style: plate_style(glass_style),
            popup: None,
            bare,
            theme,
            scale: appearance.scale.unwrap_or(1.0),
            text_shadow: appearance.shadow.unwrap_or(bare),
            border,
            shadow: appearance.shadow.unwrap_or(true),
            system_colors,
            wallpaper_only: matches!(look.background, Some(Fill::Wallpaper(_))),
            live_blur: matches!(look.background, Some(Fill::Blur(_))),
            styles,
            interaction: Default::default(),
            colors,
        }
    }

    /// The menus this bar opens (dock and tray), drawn with its theme.
    pub fn menu(&self, text: &glaze_ui::Text) -> glaze_ui::Tokens {
        glaze_ui::Tokens {
            icons: glaze_ui::icons::system_family(text),
            font: self.font.clone(),
            scale: self.scale,
            shadow: self.shadow,
            border: self.border && self.material.is_none(),
            glass: self.material,
            glass_style: self.glass_style,
            ..self.theme.tokens()
        }
    }

    pub fn color(&self, color: &ColorRef) -> Rgba {
        self.colors
            .get(color.as_str())
            .copied()
            .unwrap_or_else(|| color_with_system(color, &self.theme, &self.system_colors))
    }

    pub fn cache_config(&mut self, look: &Look, config: &glaze_bars::BarsConfig) {
        for (name, value) in &mut self.colors {
            if let Some(color) = ColorRef::parse(name) {
                *value = color_with_system(&color, &self.theme, &self.system_colors);
            }
        }
        cache_colors(
            &serde_json::to_value(&config.modules).unwrap_or_default(),
            &mut self.colors,
            &self.theme,
            &self.system_colors,
        );
        for (name, value) in &config.vars {
            if let Some(color) = value.as_str().and_then(ColorRef::parse) {
                self.colors.insert(
                    format!("var({name})"),
                    color_with_system(&color, &self.theme, &self.system_colors),
                );
            }
        }
        self.styles = super::styling::Styles::resolve(look, &self.theme, |c| self.color(c));
    }

    pub fn shadow_outset(&self, scale: f32) -> f32 {
        if self.material.is_none() && self.shadow {
            (self.styles.outset * scale).ceil()
        } else {
            0.0
        }
    }

    pub fn item(
        &self,
        id: u32,
        state: glaze_ui::interact::State,
        active: bool,
    ) -> super::styling::Item {
        let (hover, press) = self.interaction.get(&id).copied().unwrap_or((
            f32::from(state == glaze_ui::interact::State::Hover),
            f32::from(state == glaze_ui::interact::State::Pressed),
        ));
        let rest = if active {
            self.styles.items[2]
        } else {
            self.styles.items[0]
        };
        let mut item = rest
            .mix(self.styles.items[1], hover)
            .mix(self.styles.items[2], press);
        if self.styles.press_sink {
            item.scale *= 1.0 - 0.03 * press.clamp(0.0, 1.0);
        }
        item
    }
}

/// Optional optics shared by a bar and the popups it opens. Legacy `depth` means a bottom
/// inner shadow, independently of the renderer's broad shade under the lit top rim.
pub fn glass_style(look: &Look) -> glaze_ui::glass::GlassStyle {
    glaze_ui::glass::GlassStyle {
        brightness: match &look.background {
            Some(Fill::Wallpaper(fill)) => fill.brightness,
            Some(Fill::Blur(fill)) => fill.brightness,
            _ => 1.0,
        },
        saturation: look.saturation.unwrap_or(1.0),
        smoothing: if look.corners == Some(glaze_bars::style::Corners::Squircle) {
            0.6
        } else {
            0.0
        },
        sheen: look
            .glass
            .sheen
            .map(|v| v * look.glass.intensity.unwrap_or(1.0)),
        inner_shadow: look
            .glass
            .depth
            .map_or(0.0, |v| v * look.glass.intensity.unwrap_or(1.0)),
        ..Default::default()
    }
}

pub fn fade_ms(
    timing: glaze_ui::transition::Timing,
    style: glaze_bars::style::MotionStyle,
) -> f32 {
    if timing.enabled && style != glaze_bars::style::MotionStyle::None {
        (timing.duration.as_secs_f32() * 1000.0).min(5000.0)
    } else {
        0.0
    }
}

/// Attach the resolved show/open timing to glass alongside the content's transition.
pub fn glass_fade(
    glass: &mut glaze_ui::glass::GlassStyle,
    timing: glaze_ui::transition::Timing,
    style: glaze_bars::style::MotionStyle,
) {
    glass.fade_ms = fade_ms(timing, style);
    glass.fade_curve = (glass.fade_ms > 0.0).then_some(timing.easing);
}

pub fn plate_style(style: glaze_ui::glass::GlassStyle) -> glaze_glass_ipc::Style {
    glaze_glass_ipc::Style {
        brightness: style.brightness,
        saturation: style.saturation,
        smoothing: style.smoothing,
        sheen: style.sheen,
        depth: style.depth,
        inner_shadow: style.inner_shadow,
    }
}

fn cache_colors(
    value: &serde_json::Value,
    colors: &mut std::collections::HashMap<String, Rgba>,
    theme: &Theme,
    system: &[Rgba; 7],
) {
    match value {
        serde_json::Value::Object(map) => {
            for value in map.values() {
                cache_colors(value, colors, theme, system);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                cache_colors(value, colors, theme, system);
            }
        }
        serde_json::Value::String(value) => {
            if let Some(color) = ColorRef::parse(value) {
                colors.insert(
                    color.as_str().to_owned(),
                    color_with_system(&color, theme, system),
                );
            }
        }
        _ => {}
    }
}

fn color_with_system(color: &ColorRef, theme: &Theme, system: &[Rgba; 7]) -> Rgba {
    match color.as_str() {
        "var(--fill-1)" => return theme.color.well,
        "var(--fill-2)" => return theme.color.hover,
        "var(--fill-3)" => return theme.color.pressed,
        "var(--edge)" => return theme.color.separator,
        "var(--t0)" => return theme.color.label,
        "var(--t1)" => return theme.color.secondary,
        "var(--t2)" => return theme.color.tertiary,
        "var(--t3)" => return theme.color.quaternary,
        "var(--accent)" => return theme.color.accent,
        _ => {}
    }
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
    index.map_or_else(
        || resolve_color(color, theme.color.label, theme.color.accent),
        |i| system[i],
    )
}

/// The menu tokens of the top bar's look from `bars.json` and the theme now: the desktop and
/// tray menus look like the bar's own.
pub fn menu_tokens(text: &glaze_ui::Text) -> glaze_ui::Tokens {
    let bars = glaze_bars::config::path()
        .and_then(|p| glaze_bars::BarsConfig::load(&p))
        .map(|mut c| {
            c.resolve(Vec::new);
            c
        })
        .inspect_err(|e| log::debug!("menus: bars.json: {e:#}"))
        .unwrap_or_default();
    let loaded = glaze_config::Config::load()
        .and_then(|c| glaze_theme::locate(Some(&c.theme)))
        .and_then(LoadedTheme::load)
        .ok();
    let mut appearance = bars.appearance.over(&theme_appearance(loaded.as_ref()));
    // The menus follow the top bar's plugin patches, then the `menu` surface's own colours.
    let mut look = bars.top.look.clone();
    appearance_layer(&bars, "top", &mut look, &appearance);
    glass_layer(
        &bars,
        "top",
        &mut look,
        loaded
            .as_ref()
            .and_then(|t| t.theme.bars.as_ref())
            .and_then(|s| s.glass),
    );
    let mut colors =
        super::plugin_style::patch_look("top-bar", "top", bars.raw(), &mut look, &mut appearance);
    colors.extend(super::plugin_style::tokens("menu"));
    let desktop = glaze_config::Config::load().unwrap_or_default().look;
    let system = accent::read();
    popup_font(&bars, "top", &mut look);
    crate::look::settings::bar(&desktop, crate::glass::Family::Menus, &bars, "top", &mut look, &mut appearance, &system, text);
    let mut resolved = crate::flyouts::look::Look::resolve_settings(&look, &appearance, &system, &crate::glass::knobs(&desktop, crate::glass::Family::Menus), &desktop, text);
    resolved.patch_colors(&colors);
    resolved.menu_tokens()
}

/// The theme's `bars` appearance, its `auto` accent taken from the wallpaper's palette.
pub fn theme_appearance(loaded: Option<&LoadedTheme>) -> Appearance {
    let mut appearance = loaded
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|s| s.appearance.as_ref())
        .cloned()
        .unwrap_or_default();
    if let Some(t) = loaded
        && matches!(appearance.accent, Some(glaze_theme::Paint::Auto))
        && let Some(snapshot) = glaze_tweaks::palette::read().filter(|p| p.is_for(&t.dir))
    {
        appearance.accent = Some(glaze_theme::Paint::Color(snapshot.palette.accent));
    }
    appearance
}

/// Design bundles supply defaults. Theme/user appearance beats those defaults, while fields
/// explicitly written in shared/per-bar `look` (including monitor overrides) still win.
pub fn appearance_layer(
    config: &glaze_bars::BarsConfig,
    bar: &str,
    look: &mut Look,
    appearance: &Appearance,
) {
    let own = |path: &str| explicit_look(config, bar, path);
    if appearance.font.is_some() && !own("/font/family") {
        look.font.family = None;
    }
    if appearance.radius.is_some() && !own("/radius") {
        look.radius = None;
    }
    if appearance.text.is_some() && !own("/scheme") {
        look.scheme = None;
    }
    if appearance.surface.is_some() && !own("/background") && !own("/transparent") {
        look.background = None;
        look.transparent = None;
    }
    if appearance.blur.is_some() && !own("/glass/blur") {
        look.glass.blur = appearance.blur;
    }
    if appearance.opacity.is_some() {
        if !own("/tint") {
            look.tint = None;
        }
        if !own("/glass/tint") {
            look.glass.tint = None;
        }
    }
    if appearance.border.is_some() && !own("/border") {
        look.border = Default::default();
    }
}

pub(crate) fn explicit_look(config: &glaze_bars::BarsConfig, bar: &str, path: &str) -> bool {
    config
        .raw()
        .get(bar)
        .and_then(|v| v.get("look"))
        .and_then(|v| v.pointer(path))
        .or_else(|| config.raw().get("look").and_then(|v| v.pointer(path)))
        .is_some_and(|v| !v.is_null())
}

/// A bar design's 13 px / 400 label is a bar metric. Popup roles retain their own type ramp
/// unless the user explicitly overrides the shared/per-bar font size or weight.
pub fn popup_font(config: &glaze_bars::BarsConfig, bar: &str, look: &mut Look) {
    if !explicit_look(config, bar, "/font/size") {
        look.font.size = None;
    }
    if !explicit_look(config, bar, "/font/weight") {
        look.font.weight = None;
    }
}

pub fn glass_layer(
    config: &glaze_bars::BarsConfig,
    bar: &str,
    look: &mut Look,
    glass: Option<glaze_theme::Liquid>,
) {
    let Some(glass) = glass else { return };
    for (name, value, target) in [
        ("intensity", glass.intensity, &mut look.glass.intensity),
        ("refraction", glass.refraction, &mut look.glass.refraction),
        ("highlight", glass.highlight, &mut look.glass.highlight),
        ("tint", glass.tint, &mut look.glass.tint),
        ("blur", glass.blur, &mut look.glass.blur),
    ] {
        if value.is_some() && !explicit_look(config, bar, &format!("/glass/{name}")) {
            *target = value;
        }
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
                "native bars ignore CSS, gradient/image fills, noise, glow, neon and bevel/scoop/notch corners"
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
                && !f.to_ascii_lowercase().starts_with("segoe ui")
                && !f.to_ascii_lowercase().starts_with("sf pro")
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
    fn glass_optics_and_background_frost_resolve_into_cached_plate_and_popup_styles() {
        let look: Look = serde_json::from_value(serde_json::json!({
            "background": { "type": "wallpaper", "brightness": 0.65, "blur": 48 },
            "saturation": 0.0, "corners": "squircle",
            "glass": { "blur": 32, "sheen": 0.7, "depth": 0.2 }
        }))
        .unwrap();
        let tokens = Tokens::resolve(
            &look,
            &Appearance::default(),
            &crate::flyouts::look::fixed_system(),
        );
        assert_eq!(tokens.material.unwrap().blur, 80.0);
        assert_eq!(
            tokens.plate_style,
            glaze_glass_ipc::Style {
                brightness: 0.65,
                saturation: 0.0,
                smoothing: 0.6,
                sheen: Some(0.7),
                depth: None,
                inner_shadow: 0.2,
            }
        );
        let popup = crate::flyouts::look::Look::resolve(
            &look,
            &Appearance::default(),
            &crate::flyouts::look::fixed_system(),
        );
        assert_eq!(popup.glass_style, tokens.glass_style);
        assert_eq!(popup.menu_tokens().glass_style, tokens.glass_style);
        assert_eq!(popup.glass.unwrap().blur, 80.0);
        let reset = Tokens::resolve(
            &Look::default(),
            &Appearance::default(),
            &crate::flyouts::look::fixed_system(),
        );
        assert_eq!(reset.plate_style, Default::default());
    }

    #[test]
    fn overlay_fade_obeys_resolved_motion_policy_and_duration() {
        use super::super::motion::{self, Leaf};
        use glaze_bars::style::{Motion, MotionStyle};
        for (value, system, performance, expected) in [
            (
                serde_json::json!({"duration": 5000, "curve": "linear"}),
                true,
                false,
                5000.0,
            ),
            (
                serde_json::json!({"duration": 640, "curve": "linear"}),
                true,
                false,
                640.0,
            ),
            (serde_json::json!({"enabled": false}), true, false, 0.0),
            (
                serde_json::json!({"reduceMotion": "always"}),
                true,
                false,
                0.0,
            ),
            (
                serde_json::json!({"reduceMotion": "system"}),
                false,
                false,
                0.0,
            ),
            (
                serde_json::json!({"reduceMotion": "never", "duration": 640, "curve": "linear"}),
                false,
                false,
                640.0,
            ),
            (
                serde_json::json!({"reduceMotion": "never"}),
                true,
                true,
                0.0,
            ),
            (
                serde_json::json!({"open": {"style": "none"}}),
                true,
                false,
                0.0,
            ),
        ] {
            let animation: Motion = serde_json::from_value(value).unwrap();
            let timing = motion::resolve(&animation, false, Leaf::Open, system, performance);
            assert_eq!(
                fade_ms(timing, motion::presentation(&animation, false, Leaf::Open)),
                expected
            );
            assert_eq!(fade_ms(timing, MotionStyle::None), 0.0);
        }
    }

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
    /// none, and Mono keeps the kit's deliberately translucent flat tint.
    #[test]
    fn styles_pick_distinct_materials() {
        let system = crate::flyouts::look::fixed_system();
        let design = Look {
            glass: glaze_bars::style::GlassStyle {
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
            let expected = if flat == NativeStyle::Solid { 1.0 } else { 0.78 };
            assert_eq!(t.theme.color.surface.a, expected, "{flat:?}: flat fill");
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
                    glass: glaze_bars::style::GlassStyle {
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
                surface: Some(glaze_theme::Surface::None),
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
        for (i, name) in glaze_bars::style::COLOR_TOKENS[3..].iter().enumerate() {
            assert_eq!(
                tokens.color(&ColorRef::parse(name).unwrap()),
                Rgba::rgb8([i as u8, 0, 0], 1.0),
                "{name}"
            );
        }
    }

    #[test]
    fn appearance_beats_design_defaults_but_explicit_look_and_monitor_fields_win() {
        let config = glaze_bars::BarsConfig::parse(r#"{"preset":"macos","appearance":{"font":"Inter","radius":7,"surface":"solid","text":"dark"},"overrides":{"secondary":{"look":{"radius":3}}}}"#).unwrap();
        let mut look = config.top.look.clone();
        appearance_layer(&config, "top", &mut look, &config.appearance);
        let system = crate::flyouts::look::fixed_system();
        let resolved = Tokens::resolve(&look, &config.appearance, &system);
        assert_eq!(&*resolved.font.family, "Inter");
        assert_eq!(resolved.radius, 7.0);
        assert!(!resolved.bare && resolved.material.is_none() && !resolved.theme.dark());
        let second = config.for_monitor("DISPLAY2", false).unwrap();
        let mut look = second.top.look.clone();
        appearance_layer(&second, "top", &mut look, &second.appearance);
        assert_eq!(
            Tokens::resolve(&look, &second.appearance, &system).radius,
            3.0
        );
    }

    #[test]
    fn theme_glass_overrides_design_defaults_but_keeps_explicit_user_fields() {
        let theme = Some(glaze_theme::Liquid {
            blur: Some(17.0),
            refraction: Some(0.2),
            ..Default::default()
        });
        let config = glaze_bars::BarsConfig::parse(
            r#"{"preset":"macos","dock":{"look":{"glass":{"blur":4}}}}"#,
        )
        .unwrap();
        let mut look = config.dock.look.clone();
        glass_layer(&config, "dock", &mut look, theme);
        assert_eq!(look.glass.blur, Some(4.0));
        assert_eq!(look.glass.refraction, Some(0.2));
        let mut reset = config.dock.look.clone();
        glass_layer(&config, "dock", &mut reset, None);
        assert_eq!(reset, config.dock.look);
    }
}
