//! Style tokens: type, colours, material and metrics that views read instead of constants.
//!
//! Values are in design pixels (96 DPI, content scale 1); a view multiplies them by its surface's
//! scale. This holds what menus need today; styles (N6) add their metrics here.

use std::sync::Arc;

use crate::geom::Rgba;
use crate::text::TextStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct Tokens {
    /// Body text.
    pub font: TextStyle,
    /// The icon font family (glyphs from [`crate::icons`]).
    pub icons: Arc<str>,
    pub colors: Colors,
    pub material: Material,
    /// Corner radius of a surface.
    pub radius: f32,
    /// A hairline around a surface.
    pub border: bool,
    /// A soft drop shadow around a surface.
    pub shadow: bool,
    /// The theme's content scale, applied on top of the monitor's DPI (0.5..=2).
    pub scale: f32,
    pub menu: MenuMetrics,
    /// Engine glass under the body instead of the tint (`crate::glass`); `None` draws the tint.
    /// Popups fall back to the thread's style ([`crate::glass::style`]) when this is `None`.
    pub glass: Option<crate::glass::GlassMaterial>,
}

impl Default for Tokens {
    /// The built-in look: a dark tint with light text.
    fn default() -> Self {
        Self {
            font: TextStyle::new("Segoe UI Variable Text", 14.0),
            icons: crate::icons::SYSTEM_FAMILY.into(),
            colors: Colors::neutral(true),
            material: Material {
                tint: Rgba::rgb8([0x16, 0x18, 0x1d], 0.94),
            },
            radius: 10.0,
            border: true,
            shadow: true,
            scale: 1.0,
            menu: MenuMetrics::default(),
            glass: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub text: Rgba,
    /// Disabled text.
    pub text_dim: Rgba,
    /// The highlight under a hovered or selected row.
    pub hover: Rgba,
    /// Hairlines: borders and separators.
    pub line: Rgba,
    pub shadow: Rgba,
}

impl Colors {
    /// Light text on a dark surface (`dark`) or dark text on a light one, highlighted with a
    /// neutral wash of the text colour (the monochrome look).
    pub fn neutral(dark: bool) -> Self {
        let (text, edge) = if dark {
            (
                Rgba::new(0.94, 0.94, 0.96, 1.0),
                Rgba::new(1.0, 1.0, 1.0, 0.14),
            )
        } else {
            (
                Rgba::new(0.09, 0.09, 0.11, 1.0),
                Rgba::new(0.0, 0.0, 0.0, 0.14),
            )
        };
        Self {
            text,
            text_dim: text.with_alpha(0.40),
            hover: text.with_alpha(if dark { 0.14 } else { 0.10 }),
            line: edge,
            shadow: Rgba::new(0.0, 0.0, 0.0, 0.05),
        }
    }

    /// [`Colors::neutral`] with an `accent`-tinted highlight (the widgets' look).
    pub fn new(dark: bool, accent: [u8; 3]) -> Self {
        let neutral = Self::neutral(dark);
        Self {
            hover: Rgba::rgb8(accent, if dark { 0.20 } else { 0.16 }),
            text_dim: neutral.text.with_alpha(0.38),
            ..neutral
        }
    }
}

/// Contrast steps for text and glyphs over a surface: labels, details, hints and disabled.
/// The old shell's `--t0..--t3` (1, 0.74, 0.46, 0.24), with the lower two raised so details
/// stay readable over glass and busy wallpapers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub disabled: Rgba,
}

impl Ink {
    /// The steps of `ink` (the surface's text colour).
    pub fn of(ink: Rgba) -> Self {
        let step = |a: f32| ink.with_alpha(ink.a * a);
        Self {
            primary: ink,
            secondary: step(0.74),
            tertiary: step(0.56),
            disabled: step(0.34),
        }
    }
}

/// A step of the shell's type ramp (bar, flyouts, widgets), in design pixels:
/// size / weight. Above 20 px the display cut of the family is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 11 / 500: badges, counts, axis labels.
    Micro,
    /// 12 / 400: details and secondary lines.
    Caption,
    /// 13 / 400: body text, the bar.
    Body,
    /// 13 / 600: titles of rows and cards.
    Label,
    /// 15 / 600: panel and widget titles.
    Headline,
    /// 20 / 600: large values (a temperature in a compact card).
    Title,
    /// 28 / 500: hero values in regular cards.
    Large,
}

