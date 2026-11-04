//! User icon packs and cached raster/SVG decoding. All SVG work is CPU-only and done once.
use mypaper_bars::{apps::Item, icon_pack::IconPack};
use mypaper_ui::{Image, Rgba, Ui};
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
                mypaper_config::dir()
                    .ok()
                    .map(|p| p.join("icons").join(name))
            })
            .and_then(|dir| IconPack::load(&dir).ok().map(|data| Pack { dir, data }));
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
    pub fn get(&mut self, ui: &Ui, source: &str) -> Option<Arc<Image>> {
        self.load(ui, source, None)
    }
    pub fn load(&mut self, ui: &Ui, source: &str, tint: Option<Rgba>) -> Option<Arc<Image>> {
        let key = format!("{source}:{tint:?}");
        if let Some(image) = self.images.get(&key) {
            return image.clone();
        }
        let path = Path::new(source);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.theme.join(path)
        };
        let mut image = if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("svg"))
        {
            svg(&path)
        } else {
            Image::load(ui, &path).ok()
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

fn svg(path: &Path) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = 96.0 / size.width().max(size.height());
    let (width, height) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
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
