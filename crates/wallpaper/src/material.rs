//! Our glass material controls as liquid-rust glass (`crates/liquid-rust`, a vendored copy of
//! al1h3n/liquid-rust): one mapping for plates (`glass_pass`), overlays (`overlay::glass`) and
//! Studio's previews (`preview`). See `docs/glass.md`.

use liquid_rust as lr;
use mypaper_glass_ipc::Material;

/// The highlight control at 1 is the kit's Clear glass (Figma *Light* 40 %); the bars' default
/// 0.75 is then exactly the kit's regular 30 %.
const MAX_SPECULAR: f32 = 0.4;

/// Our five material controls (plan, "Material controls") as liquid-rust glass at `scale` px
/// per point. The base is Apple's `Glass.regular` (bars, menus, anything with text) and each
/// control drives one of its knobs, which are named after Figma's GLASS panel:
/// - clarity, clear (1) to tinted (0): iOS 27's Liquid Glass slider, `transparency = 1 - clarity`;
/// - blur (px): *Frost*, in points;
/// - refraction: *Refraction*;
/// - highlight: *Light* (the specular rim), up to [`MAX_SPECULAR`];
/// - tint: `Glass.tint`, stained glass whose strength is the tint's alpha.
///
/// The tint's colour also picks the colour scheme, because labels on our glass follow the
/// surface's colours, not the content: a dark tint carries light text, so its glass takes the
/// dark face. That is why glass never flips with the content (`adapt_to_content` off).
pub fn liquid(m: &Material, scale: f32) -> (lr::Material, lr::Appearance) {
    let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let [r, g, b, a] = m.tint.map(unit);
    let regular = lr::Material::regular();
    // The lens's dispersion follows refraction, and the inner rims and the edge hairline follow
    // the highlight, so both at 0 is flat frosted glass (One UI, Fluent's acrylic). Liquid's
    // values (refraction from 0.5, highlight from 0.75) keep the kit's exactly.
    let lens = (unit(m.refraction) / 0.5).min(1.0);
    let light = (unit(m.highlight) / 0.75).min(1.0);
    let material = lr::Material {
        frost: (m.blur.max(0.0) / scale.max(0.5)).min(64.0),
        refraction: unit(m.refraction),
        dispersion: regular.dispersion * lens,
        specular: unit(m.highlight) * MAX_SPECULAR,
        rim: regular.rim * light,
        rim_shade: regular.rim_shade * light,
        edge: regular.edge * light,
        tint: (a > 0.0).then_some(lr::Color { r, g, b, a }),
        ..regular
    };
    let appearance = lr::Appearance {
        dark: 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5,
        transparency: 1.0 - unit(m.clarity),
        adapt_to_content: false,
        ..Default::default()
    };
    (material, appearance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_drive_the_kits_knobs() {
        let clear = Material {
            clarity: 1.0,
            blur: 15.0,
            refraction: 0.7,
            highlight: 0.75,
            tint: [0.1, 0.1, 0.12, 0.0],
        };
        let (m, a) = liquid(&clear, 1.5);
        assert_eq!((m.frost, m.refraction, m.tint), (10.0, 0.7, None), "15 px is the kit's 10 pt frost");
        assert!((m.specular - lr::Material::regular().specular).abs() < 1e-6);
        assert_eq!((a.transparency, a.dark, a.adapt_to_content), (0.0, true, false));
        let tinted = Material {
            clarity: 0.0,
            tint: [1.0, 1.0, 1.0, 0.4],
            ..clear
        };
        let (m, a) = liquid(&tinted, 1.5);
        assert_eq!((a.transparency, a.dark), (1.0, false));
        assert_eq!(m.tint.map(|t| t.a), Some(0.4));
    }

    #[test]
    fn no_refraction_and_no_highlight_is_flat_frosted_glass() {
        let acrylic = Material {
            clarity: 0.12,
            blur: 45.0,
            refraction: 0.0,
            highlight: 0.0,
            tint: [0.12, 0.12, 0.12, 0.6],
        };
        let (m, _) = liquid(&acrylic, 1.5);
        assert_eq!((m.dispersion, m.specular, m.rim, m.rim_shade, m.edge), (0.0, 0.0, 0.0, 0.0, 0.0));
        let (kit, _) = liquid(&Material::default(), 1.5);
        let regular = lr::Material::regular();
        assert_eq!((kit.dispersion, kit.rim, kit.edge), (regular.dispersion, regular.rim, regular.edge));
    }
}
