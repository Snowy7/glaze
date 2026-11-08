//! User icon packs and cached raster/SVG decoding, at the exact device size an icon is drawn
//! (`Images::sized`). All SVG work is CPU-only and done once per size.
use glaze_bars::{apps::Item, icon_pack::IconPack};
use glaze_ui::{Image, Rgba, Ui};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Pack {
    dir: PathBuf,
    data: IconPack,
}
#[derive(Default)]
pub struct Images {
    images: HashMap<String, Option<Arc<Image>>>,
    pack: Option<Pack>,
    pub theme: PathBuf,
}
impl Images {
    pub fn configure(&mut self, pack: Option<&str>, theme: PathBuf) {
        self.theme = theme;
        self.images.clear();
        self.pack = pack
            .and_then(|name| {
                glaze_config::dir()
                    .ok()
                    .map(|p| p.join("icons").join(name))
            })
            .and_then(|dir| IconPack::load(&dir).ok().map(|data| Pack { dir, data }))
            .or_else(|| {
                (pack == Some("mono")).then(|| Pack {
                    dir: PathBuf::from("@builtin/mono"),
                    data: glaze_bars::icon_pack::bundled_mono(),
                })
            });
    }
    pub fn app(&self, app: &Item) -> Option<(String, bool)> {
        let p = self.pack.as_ref()?;
        let file = |s: &str| s.rsplit(['\\', '/']).next().unwrap_or(s).to_lowercase();
        let entry = p
            .data
            .entries
            .iter()
            .filter_map(|e| {
                let rank = if e.umid.as_ref().is_some_and(|u| {
                    app.aumid
                        .as_ref()
                        .is_some_and(|v| u.eq_ignore_ascii_case(v))
                }) {
                    4
                } else if e.path.as_ref().is_some_and(|path| {
                    path.eq_ignore_ascii_case(&app.path)
                        || app
                            .exe
                            .as_ref()
                            .is_some_and(|exe| exe.eq_ignore_ascii_case(path))
                }) {
                    3
                } else if e.path.as_ref().is_some_and(|path| {
                    file(path) == path.to_lowercase()
                        && (file(&app.path) == file(path)
                            || app.exe.as_ref().is_some_and(|exe| file(exe) == file(path)))
                }) {
                    2
                } else if e.extension.as_ref().is_some_and(|x| {
                    Path::new(&app.path)
                        .extension()
                        .is_some_and(|v| v.eq_ignore_ascii_case(x))
                }) {
                    1
                } else {
                    0
                };
                (rank > 0).then_some((rank, e))
            })
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, e)| e);
        entry
            .map(|e| {
                (
                    p.dir.join(&e.icon).display().to_string(),
                    e.mono.unwrap_or(p.data.mono),
                )
            })
            .or_else(|| {
                p.data
                    .missing
                    .as_ref()
                    .map(|f| (p.dir.join(f).display().to_string(), p.data.mono))
            })
    }
    pub fn key(&self, key: &str) -> Option<(String, bool)> {
        let p = self.pack.as_ref()?;
        let e = p
            .data
            .entries
            .iter()
            .find(|e| e.key.as_deref() == Some(key))?;
        Some((
            p.dir.join(&e.icon).display().to_string(),
            e.mono.unwrap_or(p.data.mono),
        ))
    }
    /// A module glyph the user's own icon pack draws. The bundled mono pack draws app icons
    /// only: the bars' module glyphs are the theme's icons.
    pub fn custom(&self, key: &str) -> Option<(String, bool)> {
        self.pack
            .as_ref()
            .filter(|p| !p.dir.starts_with("@builtin"))?;
        self.key(key)
    }
    pub fn glyph(&self, key: &str, set: glaze_bars::style::Glyphs) -> Option<(String, bool)> {
        self.key(key).or_else(|| {
            glaze_bars::glyphs::svg(set, key).map(|_| {
                (
                    format!(
                        "@glyph/{}/{key}.svg",
                        if set == glaze_bars::style::Glyphs::Symbols {
                            "symbols"
                        } else {
                            "default"
                        }
                    ),
                    true,
                )
            })
        })
    }
    pub fn get(&mut self, ui: &Ui, source: &str) -> Option<Arc<Image>> {
        self.load(ui, source, None)
    }
    /// `source` decoded once (an SVG at 96 px), recoloured with `tint` when given.
    pub fn load(&mut self, ui: &Ui, source: &str, tint: Option<Rgba>) -> Option<Arc<Image>> {
        self.cached(ui, source, tint, None)
    }

    /// `source` at exactly `px` device pixels on its longer side, so it is drawn 1:1 and stays
    /// crisp at any DPI: an SVG rasterized at that size, a larger bitmap area-averaged down to
    /// it. A smaller bitmap comes back as it is (the renderer scales it up).
    pub fn sized(
        &mut self,
        ui: &Ui,
        source: &str,
        tint: Option<Rgba>,
        px: u32,
    ) -> Option<Arc<Image>> {
        self.cached(ui, source, tint, Some(px.max(1)))
    }

    fn cached(
        &mut self,
        ui: &Ui,
        source: &str,
        tint: Option<Rgba>,
        px: Option<u32>,
    ) -> Option<Arc<Image>> {
        let key = format!("{source}:{tint:?}:{px:?}");
        if let Some(image) = self.images.get(&key) {
            return image.clone();
        }
        let path = Path::new(source);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.theme.join(path)
        };
        let raster = px.unwrap_or(96);
        let mut image = if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("svg"))
        {
            if let Some(file) = source.replace('\\', "/").strip_prefix("@builtin/mono/") {
                glaze_bars::icon_pack::bundled_mono_icon(file).and_then(|b| svg_bytes(b, raster))
            } else if let Some(source) = source.strip_prefix("@glyph/") {
                let (set, file) = source.split_once('/')?;
                glaze_bars::glyphs::svg(
                    if set == "symbols" {
                        glaze_bars::style::Glyphs::Symbols
                    } else {
                        glaze_bars::style::Glyphs::Default
                    },
                    file.strip_suffix(".svg")?,
                )
                .and_then(|b| svg_bytes(b, raster))
            } else {
                std::fs::read(&path)
                    .ok()
                    .and_then(|b| svg_bytes(&b, raster))
            }
        } else {
            Image::load(ui, &path)
                .ok()
                .map(|image| px.map_or_else(|| image.clone(), |px| shrink(&image, px)))
        };
        if let (Some(image), Some(color)) = (&mut image, tint) {
            for pixel in image.pixels.as_chunks_mut::<4>().0 {
                let alpha = pixel[3] as f32 * color.a;
                pixel[0] = (color.b * alpha).round() as u8;
                pixel[1] = (color.g * alpha).round() as u8;
                pixel[2] = (color.r * alpha).round() as u8;
                pixel[3] = alpha.round() as u8;
            }
        }
        let image = image.map(Arc::new);
        if self.images.len() >= 192 {
            self.images.clear();
        }
        self.images.insert(key, image.clone());
        image
    }
}

