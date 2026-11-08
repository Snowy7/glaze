//! Contrast against the visible glass face, resolved when appearance changes, never from a
//! desktop capture. Backdrop BMPs contain wallpaper only; app windows are never sampled.
use glaze_ui::theme::{contrast, over};
use glaze_ui::{Rgba, Theme};

/// Wallpaper colours retain their hue, with restrained chroma and scheme-appropriate lightness.
pub fn wallpaper_tint(color: glaze_theme::Color, dark: bool) -> glaze_theme::Color {
    let mut lab = to_lab(Rgba::rgb8(color.0, 1.0));
    lab[0] = if dark { 0.30 } else { 0.88 };
    lab[1] *= 0.65;
    lab[2] *= 0.65;
    let rgb = from_lab(lab);
    glaze_theme::Color([rgb.r, rgb.g, rgb.b].map(|v| (v * 255.0).round() as u8))
}

/// Read the wallpaper under a surface (physical virtual-desktop coordinates). Missing frames
/// fall back to the scheme; no file is created and no desktop capture is requested.
pub fn backdrop(rect: Option<glaze_ui::Rect>, dark: bool) -> Rgba {
    let fallback = Rgba::rgb8(if dark { [38, 38, 42] } else { [220, 220, 224] }, 1.0);
    let Some(root) = std::env::var_os("LOCALAPPDATA") else {
        return fallback;
    };
    let monitors = glaze_desktop::monitor_infos();
    let monitor = rect
        .and_then(|r| {
            monitors.iter().find(|m| {
                let (x, y) = ((r.left + r.right) * 0.5, (r.top + r.bottom) * 0.5);
                x >= m.rect.x as f32
                    && x < (m.rect.x + m.rect.width) as f32
                    && y >= m.rect.y as f32
                    && y < (m.rect.y + m.rect.height) as f32
            })
        })
        .or_else(|| monitors.iter().find(|m| m.primary));
    let Some(m) = monitor else { return fallback };
    let name = m.device.trim_start_matches("\\\\.\\");
    let path = std::path::PathBuf::from(root)
        .join("glaze/backdrop")
        .join(format!("{name}.bmp"));
    let Ok(bytes) = std::fs::read(path) else {
        return fallback;
    };
    sample_bmp(
        &bytes,
        rect.map(|r| {
            [
                (r.left - m.rect.x as f32) / m.rect.width as f32,
                (r.top - m.rect.y as f32) / m.rect.height as f32,
                (r.right - m.rect.x as f32) / m.rect.width as f32,
                (r.bottom - m.rect.y as f32) / m.rect.height as f32,
            ]
        }),
    )
    .unwrap_or(fallback)
}

