//! The design tokens every surface draws from: one [`Theme`] per style, scheme and accent.
//!
//! - **Colour roles** ([`Palette`]): labels in four steps, fills in three, separators, accent,
//!   destructive, the surface itself. Tuned for text **on glass**: the label steps sit above
//!   Apple's (whose vibrancy blend we can't use on a layered window), and every step is held to
//!   a contrast floor over its own surface by a test (label 7:1, the kit's AAA target from
//!   liquid-rust's large light glass; secondary 4.5:1; tertiary 3:1).
//! - **Type** ([`TypeRamp`], [`TextRole`]): Segoe UI Variable through its axes, so a 500
//!   weight is a real medium and the optical size follows the size, with tracking and line
//!   heights per role.
//! - **Space, radii, elevation, motion** ([`Metrics`], [`Radii`], [`Elevation`],
//!   [`MotionTokens`]). Radii are concentric: a card inside a panel is the panel's radius less
//!   the padding between them ([`Radii::concentric`]).
//! - **Glass**: the five styles are token sets over the same components. Liquid, One UI and
//!   Fluent put engine glass under a surface (one border total, drawn by the engine); Solid and
//!   Mono draw their own fill and, for Solid, one hairline.
//!
//! Values are in design pixels (96 DPI); the kit's [`crate::kit::Frame`] multiplies them by the
//! surface's scale. Adaptive light/dark for small glass and the springs follow liquid-rust
//! (github.com/al1h3n/liquid-rust, MIT/Apache-2.0), which matches Apple's iOS 26 kit.

use std::sync::Arc;

use crate::geom::Rgba;
use crate::glass::GlassMaterial;
use crate::icons::IconSet;
use crate::motion::Spring;
use crate::text::{TextStyle, VARIABLE};

/// The five looks (plan, "Styles").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Style {
    /// macOS Tahoe: clear Liquid Glass, capsule controls, vector icons.
    #[default]
    Liquid,
    /// One UI: tinted blur, large radii, bold titles.
    OneUi,
    /// Opaque surfaces with one crisp hairline.
    Solid,
    /// Windows 11: tinted blur, small radii, Segoe Fluent Icons.
    Fluent,
    /// Monochrome: no accent colour, a flat tint.
    Mono,
}

impl Style {
    pub const ALL: [Self; 5] = [Self::Liquid, Self::OneUi, Self::Solid, Self::Fluent, Self::Mono];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Liquid => "liquid",
            Self::OneUi => "oneui",
            Self::Solid => "solid",
            Self::Fluent => "fluent",
            Self::Mono => "mono",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    Light,
    Dark,
}

/// Colour roles, straight alpha. "On glass" means over the surface's own material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Titles, values, glyphs.
    pub label: Rgba,
    /// Subtitles, details.
    pub secondary: Rgba,
    /// Timestamps, placeholders, hints.
    pub tertiary: Rgba,
    /// Disabled text and glyphs.
    pub quaternary: Rgba,
    /// A control's resting fill: an off switch, a slider's track, a plain button.
    pub fill: Rgba,
    /// Cards, tiles and grouped rows on the surface.
    pub card: Rgba,
    /// A quieter well (a text field, a segmented control's track).
    pub well: Rgba,
    pub separator: Rgba,
    pub accent: Rgba,
    /// Text and glyphs on an accent fill.
    pub on_accent: Rgba,
    /// The accent as a wash (tinted buttons, a selected row).
    pub accent_wash: Rgba,
    pub destructive: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    /// The surface's own fill where there is no glass (and the glass's tint).
    pub surface: Rgba,
    /// Popovers and tooltips drawn by the shell over the surface.
    pub elevated: Rgba,
    /// Washes over a hovered and a pressed control.
    pub hover: Rgba,
    pub pressed: Rgba,
    /// A switch's or slider's knob.
    pub knob: Rgba,
    /// The overlay scrollbar's thumb.
    pub scrollbar: Rgba,
    pub shadow: Rgba,
}

/// A step of the type ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextRole {
    /// Hero values (a clock, a temperature in a large widget).
    LargeTitle,
    Title1,
    /// Panel titles.
    Title2,
    /// Widget and section titles.
    Title3,
    /// Row and card titles.
    Headline,
    /// Reading text.
    Body,
    /// Secondary lines under a title.
    Callout,
    /// Section headers, small labels.
    Subheadline,
    /// Timestamps, footers.
    Footnote,
    /// Badges, counts, axis labels.
    Caption,
}

