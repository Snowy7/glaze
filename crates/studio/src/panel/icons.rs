//! Real installed app artwork comes from the shell's icon service; built-ins use kit icons.
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

pub fn image(app: &App, path: &str) -> Option<std::sync::Arc<Image>> {
    if app.fixture {
        // Deterministic fixtures use shipped artwork for this app, and cached system icons.
        let cache = mypaper_bars::files::local("icons")
            .ok()?
            .join(mypaper_bars::icons::cache_name(&path.to_lowercase()));
        return app.images.borrow_mut().get(&cache, (48, 48));
    }
    let file = app
        .stage
        .borrow_mut()
        .app_icon(app.thread, mypaper_bars::icons::Source::Path(path.into()))?;
    app.images.borrow_mut().get(&file, (48, 48))
}

pub fn plugin_image(app: &App, id: &str) -> Option<std::sync::Arc<Image>> {
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
    let path = shortcuts()
        .get(&name)
        .or_else(|| shortcuts().get(&plugin.to_lowercase()))?;
    image(app, &path.to_string_lossy())
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
    let start = c.y;
    let image = plugin_image(app, id);
    super::controls::row(
        b,
        c,
        Some(fallback),
        label,
        "",
        caption,
        Some(checked),
        action,
    );
    if let Some(image) = image {
        let d = b.px(if caption.is_empty() { 18.0 } else { 28.0 });
        let r = Rect::new(c.x, start + (c.y - start - d) / 2.0, d, d);
        b.fill(r, b.px(5.0), b.p.panel);
        crate::stage::image_round(b, r, &image, 1.0, b.px(4.0));
    }
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