impl Step {
    pub const fn metrics(self) -> (f32, u16) {
        match self {
            Self::Micro => (11.0, 500),
            Self::Caption => (12.0, 400),
            Self::Body => (13.0, 400),
            Self::Label => (13.0, 600),
            Self::Headline => (15.0, 600),
            Self::Title => (20.0, 600),
            Self::Large => (28.0, 500),
        }
    }
}

/// The families of the type ramp: a text cut for small sizes and a display cut for large
/// ones (Segoe UI Variable's optical sizes), or one theme font for both.
#[derive(Debug, Clone, PartialEq)]
pub struct Typeface {
    pub text: Arc<str>,
    pub display: Arc<str>,
}

impl Typeface {
    /// Segoe UI Variable where installed (Windows 11), else Segoe UI.
    pub fn system(text: &crate::Text) -> Self {
        let pick = |family: &str| -> Arc<str> {
            if text.has_family(family) {
                family.into()
            } else {
                "Segoe UI".into()
            }
        };
        Self {
            text: pick("Segoe UI Variable Text"),
            display: pick("Segoe UI Variable Display"),
        }
    }

    /// One family for every size (a theme's font).
    pub fn single(family: &str) -> Self {
        Self {
            text: family.into(),
            display: family.into(),
        }
    }

    /// `step` at `scale` device pixels per design pixel.
    pub fn style(&self, step: Step, scale: f32) -> TextStyle {
        let (size, weight) = step.metrics();
        self.sized(size, weight, scale)
    }

    /// Any size: big numerals (a clock, a temperature) use the display cut.
    pub fn sized(&self, size: f32, weight: u16, scale: f32) -> TextStyle {
        let family = if size > 20.0 {
            &self.display
        } else {
            &self.text
        };
        let mut style = TextStyle::new(family, size * scale);
        style.weight = weight;
        style
    }
}

/// What a surface's body is made of. N1 draws a flat tint; the engine's Liquid Glass (N2, N3)
/// adds its controls (clarity, blur, refraction, highlight) here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// The fill, with its opacity as alpha.
    pub tint: Rgba,
}

/// A popup menu's metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuMetrics {
    pub row: f32,
    pub separator: f32,
    /// Inner margin of the panel.
    pub pad: f32,
    /// Room for the check mark, left of the labels.
    pub gutter: f32,
    /// Room for a submenu chevron.
    pub arrow: f32,
    /// Between the widest label and the chevron.
    pub gap: f32,
    pub min_width: f32,
    /// Transparent room around the panel for its shadow.
    pub shadow: f32,
    /// Check mark and chevron size.
    pub glyph: f32,
}

impl Default for MenuMetrics {
    fn default() -> Self {
        Self {
            row: 32.0,
            separator: 9.0,
            pad: 6.0,
            gutter: 30.0,
            arrow: 22.0,
            gap: 24.0,
            min_width: 224.0,
            shadow: 14.0,
            glyph: 12.0,
        }
    }
}

impl MenuMetrics {
    /// Every length times `scale` (device pixels per design pixel).
    pub fn scaled(&self, scale: f32) -> Self {
        Self {
            row: self.row * scale,
            separator: self.separator * scale,
            pad: self.pad * scale,
            gutter: self.gutter * scale,
            arrow: self.arrow * scale,
            gap: self.gap * scale,
            min_width: self.min_width * scale,
            shadow: self.shadow * scale,
            glyph: self.glyph * scale,
        }
    }
}