impl TextRole {
    pub const ALL: [Self; 10] = [
        Self::LargeTitle,
        Self::Title1,
        Self::Title2,
        Self::Title3,
        Self::Headline,
        Self::Body,
        Self::Callout,
        Self::Subheadline,
        Self::Footnote,
        Self::Caption,
    ];
}

/// One role's type, design px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font {
    pub size: f32,
    pub weight: u16,
    /// Extra space per character (negative tightens).
    pub tracking: f32,
    /// Baseline-to-baseline distance of wrapped lines.
    pub line: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeRamp {
    pub family: Arc<str>,
    pub roles: [Font; 10],
}

impl TypeRamp {
    pub fn font(&self, role: TextRole) -> Font {
        self.roles[role as usize]
    }

    /// `role` at `s` device px per design px.
    pub fn style(&self, role: TextRole, s: f32) -> TextStyle {
        let f = self.font(role);
        TextStyle {
            family: self.family.clone(),
            size: f.size * s,
            weight: f.weight,
            tracking: f.tracking * s,
            optical: f.size * 0.75,
        }
    }

    /// The line height of `role` in device px.
    pub fn line(&self, role: TextRole, s: f32) -> f32 {
        (self.font(role).line * s).round()
    }
}

/// Lengths, design px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Inside a panel's edge.
    pub pad: f32,
    /// Between blocks of a panel.
    pub gap: f32,
    /// Between items of a list or a row of controls.
    pub item: f32,
    /// A row with a title and a subtitle.
    pub row: f32,
    /// A row with a title only.
    pub row_single: f32,
    /// A button's height.
    pub control: f32,
    pub control_small: f32,
    /// A row's or a button's glyph.
    pub icon: f32,
    pub icon_small: f32,
    pub icon_large: f32,
    /// Smallest pointer target.
    pub hit: f32,
    /// The overlay scrollbar's thickness and its inset from the viewport's edge.
    pub scrollbar: f32,
    pub scrollbar_inset: f32,
}

/// Corner radii, design px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    /// A panel, flyout, menu or widget body.
    pub surface: f32,
    /// A card inside a surface: concentric with it across `Metrics::pad`.
    pub card: f32,
    /// Buttons, fields and rows' highlights; `None` makes them capsules.
    pub control: Option<f32>,
    /// Badges, thumbnails.
    pub small: f32,
}

impl Radii {
    /// The radius of a shape inset `inset` inside one of radius `outer`: concentric corners
    /// share their centre, so the inner radius is the outer less the inset (never below
    /// `min`).
    pub fn concentric(outer: f32, inset: f32, min: f32) -> f32 {
        (outer - inset).max(min)
    }
}

/// A shadow under shell-drawn popovers (glass surfaces get theirs from the engine).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elevation {
    pub spread: f32,
    pub drop: f32,
    pub color: Rgba,
}

/// Motion tokens, Apple's duration/bounce springs (see [`Spring::apple`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTokens {
    /// Press and release, drags.
    pub interactive: Spring,
    /// Panels opening.
    pub open: Spring,
    /// Layout changes, a switch's knob.
    pub layout: Spring,
    /// A wheel notch scrolling.
    pub scroll: Spring,
    /// How long the scrollbar stays after the last scroll, then fades, in seconds.
    pub scrollbar_idle: f32,
    pub scrollbar_fade: f32,
}

/// Every token a surface draws with.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub style: Style,
    pub scheme: Scheme,
    pub color: Palette,
    pub font: TypeRamp,
    pub metrics: Metrics,
    pub radius: Radii,
    pub elevation: Elevation,
    pub motion: MotionTokens,
    pub icons: IconSet,
    /// Icon stroke per 24 units of icon size (Lucide draws 2; SF-like is lighter).
    pub icon_weight: f32,
    /// Engine glass under surfaces, or `None` for a drawn [`Palette::surface`].
    pub glass: Option<GlassMaterial>,
    /// The one hairline a glassless surface draws around itself (Solid), never with glass.
    pub border: Option<Rgba>,
}

/// iOS 26 `systemBlue` (`#0088FF`, liquid-rust's `Color::BLUE`): the accent when Windows gives
/// none.
pub const SYSTEM_BLUE: Rgba = Rgba::new(0.0, 136.0 / 255.0, 1.0, 1.0);