fn sample_bmp(b: &[u8], area: Option<[f32; 4]>) -> Option<Rgba> {
    let int = |i| Some(i32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    if b.get(..2)? != b"BM" || b.get(28..30)? != [24, 0] {
        return None;
    }
    let (offset, w, h) = (usize::try_from(int(10)?).ok()?, int(18)?, int(22)?);
    if w <= 0 || h == 0 || h == i32::MIN || w > 16384 || h.abs() > 16384 {
        return None;
    }
    let (width, height) = (w as usize, h.unsigned_abs() as usize);
    let stride = (width * 3 + 3) & !3;
    b.get(..offset.checked_add(stride.checked_mul(height)?)?)?;
    let a = area.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let mut sum = [0.0; 3];
    for y in 0..8 {
        for x in 0..8 {
            let px = ((a[0] + (a[2] - a[0]) * (x as f32 + 0.5) / 8.0).clamp(0.0, 0.9999)
                * width as f32) as usize;
            let py = ((a[1] + (a[3] - a[1]) * (y as f32 + 0.5) / 8.0).clamp(0.0, 0.9999)
                * height as f32) as usize;
            let row = if h < 0 { py } else { height - 1 - py };
            let p = offset + row * stride + px * 3;
            for i in 0..3 {
                sum[i] += b[p + 2 - i] as f32 / (255.0 * 64.0);
            }
        }
    }
    Some(Rgba::new(sum[0], sum[1], sum[2], 1.0))
}

/// CPU equivalent of regular glass's face fills and stained tint (liquid-rust material.rs and
/// glass.wgsl). Rim highlights are excluded: labels occupy the flat interior of the surface.
pub fn face(m: &super::Material, under: Rgba, min_side: f32) -> Rgba {
    let t = m.tint;
    let dark = 0.2126 * t.r + 0.7152 * t.g + 0.0722 * t.b < 0.5;
    let tau = ((min_side - 44.0) / 276.0).clamp(0.0, 1.0);
    let tau = tau * tau * (3.0 - 2.0 * tau);
    let (lift, lum, mut lm, mut light, cap, mut dim) = if dark {
        (0.03, 0.25, 0.2, 0.0, 0.0, 0.25 + 0.27 * tau)
    } else {
        (0.0, 0.92, 0.3 + 0.15 * tau, 0.2 + 0.55 * tau, 0.75, 0.1)
    };
    let k = 1.0 - 2.0 * m.clarity.clamp(0.0, 1.0);
    let mut cap = cap;
    if k > 0.0 {
        if dark {
            cap = 28.0 / 255.0 * k;
            dim += (1.0 - dim) * 0.6 * k;
        } else {
            light += (1.0 - light) * 0.6 * k;
        }
    } else {
        lm *= 1.0 + 0.45 * k;
        light *= 1.0 + 0.35 * k;
        dim *= 1.0 + 0.45 * k;
    }
    let rgb = [under.r, under.g, under.b].map(|v| (v + lift).min(1.0));
    let lum_of = |c: [f32; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    let mut s = rgb.map(|v| v + lum - lum_of(rgb));
    let (l, lo, hi) = (
        lum_of(s),
        s.into_iter().fold(f32::INFINITY, f32::min),
        s.into_iter().fold(f32::NEG_INFINITY, f32::max),
    );
    if lo < 0.0 {
        s = s.map(|v| l + (v - l) * l / (l - lo).max(1e-5));
    }
    if hi > 1.0 {
        s = s.map(|v| l + (v - l) * (1.0 - l) / (hi - l).max(1e-5));
    }
    let g = [0, 1, 2].map(|i| {
        let v = rgb[i] + (s[i] - rgb[i]) * lm;
        let v = v + (1.0 - v) * light;
        v + (v.min(cap) - v) * dim
    });
    let g = Rgba::new(g[0], g[1], g[2], 1.0);
    let mut lab = to_lab(t);
    lab[0] = (lab[0] + (to_lab(g)[0] - 0.65) * 0.6).clamp(0.15, 0.97);
    // renderer::material_gpu retains 15% of the original face even at full stain strength.
    over(from_lab(lab).with_alpha(t.a.clamp(0.0, 1.0) * 0.85), g)
}

/// Pick the stronger ink and raise alpha only as far as the requested contrast needs.
pub fn ink(surface: Rgba, minimum: f32, alpha: f32) -> Rgba {
    let black = Rgba::new(0.0, 0.0, 0.0, 1.0);
    let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
    let ink = if contrast(black, surface) >= contrast(white, surface) {
        black
    } else {
        white
    };
    let (mut lo, mut hi) = (alpha, 1.0);
    for _ in 0..12 {
        let a = (lo + hi) * 0.5;
        if contrast(over(ink.with_alpha(a), surface), surface) >= minimum {
            hi = a;
        } else {
            lo = a;
        }
    }
    ink.with_alpha(hi)
}

pub fn apply(theme: &mut Theme, surface: Rgba) {
    theme.color.label = ink(surface, 4.5, 0.88);
    theme.color.secondary = ink(surface, 3.0, 0.72);
    theme.color.tertiary = ink(surface, 3.0, 0.56);
}

fn to_lab(c: Rgba) -> [f32; 3] {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = [c.r, c.g, c.b].map(lin);
    let l = (0.41222146 * r + 0.53633255 * g + 0.051445995 * b).cbrt();
    let m = (0.2119035 * r + 0.6806995 * g + 0.10739696 * b).cbrt();
    let s = (0.08830246 * r + 0.28171885 * g + 0.6299787 * b).cbrt();
    [
        0.21045426 * l + 0.7936178 * m - 0.004072047 * s,
        1.9779985 * l - 2.4285922 * m + 0.4505937 * s,
        0.025904037 * l + 0.78277177 * m - 0.80867577 * s,
    ]
}

fn from_lab([l, a, b]: [f32; 3]) -> Rgba {
    let x = (l + 0.39633778 * a + 0.21580376 * b).powi(3);
    let y = (l - 0.105561346 * a - 0.06385417 * b).powi(3);
    let z = (l - 0.08948418 * a - 1.2914855 * b).powi(3);
    let enc = |v: f32| {
        if v <= 0.0031308 {
            12.92 * v.max(0.0)
        } else {
            (1.055 * v.powf(1.0 / 2.4) - 0.055).min(1.0)
        }
    };
    Rgba::new(
        enc(4.0767417 * x - 3.3077116 * y + 0.23096994 * z),
        enc(-1.268438 * x + 2.6097574 * y - 0.3413194 * z),
        enc(-0.0041960863 * x - 0.7034186 * y + 1.7076147 * z),
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ink_flips_with_the_visible_face_instead_of_the_requested_scheme() {
        assert_eq!(ink(Rgba::rgb8([235; 3], 1.0), 4.5, 0.88).r, 0.0);
        assert_eq!(ink(Rgba::rgb8([35; 3], 1.0), 4.5, 0.88).r, 1.0);
    }
    #[test]
    fn ink_meets_contrast_after_material_and_backdrop_composition() {
        for tint in [
            [139, 158, 183],
            [200, 141, 60],
            [127, 174, 142],
            [201, 107, 115],
            [147, 136, 201],
            [230; 3],
            [5, 8, 12],
            [255; 3],
        ] {
            for clarity in [0.0, 0.5, 0.94, 1.0] {
                for bg in [0, 48, 128, 200, 255] {
                    for size in [32.0, 260.0] {
                        let m = super::super::Material {
                            clarity,
                            blur: 12.0,
                            refraction: 0.6,
                            highlight: 0.55,
                            tint: Rgba::rgb8(tint, 1.0 - clarity),
                        };
                        let surface = face(&m, Rgba::rgb8([bg; 3], 1.0), size);
                        for (minimum, alpha) in [(4.5, 0.88), (3.0, 0.72), (3.0, 0.56)] {
                            assert!(
                                contrast(over(ink(surface, minimum, alpha), surface), surface)
                                    >= minimum - 0.002,
                                "{tint:?}, {clarity}, {bg}, {size}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn wallpaper_tint_is_deep_in_dark_scheme_and_light_in_light_scheme() {
        let source = glaze_theme::Color([184, 96, 46]);
        let dark = wallpaper_tint(source, true);
        let light = wallpaper_tint(source, false);
        assert!(glaze_ui::theme::luminance(Rgba::rgb8(dark.0, 1.0)) < 0.06);
        assert!(glaze_ui::theme::luminance(Rgba::rgb8(light.0, 1.0)) > 0.5);
        assert!(dark.0[0] > dark.0[2]);
        assert!(light.0[0] > light.0[2]);
    }
}