/// An SVG rasterized with its longer side `px` pixels.
fn svg_bytes(bytes: &[u8], px: u32) -> Option<Image> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    let (width, height) = (
        (size.width() * scale).round().max(1.0) as u32,
        (size.height() * scale).round().max(1.0) as u32,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some(Image {
        width,
        height,
        pixels,
    })
}

/// `image` (premultiplied BGRA) area-averaged down so its longer side is `px`; as it is when it
/// already fits. Each target pixel is the coverage-weighted mean of the source pixels under it,
/// so a 256 px app icon drawn at 48 keeps clean edges where bilinear sampling would alias.
fn shrink(image: &Image, px: u32) -> Image {
    let longer = image.width.max(image.height);
    if longer <= px {
        return image.clone();
    }
    let scale = px as f32 / longer as f32;
    let w = ((image.width as f32 * scale).round() as u32).max(1);
    let h = ((image.height as f32 * scale).round() as u32).max(1);
    // For each target column (row): the first source index and the weights of those it covers.
    let spans = |from: u32, to: u32| -> Vec<(usize, Vec<f32>)> {
        let step = from as f32 / to as f32;
        (0..to)
            .map(|i| {
                let (a, b) = (i as f32 * step, (i + 1) as f32 * step);
                let first = a.floor() as usize;
                let last = (b.ceil() as usize).min(from as usize);
                let weights = (first..last)
                    .map(|j| ((j + 1) as f32).min(b) - (j as f32).max(a))
                    .collect();
                (first, weights)
            })
            .collect()
    };
    let (columns, rows) = (spans(image.width, w), spans(image.height, h));
    let (sw, tw) = (image.width as usize, w as usize);
    // Across, into f32 rows; then down.
    let mut wide = vec![0f32; tw * image.height as usize * 4];
    for y in 0..image.height as usize {
        for (x, (first, weights)) in columns.iter().enumerate() {
            let total: f32 = weights.iter().sum();
            for (k, weight) in weights.iter().enumerate() {
                let src = (y * sw + first + k) * 4;
                for c in 0..4 {
                    wide[(y * tw + x) * 4 + c] += image.pixels[src + c] as f32 * weight / total;
                }
            }
        }
    }
    let mut pixels = vec![0u8; tw * h as usize * 4];
    for (y, (first, weights)) in rows.iter().enumerate() {
        let total: f32 = weights.iter().sum();
        for x in 0..tw {
            for c in 0..4 {
                let v: f32 = weights
                    .iter()
                    .enumerate()
                    .map(|(k, weight)| wide[((first + k) * tw + x) * 4 + c] * weight / total)
                    .sum();
                pixels[(y * tw + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Image {
        width: w,
        height: h,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_icon_shrinks_to_the_exact_size_and_keeps_its_edges() {
        // 4x4 opaque white in an 8x8 transparent field, halved: 2x2 white in 4x4.
        let mut pixels = vec![0u8; 8 * 8 * 4];
        for y in 2..6 {
            for x in 2..6 {
                pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&[255; 4]);
            }
        }
        let image = Image {
            width: 8,
            height: 8,
            pixels,
        };
        let small = shrink(&image, 4);
        assert_eq!((small.width, small.height), (4, 4));
        let alpha = |x: usize, y: usize| small.pixels[(y * 4 + x) * 4 + 3];
        assert_eq!((alpha(1, 1), alpha(2, 2), alpha(0, 0)), (255, 255, 0));
        assert_eq!(shrink(&small, 16).width, 4, "never scaled up");
    }
}
