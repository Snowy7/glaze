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
}

impl Default for Tokens {
    /// The built-in look: a dark tint with light text.
    fn default() -> Self {
        Self {
            font: TextStyle::new("Segoe UI Variable Text", 14.0),
            icons: crate::icons::SYSTEM_FAMILY.into(),
            colors: Colors::new(true, [0xff; 3]),
            material: Material {
                tint: Rgba::rgb8([0x16, 0x18, 0x1d], 0.94),
            },
            radius: 10.0,
            border: true,
            shadow: true,
            scale: 1.0,
            menu: MenuMetrics::default(),
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
    /// Light text on a dark surface (`dark`) or dark text on a light one, with `accent`
    /// for highlights.
    pub fn new(dark: bool, accent: [u8; 3]) -> Self {
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
            text_dim: text.with_alpha(0.38),
            hover: Rgba::rgb8(accent, if dark { 0.20 } else { 0.16 }),
            line: edge,
            shadow: Rgba::new(0.0, 0.0, 0.0, 0.05),
        }
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
