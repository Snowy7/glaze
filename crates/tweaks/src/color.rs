//! Color math and the encodings Windows uses for them. All of these are
//! pure, so the byte orders (which are easy to get backwards) are tested.

use glaze_theme::Color;

/// `COLORREF` as `DwmSetWindowAttribute` wants it: `0x00BBGGRR`.
pub fn colorref(Color([r, g, b]): Color) -> u32 {
    u32::from_le_bytes([r, g, b, 0])
}

/// The accent registry values (`AccentColor`, `AccentColorMenu`): `0xAABBGGRR`, opaque.
pub fn accent_abgr(Color([r, g, b]): Color) -> u32 {
    u32::from_le_bytes([r, g, b, 0xFF])
}

/// `ColorizationColor` in the DWM key: `0xAARRGGBB`, with the alpha Windows itself writes.
pub fn colorization_argb(Color([r, g, b]): Color) -> u32 {
    u32::from_be_bytes([0xC4, r, g, b])
}

/// The `AccentPalette` blob: eight `R G B 00` swatches. Windows stores the
/// accent in the fourth slot, three lighter shades before it and three
/// darker after it, then a fixed orange that is not part of the ramp.
pub fn accent_palette(accent: Color) -> [u8; 32] {
    const LIGHTEN: [f32; 3] = [0.66, 0.40, 0.13];
    const DARKEN: [f32; 3] = [0.90, 0.69, 0.48];
    const FIXED: [u8; 3] = [0xF7, 0x63, 0x0C];

    let base = Hsl::from(accent);
    let shades = LIGHTEN
        .map(|t| base.with_lightness(base.l + (1.0 - base.l) * t))
        .into_iter()
        .chain([base])
        .chain(DARKEN.map(|k| base.with_lightness(base.l * k)))
        .map(Color::from)
        .map(|Color(rgb)| rgb)
        .chain([FIXED]);

    let mut out = [0u8; 32];
    for (slot, [r, g, b]) in out.as_chunks_mut::<4>().0.iter_mut().zip(shades) {
        slot.copy_from_slice(&[r, g, b, 0]);
    }
    out
}

/// `"#rrggbb"`, the form theme files and `palette.json` use.
pub fn hex(Color([r, g, b]): Color) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Hue in degrees (`0..360`), saturation and lightness in `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

impl Hsl {
    pub fn with_lightness(self, l: f32) -> Self {
        Self {
            l: l.clamp(0.0, 1.0),
            ..self
        }
    }
}

impl From<Color> for Hsl {
    fn from(Color(rgb): Color) -> Self {
        let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let delta = max - min;
        if delta == 0.0 {
            return Self { h: 0.0, s: 0.0, l };
        }
        let s = delta / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        Self { h: h * 60.0, s, l }
    }
}

impl From<Hsl> for Color {
    fn from(Hsl { h, s, l }: Hsl) -> Self {
        let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let sector = h.rem_euclid(360.0) / 60.0;
        let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
        let (r, g, b) = match sector as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = l - chroma / 2.0;
        Color([r, g, b].map(|c| ((c + m) * 255.0).round().clamp(0.0, 255.0) as u8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORANGE: Color = Color([0xFF, 0x88, 0x00]);

    #[test]
    fn encodings_put_the_channels_where_windows_expects_them() {
        assert_eq!(colorref(ORANGE), 0x0000_88FF);
        assert_eq!(accent_abgr(ORANGE), 0xFF00_88FF);
        assert_eq!(colorization_argb(ORANGE), 0xC4FF_8800);
        assert_eq!(hex(ORANGE), "#ff8800");
    }

    #[test]
    fn accent_palette_is_a_light_to_dark_ramp_around_the_accent() {
        let blue = Color([0x00, 0x78, 0xD4]);
        let palette = accent_palette(blue);
        let swatch = |i: usize| Color([palette[i * 4], palette[i * 4 + 1], palette[i * 4 + 2]]);
        assert_eq!(swatch(3), blue);
        assert!(palette.as_chunks::<4>().0.iter().all(|s| s[3] == 0));
        assert_eq!(swatch(7), Color([0xF7, 0x63, 0x0C]));
        let lightness: Vec<f32> = (0..7).map(|i| Hsl::from(swatch(i)).l).collect();
        assert!(lightness.windows(2).all(|w| w[0] > w[1]), "{lightness:?}");
    }

    #[test]
    fn hsl_round_trips() {
        for color in [
            ORANGE,
            Color([0, 0, 0]),
            Color([255; 3]),
            Color([12, 200, 99]),
        ] {
            let back = Color::from(Hsl::from(color));
            for (a, b) in color.0.into_iter().zip(back.0) {
                assert!(a.abs_diff(b) <= 1, "{color:?} -> {back:?}");
            }
        }
    }
}
