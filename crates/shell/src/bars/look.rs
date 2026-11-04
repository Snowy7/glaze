//! Native style tokens. CSS-era decoration is accepted on load and ignored with one warning.
use crate::glass::Material;
use mypaper_bars::{
    accent,
    style::{ColorRef, Family, Fill, Look, NativeStyle, Scheme},
};
use mypaper_theme::{Appearance, TextTone};
use mypaper_ui::{Rgba, TextStyle};

#[derive(Clone)]
pub struct Tokens {
    pub family: Family,
    pub font: TextStyle,
    pub ink: Rgba,
    pub accent: Rgba,
    pub fill: Rgba,
    pub rim: Rgba,
    pub hover: Rgba,
    pub radius: f32,
    pub material: Option<Material>,
}

impl Tokens {
    pub fn resolve(look: &Look, appearance: &Appearance, system: &accent::System) -> Self {
        let style = look.style.unwrap_or_default();
        let dark = match look.scheme {
            Some(Scheme::Dark) => true,
            Some(Scheme::Light) => false,
            Some(Scheme::Auto) => system.scheme == accent::Scheme::Dark,
            None => appearance.text.is_none_or(|t| t == TextTone::Light),
        };
        let ink = Rgba::rgb8(if dark { [245, 245, 247] } else { [25, 25, 28] }, 1.0);
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [23, 24, 28] } else { [243, 243, 247] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let accent = match appearance.accent {
            Some(mypaper_theme::Paint::Color(c)) => Rgba::rgb8(c.0, 1.0),
            _ => hex(&system.accent[0]).unwrap_or(ink),
        };
        let fill = match &look.background {
            Some(Fill::Solid { color }) => resolve_color(color, ink, accent),
            _ => tint.with_alpha(match style {
                NativeStyle::Liquid => 1.0 / 255.0,
                NativeStyle::Mono => look.tint.or(appearance.opacity).unwrap_or(0.8),
                NativeStyle::Solid => 1.0,
            }),
        };
        let material = (style == NativeStyle::Liquid && fill.a < 0.999).then_some(Material {
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
        let mut font = TextStyle::new(
            &font_family(look.font.family.as_deref().unwrap_or("Segoe UI")),
            look.font.size.unwrap_or(12.0),
        );
        font.weight = look.font.weight.unwrap_or(500);
        Self {
            family: look.family.unwrap_or_default(),
            font,
            ink,
            accent,
            fill,
            rim: ink.with_alpha(if style == NativeStyle::Solid {
                0.2
            } else {
                0.16
            }),
            hover: ink.with_alpha(0.12),
            radius: look.radius.or(appearance.radius).unwrap_or(12.0),
            material,
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
                "native bars ignore CSS, gradient/image fills, noise, glow, neon and shaped corners"
            )
        });
    }
}

fn font_family(list: &str) -> String {
    let families: Vec<_> = list
        .split(',')
        .map(|s| s.trim().trim_matches(['\"', '\'']))
        .collect();
    families
        .iter()
        .find(|s| s.starts_with("Segoe UI"))
        .or_else(|| families.first())
        .copied()
        .unwrap_or("Segoe UI")
        .to_owned()
}