impl Theme {
    /// A surface override and its concentric cards, without changing the style's defaults.
    pub fn with_surface_radius(mut self, radius: f32) -> Self {
        self.radius.surface = radius;
        self.radius.card = (radius - self.metrics.pad).max(0.0);
        self
    }

    /// `style` in `scheme`, with `accent` (Windows' accent colour, or [`SYSTEM_BLUE`]).
    pub fn new(style: Style, scheme: Scheme, accent: Rgba) -> Self {
        let dark = scheme == Scheme::Dark;
        let color = palette(style, dark, accent);
        let (surface, pad, card_min, control) = match style {
            Style::Liquid => (24.0, 12.0, 10.0, None),
            Style::OneUi => (28.0, 12.0, 16.0, None),
            Style::Solid => (14.0, 10.0, 6.0, Some(8.0)),
            Style::Fluent => (8.0, 8.0, 4.0, Some(4.0)),
            Style::Mono => (16.0, 10.0, 8.0, Some(10.0)),
        };
        let metrics = Metrics {
            pad,
            gap: if style == Style::Fluent { 8.0 } else { 12.0 },
            item: 6.0,
            row: if style == Style::OneUi { 52.0 } else { 46.0 },
            row_single: if style == Style::OneUi { 40.0 } else { 36.0 },
            control: if style == Style::Fluent { 32.0 } else { 30.0 },
            control_small: 24.0,
            icon: 16.0,
            icon_small: 13.0,
            icon_large: 20.0,
            hit: 28.0,
            scrollbar: 5.0,
            scrollbar_inset: 3.0,
        };
        let glass = material(style, color.surface);
        Self {
            style,
            scheme,
            font: ramp(style),
            radius: Radii {
                surface,
                card: Radii::concentric(surface, pad, card_min),
                control,
                small: if style == Style::Fluent { 4.0 } else { 6.0 },
            },
            elevation: Elevation {
                spread: 16.0,
                drop: 4.0,
                color: color.shadow,
            },
            motion: MotionTokens {
                interactive: Spring::INTERACTIVE,
                open: Spring::apple(0.3, 0.12),
                layout: Spring::SNAPPY,
                scroll: Spring::apple(0.22, 0.0),
                scrollbar_idle: 0.9,
                scrollbar_fade: 0.35,
            },
            icons: match style {
                Style::Fluent => IconSet::Font(crate::icons::SYSTEM_FAMILY.into()),
                _ => IconSet::Vector,
            },
            icon_weight: match style {
                Style::OneUi | Style::Fluent => 2.0,
                _ => 1.75,
            },
            border: (style == Style::Solid).then_some(color.separator),
            glass,
            color,
            metrics,
        }
    }

    pub fn dark(&self) -> bool {
        self.scheme == Scheme::Dark
    }

    /// The scheme of content on glass whose smaller side is `min_side` design px, over a
    /// backdrop of WCAG luminance `backdrop` (when the engine reports one). liquid-rust's rule,
    /// from Apple's kit: small glass (buttons, a bar, a small widget) flips with what is behind
    /// it; from about 180 pt (panels, menus) it follows the colour scheme. It always takes one
    /// face or the other. The flip is where black and white text have equal contrast (0.179).
    pub fn adaptive(&self, min_side: f32, backdrop: Option<f32>) -> Scheme {
        let thickness = smoothstep(44.0, 320.0, min_side);
        let adapt = 1.0 - smoothstep(0.0, 0.5, thickness);
        match backdrop {
            Some(luminance) if adapt >= 0.5 => {
                if luminance > 0.179 {
                    Scheme::Light
                } else {
                    Scheme::Dark
                }
            }
            _ => self.scheme,
        }
    }

    /// This theme in `scheme` (for small glass that flipped, [`Theme::adaptive`]).
    pub fn with_scheme(&self, scheme: Scheme) -> Self {
        if scheme == self.scheme {
            return self.clone();
        }
        Self::new(self.style, scheme, self.color.accent)
    }

