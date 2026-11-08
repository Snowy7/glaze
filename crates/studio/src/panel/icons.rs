//! Real installed app artwork comes from the shell's icon service; built-ins use kit icons.
use crate::panel::tokens::radius;
use crate::{
    actions::Action,
    app::App,
    ui::{B, Col},
};
use mypaper_ui::controls::Glyph;
use mypaper_ui::{Image, Rect};
use std::{collections::BTreeMap, path::PathBuf, sync::OnceLock};

fn shortcuts() -> &'static BTreeMap<String, PathBuf> {
    static APPS: OnceLock<BTreeMap<String, PathBuf>> = OnceLock::new();
    APPS.get_or_init(|| {
        let mut apps = BTreeMap::new();
        let mut dirs = mypaper_bars::icons::start_menu_dirs();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        apps.insert(name.to_lowercase(), path);
                    }
                }
            }
        }
        apps
    })
}

/// Installed Start menu apps, kept in the same read-only catalog as the icon resolver.
pub fn apps() -> Vec<(String, String)> {
    shortcuts()
        .values()
        .filter_map(|path| {
            Some((
                path.file_stem()?.to_str()?.to_owned(),
                path.display().to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
pub fn fixture_pins() -> Vec<serde_json::Value> {
    let mut pins = vec![];
    for (name, path) in [
        ("File Explorer", r"C:\Windows\explorer.exe"),
        (
            "Microsoft Edge",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ),
        ("Notepad", r"C:\Windows\System32\notepad.exe"),
    ] {
        if std::path::Path::new(path).is_file() {
            pins.push(serde_json::json!({"name":name,"path":path}));
        }
    }
    for name in ["OBS Studio", "Discord", "Docker Desktop", "Tailscale"] {
        if let Some(path) = shortcuts().get(&name.to_lowercase()) {
            pins.push(serde_json::json!({"name":name,"path":path}));
        }
    }
    pins
}

pub fn image(app: &App, path: &str) -> Option<std::sync::Arc<Image>> {
    if app.fixture {
        // Fixture rendering extracts the same real artwork without writing the icon cache.
        let key = format!("app-icon:{path}");
        let state = app.panel.as_ref().unwrap();
        if let Some(image) = state.specimens.borrow().get(&key) {
            return Some(image.clone());
        }
        let image = std::sync::Arc::new(system_image(path)?);
        state.specimens.borrow_mut().insert(key, image.clone());
        return Some(image);
    }
    let file = app
        .stage
        .borrow_mut()
        .app_icon(app.thread, mypaper_bars::icons::Source::Path(path.into()))?;
    app.images.borrow_mut().get(&file, (48, 48))
}

pub fn plugin_image(app: &App, id: &str) -> Option<std::sync::Arc<Image>> {
    // Built-in module/widget IDs are nouns, not app identities ("app" is not EA App).
    if !id.starts_with("custom:") && !id.contains("--") {
        return None;
    }
    let plugin = id
        .strip_prefix("custom:")
        .unwrap_or(id)
        .split("--")
        .next()
        .unwrap_or(id);
    let name = app
        .plugins
        .installed
        .iter()
        .find(|p| p.id == plugin)
        .map_or(plugin, |p| p.name.as_str())
        .to_lowercase();
    // Prefer package artwork when it exists. Never guess another app's picture.
    for root in mypaper_widgets::registry::plugin_roots() {
        for file in ["icon.png", "assets/icon.png", "logo.png"] {
            let path = root.join(plugin).join(file);
            if path.is_file() {
                return app.images.borrow_mut().get(&path, (48, 48));
            }
        }
    }
    let exact = shortcuts()
        .get(&name)
        .or_else(|| shortcuts().get(&plugin.to_lowercase()));
    let path = exact.or_else(|| {
        // App channels and distribution names may add a suffix, e.g. “T3 Code (Nightly)”.
        let mut matches = shortcuts().iter().filter(|(title, _)| {
            title.starts_with(&format!("{name} ")) || title.starts_with(&format!("{plugin} "))
        });
        let first = matches.next()?.1;
        matches.next().is_none().then_some(first)
    })?;
    image(app, &path.to_string_lossy())
}

/// A component face's declared kit icon when the package has no raster artwork.
pub fn plugin_glyph(app: &App, id: &str) -> Glyph {
    use mypaper_ui::icons::Icon;
    let Some((plugin, face)) = id.split_once("--") else {
        return crate::editing::widgets::glyph_of(id);
    };
    let manifest = app
        .plugins
        .installed
        .iter()
        .find(|manifest| manifest.id == plugin)
        .cloned()
        .or_else(|| mypaper_widgets::registry::find_plugin(plugin).map(|(manifest, _)| manifest));
    let icon = manifest
        .as_ref()
        .and_then(|manifest| manifest.faces.iter().find(|item| item.id == face))
        .and_then(|face| face.icon.as_deref());
    match icon {
        Some("cloud-sun") => Icon::CloudSun,
        Some("cloud") => Icon::Cloud,
        Some("sun") => Icon::Sun,
        Some("terminal") => Icon::Terminal,
        Some("video") => Icon::Video,
        Some("message") => Icon::Message,
        Some("globe") => Icon::Globe,
        Some("layers") => Icon::Layers,
        Some("sparkles") => Icon::Sparkles,
        Some("activity") => Icon::Activity,
        Some("gauge") => Icon::Gauge,
        Some("clock") => Icon::Clock,
        Some("calendar") => Icon::Calendar,
        Some("package") => Icon::Package,
        _ => return crate::editing::widgets::glyph_of(plugin),
    }
    .into()
}

/// Read a real shell icon into premultiplied BGRA. Used only by fixture rendering, with no
/// windows, worker, cache files, or changes to the source app.
fn system_image(path: &str) -> Option<Image> {
    use windows::Win32::{
        Foundation::SIZE,
        Graphics::Gdi::*,
        UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY},
    };
    // SAFETY: Ui initialized COM on this render thread. Every GDI object is released before
    // returning, including failed reads; buffers match the returned bitmap dimensions.
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&windows::core::HSTRING::from(path), None).ok()?;
        let bitmap = factory
            .GetImage(SIZE { cx: 48, cy: 48 }, SIIGBF_ICONONLY)
            .ok()?;
        let mut info = BITMAP::default();
        let read = GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some((&raw mut info).cast()),
        );
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        if read == 0 || width <= 0 || height <= 0 || width > 256 || height > 256 {
            let _ = DeleteObject(bitmap.into());
            return None;
        }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bitmap.into());
        if rows != height {
            return None;
        }
        if pixels.chunks_exact(4).all(|p| p[3] == 0) {
            for p in pixels.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
        Some(Image {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    }
}

pub fn row(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: &str,
    fallback: Glyph,
    label: &str,
    caption: &str,
    checked: bool,
    action: Action,
) {
    let image = plugin_image(app, id);
    let h = super::controls::height(b, if caption.is_empty() { 40.0 } else { 72.0 });
    let r = Rect::new(c.x, c.y, c.w, h);
    let k = mypaper_ui::controls::key(("catalog-row", c.view, id));
    let area = b.list_row_area(r);
    if checked {
        b.fill(area, b.px(radius::CONTROL), b.p.selected);
    } else {
        b.wash(k, area, b.px(radius::CONTROL));
    }
    let icon = Rect::new(r.left, r.top + (h - b.px(28.0)) / 2.0, b.px(28.0), b.px(28.0));
    if let Some(image) = image {
        crate::stage::image_round(b, icon, &image, 1.0, b.px(4.0));
    } else {
        b.icon(icon, fallback, 16.0, b.p.ink2);
    }
    super::controls::text(
        b,
        Rect::new(
            r.left + b.px(40.0),
            r.top + b.px(5.0),
            r.width() - b.px(64.0),
            b.px(24.0),
        ),
        label,
        13.0,
        500,
        b.p.ink1,
        mypaper_ui::controls::Align::Start,
    );
    let font = super::controls::font(b, 12.0, 400);
    let lines = b.text.lines(caption, &font, r.width() - b.px(54.0), 2);
    for (i, line) in lines.iter().enumerate() {
        b.text_at(
            r.left + b.px(40.0),
            r.top + b.px(29.0 + i as f32 * 18.0),
            line,
            &font,
            b.p.ink3,
        );
    }
    b.hit_checked(
        k,
        area,
        mypaper_ui::Role::RadioButton,
        label,
        Some(checked),
        action,
        mypaper_ui::controls::Behavior::Click,
    );
    b.focus_ring(k, area, b.px(radius::CONTROL));
    c.y = r.bottom;
}

pub fn module(id: &str) -> Glyph {
    use crate::theme::glyph as g;
    match id {
        "start" => g::START,
        "search" => g::SEARCH,
        "workspaces" => g::LAYERS,
        "app" => g::APP,
        "clock" => g::CLOCK,
        "media" => g::MUSIC,
        "tray" => g::CHEVRON_DOWN,
        "network" => g::WIFI,
        "volume" => g::SOUND,
        "battery" => g::BATTERY,
        "quick" => g::TOGGLES,
        "power" => g::POWER,
        "stats" => g::CHIP,
        "language" => g::KEYBOARD,
        "notifications" => g::BELL,
        _ => g::APP,
    }
}