    /// The menu component's tokens (`crate::Menu`), from this theme.
    pub fn tokens(&self) -> crate::Tokens {
        let base = crate::Tokens::default();
        crate::Tokens {
            font: self.font.style(TextRole::Body, 1.0),
            icons: crate::icons::SYSTEM_FAMILY.into(),
            colors: crate::Colors {
                text: self.color.label,
                text_dim: self.color.quaternary,
                hover: self.color.hover.with_alpha(self.color.hover.a * 1.6),
                line: self.color.separator,
                shadow: self.color.shadow.with_alpha(0.05),
            },
            material: crate::Material {
                tint: self.color.surface,
            },
            radius: Radii::concentric(self.radius.surface, 6.0, 8.0),
            border: self.border.is_some(),
            glass: self.glass,
            ..base
        }
    }

    /// The four label steps as the older [`crate::Ink`].
    pub fn ink(&self) -> crate::Ink {
        crate::Ink {
            primary: self.color.label,
            secondary: self.color.secondary,
            tertiary: self.color.tertiary,
            disabled: self.color.quaternary,
        }
    }
}

/// The engine material of `style` under a surface tinted `tint` (its alpha is the tint's
/// strength), or `None` for the styles that draw their own fill. The one place a style becomes
/// glass; bars, widgets, flyouts and menus all start here and only layer the user's knobs over
/// it.
///
/// - **Liquid**: Liquid Glass, a lens at the rim and a specular light (liquid-rust's regular).
/// - **One UI**: a frosted card, slightly translucent: heavy blur, no lens, no rim light.
/// - **Fluent**: Windows 11 acrylic: the heaviest blur under a strong tint, no lens or rim
///   (the engine drops dispersion and the inner rims with refraction and highlight at 0).
/// - **Solid**, **Mono**: no glass (opaque, and flat near-opaque monochrome).
pub fn material(style: Style, tint: Rgba) -> Option<GlassMaterial> {
    let (clarity, blur, refraction, highlight, strength) = match style {
        Style::Liquid => (0.55, 12.0, 0.6, 0.75, tint.a),
        Style::OneUi => (0.35, 24.0, 0.0, 0.0, tint.a.max(0.35)),
        Style::Fluent => (0.12, 30.0, 0.0, 0.0, tint.a.max(0.6)),
        Style::Solid | Style::Mono => return None,
    };
    Some(GlassMaterial {
        clarity,
        blur,
        refraction,
        highlight,
        tint: tint.with_alpha(strength),
    })
}

/// Whether `style` puts a lens under its surfaces (only Liquid; the others are flat).
pub const fn lensed(style: Style) -> bool {
    matches!(style, Style::Liquid)
}

/// The user's material controls (`config.json` › `look.material`, Studio's Look sliders). Each
/// one that is set replaces a surface's own value ([`GlassMaterial::tuned`]); unset ones keep it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Knobs {
    /// Clear (1) to tinted (0).
    pub clarity: Option<f32>,
    /// Frost in design px (DIPs): 0 is no frost at all.
    pub blur: Option<f32>,
    pub refraction: Option<f32>,
    pub highlight: Option<f32>,
}

impl Knobs {
    /// `self` over `base`: each control set here wins.
    pub fn over(self, base: Knobs) -> Knobs {
        Knobs {
            clarity: self.clarity.or(base.clarity),
            blur: self.blur.or(base.blur),
            refraction: self.refraction.or(base.refraction),
            highlight: self.highlight.or(base.highlight),
        }
    }
}

impl GlassMaterial {
    /// This surface material (its style's, from [`material`], with the surface's own defaults)
    /// under the user's `knobs`: the one rule bars, widgets, flyouts and menus all follow, so the
    /// same sliders give the same glass everywhere. Nothing is floored or capped: blur 0 is no
    /// frost, and clarity 1 is clear glass with no tint left (nearly invisible). A clarity the
    /// user set also sets the tint's strength, `1 - clarity`, on every surface alike; unset, each
    /// surface keeps its own. Refraction and the highlight belong to Liquid; the flat styles keep
    /// them at 0.
    pub fn tuned(self, style: Style, knobs: &Knobs) -> Self {
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let clarity = knobs.clarity.map_or(self.clarity, unit);
        let strength = knobs.clarity.map_or(self.tint.a, |_| 1.0 - clarity);
        let lensed = lensed(style);
        let lens = |own: f32, knob: Option<f32>| if lensed { knob.map_or(own, unit) } else { 0.0 };
        Self {
            clarity,
            blur: knobs.blur.map_or(self.blur, |b| if b.is_finite() { b.max(0.0) } else { 0.0 }),
            refraction: lens(self.refraction, knobs.refraction),
            highlight: lens(self.highlight, knobs.highlight),
            tint: self.tint.with_alpha(strength),
        }
    }
}

/// The user's style for surfaces that carry no look of their own (widgets), process-wide.
static ACTIVE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Sets the user's active style (the shell sets it from `config.json` › `look`).
pub fn set_active(style: Style) {
    let index = Style::ALL.iter().position(|s| *s == style).unwrap_or(0);
    ACTIVE.store(index as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The user's active style; Liquid until [`set_active`] says otherwise.
pub fn active() -> Style {
    Style::ALL[ACTIVE.load(std::sync::atomic::Ordering::Relaxed) as usize % Style::ALL.len()]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hex(rgb: u32, a: f32) -> Rgba {
    Rgba::rgb8([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8], a)
}

/// WCAG relative luminance of an opaque sRGB colour.
pub fn luminance(c: Rgba) -> f32 {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio of two opaque colours.
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` composited over opaque `under` (straight alpha, in sRGB as DWM blends).
pub fn over(top: Rgba, under: Rgba) -> Rgba {
    let mix = |a: f32, b: f32| a * top.a + b * (1.0 - top.a);
    Rgba::new(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b), 1.0)
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn palette(style: Style, dark: bool, accent: Rgba) -> Palette {
    // Labels: Apple's (white / `#3C3C43` on light) with the lower steps raised for glass.
    let (ink, sub) = if dark {
        (hex(0xFFFFFF, 1.0), hex(0xEBEBF5, 1.0))
    } else {
        (hex(0x000000, 0.88), hex(0x3C3C43, 1.0))
    };
    let (secondary, tertiary, quaternary) = match (style, dark) {
        // Windows 11's text steps (TextFillColorSecondary/Tertiary/Disabled).
        (Style::Fluent, true) => (0.79, 0.55, 0.36),
        (Style::Fluent, false) => (0.80, 0.62, 0.42),
        (_, true) => (0.72, 0.50, 0.30),
        (_, false) => (0.78, 0.60, 0.34),
    };
    let surface = match (style, dark) {
        (Style::Solid, true) => hex(0x1C1C1E, 1.0),
        (Style::Solid, false) => hex(0xF2F2F7, 1.0),
        (Style::Fluent, true) => hex(0x202020, 0.90),
        (Style::Fluent, false) => hex(0xF3F3F3, 0.90),
        (Style::OneUi, true) => hex(0x17171A, 0.92),
        (Style::OneUi, false) => hex(0xF4F4F6, 0.92),
        (Style::Mono, true) => hex(0x141416, 0.94),
        (Style::Mono, false) => hex(0xF7F7F8, 0.94),
        // liquid-rust's tinted dark face, `#1C1C1E` at 70-85 %; white frost on light.
        (Style::Liquid, true) => hex(0x1C1C1E, 0.82),
        (Style::Liquid, false) => hex(0xF5F5F7, 0.80),
    };
    let wash = |a: f32| if dark { ink.with_alpha(a) } else { hex(0x000000, a) };
    let card = match (style, dark) {
        (Style::Solid, true) => hex(0x2C2C2E, 1.0),
        (Style::Solid, false) => hex(0xFFFFFF, 1.0),
        (Style::Fluent, true) => hex(0xFFFFFF, 0.06),
        (Style::Fluent, false) => hex(0xFFFFFF, 0.70),
        (Style::OneUi, true) => hex(0xFFFFFF, 0.09),
        (Style::OneUi, false) => hex(0xFFFFFF, 0.90),
        (_, true) => hex(0xFFFFFF, 0.10),
        (_, false) => hex(0xFFFFFF, 0.55),
    };
    // Mono has no colour: its accent is the label.
    let accent = if style == Style::Mono {
        if dark { hex(0xF2F2F2, 1.0) } else { hex(0x1C1C1E, 1.0) }
    } else {
        readable_accent(accent, over(surface, if dark { hex(0, 1.0) } else { hex(0xFFFFFF, 1.0) }))
    };
    let on_accent = if contrast(hex(0xFFFFFF, 1.0), accent) >= 3.0 {
        hex(0xFFFFFF, 1.0)
    } else {
        hex(0x111113, 1.0)
    };
    Palette {
        label: ink,
        secondary: sub.with_alpha(secondary),
        tertiary: sub.with_alpha(tertiary),
        quaternary: sub.with_alpha(quaternary),
        fill: if dark {
            hex(0xFFFFFF, 0.16)
        } else {
            hex(0x787880, 0.20)
        },
        card,
        well: if dark {
            hex(0xFFFFFF, 0.07)
        } else {
            hex(0x787880, 0.10)
        },
        separator: if dark {
            hex(0xFFFFFF, 0.13)
        } else {
            hex(0x3C3C43, 0.16)
        },
        accent,
        on_accent,
        accent_wash: accent.with_alpha(if dark { 0.24 } else { 0.16 }),
        destructive: if dark { hex(0xFF453A, 1.0) } else { hex(0xFF3B30, 1.0) },
        success: if dark { hex(0x30D158, 1.0) } else { hex(0x34C759, 1.0) },
        warning: if dark { hex(0xFF9F0A, 1.0) } else { hex(0xFF9500, 1.0) },
        surface,
        elevated: if dark { hex(0x2C2C2E, 0.98) } else { hex(0xFFFFFF, 0.98) },
        hover: wash(if dark { 0.08 } else { 0.05 }),
        pressed: wash(if dark { 0.16 } else { 0.10 }),
        knob: hex(0xFFFFFF, 1.0),
        scrollbar: wash(if dark { 0.42 } else { 0.36 }),
        shadow: hex(0x000000, if dark { 0.45 } else { 0.18 }),
    }
}

/// `accent`, lightened (on dark) or darkened (on light) until it reaches 3:1 against
/// `surface`, so an accent glyph or a selected switch reads on either scheme.
fn readable_accent(accent: Rgba, surface: Rgba) -> Rgba {
    let toward = if luminance(surface) < 0.18 {
        hex(0xFFFFFF, 1.0)
    } else {
        hex(0x000000, 1.0)
    };
    let mut c = accent.with_alpha(1.0);
    for _ in 0..10 {
        if contrast(c, surface) >= 3.0 {
            break;
        }
        c = mix(c, toward, 0.12);
    }
    c
}

fn ramp(style: Style) -> TypeRamp {
    let f = |size: f32, weight: u16, tracking: f32, line: f32| Font {
        size,
        weight,
        tracking,
        line,
    };
    let roles = match style {
        // Windows 11's ramp: Caption 12, Body 14, Body Strong 14, Subtitle 20, Title 28.
        Style::Fluent => [
            f(28.0, 600, -0.3, 36.0),
            f(20.0, 600, -0.2, 26.0),
            f(16.0, 600, 0.0, 22.0),
            f(14.0, 600, 0.0, 20.0),
            f(14.0, 600, 0.0, 20.0),
            f(14.0, 400, 0.0, 20.0),
            f(13.0, 400, 0.0, 18.0),
            f(12.0, 600, 0.0, 16.0),
            f(12.0, 400, 0.0, 16.0),
            f(11.0, 400, 0.0, 14.0),
        ],
        Style::OneUi => [
            f(30.0, 700, -0.5, 36.0),
            f(23.0, 700, -0.3, 28.0),
            f(18.0, 700, -0.2, 23.0),
            f(16.0, 700, -0.1, 21.0),
            f(15.0, 600, 0.0, 20.0),
            f(15.0, 400, 0.0, 20.0),
            f(13.0, 400, 0.0, 18.0),
            f(12.0, 600, 0.0, 16.0),
            f(12.0, 400, 0.0, 16.0),
            f(11.0, 600, 0.0, 14.0),
        ],
        // macOS Tahoe's proportions at Windows' reading size (body 14, not 13: grayscale text
        // at 96 DPI needs the extra pixel).
        Style::Liquid | Style::Solid | Style::Mono => [
            f(28.0, 700, -0.6, 34.0),
            f(22.0, 700, -0.4, 28.0),
            f(17.0, 600, -0.2, 22.0),
            f(15.0, 600, -0.1, 20.0),
            f(14.0, 600, 0.0, 19.0),
            f(14.0, 400, 0.0, 19.0),
            f(13.0, 400, 0.0, 17.0),
            f(12.0, 600, 0.1, 16.0),
            f(12.0, 400, 0.0, 16.0),
            f(11.0, 500, 0.1, 14.0),
        ],
    };
    TypeRamp {
        family: VARIABLE.into(),
        roles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_their_contrast_floors_over_their_own_surface_in_every_style() {
        for style in Style::ALL {
            for scheme in [Scheme::Dark, Scheme::Light] {
                let t = Theme::new(style, scheme, SYSTEM_BLUE);
                let under = if t.dark() { hex(0x000000, 1.0) } else { hex(0xFFFFFF, 1.0) };
                // The worst case for the surface: its tint over a backdrop of the other
                // extreme is checked by the engine's glass; here, over its own scheme's.
                let surface = over(t.color.surface, under);
                let check = |name: &str, c: Rgba, floor: f32| {
                    let ratio = contrast(over(c, surface), surface);
                    assert!(
                        ratio >= floor,
                        "{style:?} {scheme:?} {name}: {ratio:.2}:1 < {floor}"
                    );
                };
                check("label", t.color.label, 7.0);
                check("secondary", t.color.secondary, 4.5);
                check("tertiary", t.color.tertiary, 3.0);
                // A card's labels too.
                let card = over(t.color.card, surface);
                let ratio = contrast(over(t.color.secondary, card), card);
                assert!(ratio >= 4.5, "{style:?} {scheme:?} secondary on card: {ratio:.2}");
                if style != Style::Mono {
                    let a = contrast(t.color.accent, surface);
                    assert!(a >= 3.0, "{style:?} {scheme:?} accent {a:.2}");
                }
                let on = contrast(t.color.on_accent, t.color.accent);
                assert!(on >= 3.0, "{style:?} {scheme:?} on accent {on:.2}");
            }
        }
    }

    #[test]
    fn radii_are_concentric_and_glass_draws_no_border() {
        for style in Style::ALL {
            let t = Theme::new(style, Scheme::Dark, SYSTEM_BLUE);
            assert!(t.radius.card <= t.radius.surface);
            if t.glass.is_some() {
                assert_eq!(t.border, None, "{style:?}: one border, the engine's");
            }
        }
        assert_eq!(Radii::concentric(24.0, 12.0, 4.0), 12.0);
        assert_eq!(Radii::concentric(8.0, 8.0, 4.0), 4.0);
    }

    #[test]
    fn small_glass_flips_with_its_backdrop_and_panels_follow_the_scheme() {
        let t = Theme::new(Style::Liquid, Scheme::Dark, SYSTEM_BLUE);
        assert_eq!(t.adaptive(44.0, Some(0.8)), Scheme::Light, "a button over snow");
        assert_eq!(t.adaptive(44.0, Some(0.02)), Scheme::Dark);
        assert_eq!(t.adaptive(380.0, Some(0.8)), Scheme::Dark, "a flyout");
        assert_eq!(t.adaptive(44.0, None), Scheme::Dark, "no backdrop known");
    }

    /// The user's controls reach the glass unclamped in every glass style: blur 0 is no frost,
    /// clarity 1 has no tint left, and the surface's own default is left exactly as it was.
    #[test]
    fn knobs_drive_every_glass_style_to_both_ends() {
        let tint = hex(0x17171A, 0.46);
        for style in [Style::Liquid, Style::OneUi, Style::Fluent] {
            let base = material(style, tint).unwrap();
            assert_eq!(base.tuned(style, &Knobs::default()), base, "{style:?}: unset keeps it");
            let clear = base.tuned(style, &Knobs { clarity: Some(1.0), blur: Some(0.0), ..Default::default() });
            assert_eq!((clear.clarity, clear.blur, clear.tint.a), (1.0, 0.0, 0.0), "{style:?}");
            let tinted = base.tuned(style, &Knobs { clarity: Some(0.0), blur: Some(32.0), ..Default::default() });
            assert_eq!((tinted.clarity, tinted.blur), (0.0, 32.0), "{style:?}");
            assert!(tinted.tint.a > base.tint.a, "{style:?}: more tinted than its default");
            let lens = base.tuned(style, &Knobs { refraction: Some(1.0), highlight: Some(1.0), ..Default::default() });
            assert_eq!(lens.refraction > 0.0, lensed(style), "{style:?}: only Liquid bends");
        }
    }

    #[test]
    fn apple_springs_match_liquid_rust() {
        // ω₀ = 2π/0.5, ζ = 0.85 for .snappy.
        let s = Spring::SNAPPY;
        let w0 = std::f32::consts::TAU / 0.5;
        assert!((s.stiffness - w0 * w0).abs() < 0.01);
        assert!((s.damping - 2.0 * 0.85 * w0).abs() < 0.01);
        assert!(Spring::SMOOTH.at(0.5) > 0.98, "settles in about its duration");
    }
}
