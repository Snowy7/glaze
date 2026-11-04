//! The frontend's whole backend surface. Every command is `async` so Tauri
//! runs it off the UI thread (process and registry calls can take a moment),
//! and every error is the full `anyhow` chain as a string.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_bars_schema::vars::{self, VarDecl};
use mypaper_config::Config;
use mypaper_theme::LoadedTheme;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::autostart::{self, Autostart};
use crate::bars::{self, BarsState, Catalog, Doc};
use crate::config_edit::{self, ConfigPatch, ConfigState};
use crate::engine::{self, Component, EngineStatus};
use crate::iconpacks::{self, AppIcon, PackInfo};
use crate::icons;
use crate::look::Look;
use crate::manifest::{SkinEdit, SkinKind, WindowsStyle};
use crate::plugins;
use crate::source::{self, Kind};
use crate::themes::{self, NewTheme, Origin, ThemeEntry, ThemeList};
use crate::widgets::{self, InstancePatch, Layout, WidgetInfo};

type CmdResult<T> = Result<T, String>;

fn text(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Loads the config, retrying briefly: a watcher event can arrive while a
/// writer (the tray, the engine) is in the middle of its rename.
fn load_config() -> Result<Config> {
    let mut config = Config::load();
    for _ in 0..3 {
        if config.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
        config = Config::load();
    }
    config
}

/// Refuses themes the engine couldn't load, since it stops (rather than
/// crash-loops) on a config it can't use.
fn check_theme(theme: &str) -> Result<()> {
    let dir = mypaper_theme::locate(Some(theme))?;
    LoadedTheme::load(dir)?;
    Ok(())
}

/// Applies a patch to `config.json`, changing only the keys it names (see `config_edit`).
fn update(patch: ConfigPatch) -> Result<Config> {
    if let Some(theme) = &patch.theme {
        check_theme(theme).with_context(|| format!("can't apply theme {theme:?}"))?;
    }
    config_edit::update_at(&mypaper_config::path()?, patch)
}

#[tauri::command]
pub async fn get_config() -> CmdResult<ConfigState> {
    load_config().map(Into::into).map_err(text)
}

#[tauri::command]
pub async fn update_config(patch: ConfigPatch) -> CmdResult<ConfigState> {
    update(patch).map(Into::into).map_err(text)
}

/// Writes the defaults, for when the file on disk can't be parsed.
#[tauri::command]
pub async fn reset_config() -> CmdResult<ConfigState> {
    let config = Config::default();
    config.save().map_err(text)?;
    Ok(config.into())
}

/// The webview may only read preview images of themes we listed.
fn allow_previews(app: &AppHandle, themes: &[ThemeEntry]) {
    let scope = app.asset_protocol_scope();
    for preview in themes.iter().filter_map(|t| t.preview.as_deref()) {
        let _ = scope.allow_file(preview);
    }
}

#[tauri::command]
pub async fn list_themes(app: AppHandle) -> CmdResult<ThemeList> {
    let active = load_config().ok().map(|c| c.theme);
    let list = themes::list(active.as_deref());
    allow_previews(&app, &list.themes);
    Ok(list)
}

#[derive(Debug, Serialize)]
pub struct PresetInfo {
    id: &'static str,
    label: &'static str,
    /// The arguments as they'd be typed, e.g. `-parentHWND {hwnd} delayed`.
    args: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// The absolute path, or the web address.
    source: String,
    kind: Kind,
    uses_fit: bool,
    suggested_name: String,
    /// Argument presets to offer (programs only).
    presets: Vec<PresetInfo>,
    /// Arguments to start with, e.g. the Unity preset when the exe looks like a Unity player.
    default_args: String,
}

impl From<source::Resolved> for SourceInfo {
    fn from(resolved: source::Resolved) -> Self {
        let source::Resolved { kind, source } = resolved;
        let path = Path::new(&source);
        let is_app = kind == Kind::App;
        SourceInfo {
            suggested_name: if kind == Kind::Web && source.contains("://") {
                source::url_name(&source)
            } else {
                source::suggested_name(path)
            },
            uses_fit: kind.uses_fit(),
            presets: if is_app {
                source::ARG_PRESETS
                    .iter()
                    .map(|p| PresetInfo {
                        id: p.id,
                        label: p.label,
                        args: source::join_args(p.args),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            default_args: source::suggested_preset(path)
                .filter(|_| is_app)
                .map_or_else(String::new, |p| source::join_args(p.args)),
            kind,
            source,
        }
    }
}

/// What a picked file or folder would become, or why it can't be a theme. A picked file may then be
/// read by the webview, so New theme can preview the picture or video itself.
#[tauri::command]
pub async fn inspect_source(app: AppHandle, path: PathBuf) -> CmdResult<SourceInfo> {
    let info: SourceInfo = source::resolve(&path.to_string_lossy())
        .map(Into::into)
        .map_err(text)?;
    if path.is_file() {
        let _ = app.asset_protocol_scope().allow_file(&path);
    }
    Ok(info)
}

/// Checks a typed web address.
#[tauri::command]
pub async fn inspect_url(url: String) -> CmdResult<SourceInfo> {
    let url = source::check_url(&url).map_err(text)?;
    source::resolve(url).map(Into::into).map_err(text)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateThemeRequest {
    #[serde(flatten)]
    theme: NewTheme,
    /// Select the new theme right away.
    apply: bool,
}

#[tauri::command]
pub async fn create_theme(app: AppHandle, request: CreateThemeRequest) -> CmdResult<ThemeEntry> {
    let create = || -> Result<ThemeEntry> {
        let root = mypaper_config::user_themes_dir()?;
        let dir = themes::create_in(&root, &request.theme)?;
        let mut entry = themes::entry(&dir, Origin::User, None);
        if request.apply {
            update(ConfigPatch {
                theme: Some(entry.apply_as.clone()),
                ..Default::default()
            })
            .context("the theme was created but could not be applied")?;
            entry.active = true;
        }
        Ok(entry)
    };
    let entry = create().map_err(text)?;
    allow_previews(&app, std::slice::from_ref(&entry));
    Ok(entry)
}

/// Makes the thumbnail for a video theme. The caller re-lists themes afterwards.
#[tauri::command]
pub async fn generate_preview(dir: PathBuf) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || themes::generate_preview(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(text)
}

/// Copies any theme into the user's themes folder so it can be customized.
#[tauri::command]
pub async fn duplicate_theme(app: AppHandle, dir: PathBuf) -> CmdResult<ThemeEntry> {
    let duplicate = || -> Result<ThemeEntry> {
        let root = mypaper_config::user_themes_dir()?;
        let copy = themes::duplicate_into(&root, &dir)?;
        Ok(themes::entry(&copy, Origin::User, None))
    };
    let entry = duplicate().map_err(text)?;
    allow_previews(&app, std::slice::from_ref(&entry));
    Ok(entry)
}

#[tauri::command]
pub async fn get_style(dir: PathBuf) -> CmdResult<WindowsStyle> {
    themes::read_style(&dir).map_err(text)
}

/// Saves the `windows` section of a user theme and returns what is now on disk.
#[tauri::command]
pub async fn set_style(dir: PathBuf, style: WindowsStyle) -> CmdResult<WindowsStyle> {
    let save = || -> Result<WindowsStyle> {
        themes::save_style(&dir, &mypaper_config::user_themes_dir()?, &style)?;
        themes::read_style(&dir)
    };
    save().map_err(text)
}

/// Reads a theme's `widgets` or `bars` section.
#[tauri::command]
pub async fn get_skin(dir: PathBuf, kind: SkinKind) -> CmdResult<SkinEdit> {
    themes::read_skin(&dir, kind).map_err(text)
}

/// Saves a user theme's `widgets` or `bars` section and returns what is now on disk.
#[tauri::command]
pub async fn set_skin(dir: PathBuf, kind: SkinKind, skin: SkinEdit) -> CmdResult<SkinEdit> {
    let save = || -> Result<SkinEdit> {
        themes::save_skin(&dir, &mypaper_config::user_themes_dir()?, kind, &skin)?;
        themes::read_skin(&dir, kind)
    };
    save().map_err(text)
}

/// Copies a style sheet into a user theme; resolves to the path for the skin's `css`.
#[tauri::command]
pub async fn import_skin_css(dir: PathBuf, kind: SkinKind, source: PathBuf) -> CmdResult<String> {
    let import =
        || themes::import_skin_css(&dir, &mypaper_config::user_themes_dir()?, kind, &source);
    import().map_err(text)
}

/// The active theme's looks for widgets and bars: the layer under the user's own choices.
#[derive(Debug, Default, Serialize)]
pub struct ActiveSkins {
    widgets: Look,
    bars: Look,
}

#[tauri::command]
pub async fn active_skins() -> ActiveSkins {
    let skins = || -> Result<ActiveSkins> {
        let theme = LoadedTheme::load(mypaper_theme::locate(Some(&load_config()?.theme))?)?.theme;
        let look = |skin: Option<mypaper_theme::Skin>| {
            Look(skin.and_then(|s| s.appearance).unwrap_or_default())
        };
        Ok(ActiveSkins {
            widgets: look(theme.widgets),
            bars: look(theme.bars),
        })
    };
    skins().unwrap_or_default()
}

#[tauri::command]
pub async fn get_bars() -> CmdResult<BarsState> {
    bars::path()
        .and_then(|path| bars::load_from(&path))
        .map_err(text)
}

/// The host's defaults, presets and geometry: what the page merges a `bars.json` against.
#[tauri::command]
pub async fn bars_catalog() -> CmdResult<Catalog> {
    bars::catalog().map_err(text)
}

/// Writes the whole document, after the host's schema has accepted it.
#[tauri::command]
pub async fn set_bars(doc: Doc) -> CmdResult<BarsState> {
    bars::path()
        .and_then(|path| bars::save_to(&path, &doc))
        .map_err(text)
}

/// Writes an empty `bars.json` (all defaults), for when the file on disk can't be parsed.
#[tauri::command]
pub async fn reset_bars() -> CmdResult<BarsState> {
    bars::path()
        .and_then(|path| bars::reset_at(&path))
        .map_err(text)
}

#[derive(Debug, Serialize)]
pub struct ThemeVar {
    name: String,
    #[serde(flatten)]
    decl: VarDecl,
}

/// The active theme's tunable bars variables (docs/bars.md, "Theme variables").
#[derive(Debug, Default, Serialize)]
pub struct ThemeVars {
    /// The active theme's name.
    theme: String,
    /// Its bars style sheet, when it has one.
    css: Option<String>,
    vars: Vec<ThemeVar>,
    /// The theme or its declarations couldn't be read.
    error: Option<String>,
}

#[tauri::command]
pub async fn theme_vars() -> ThemeVars {
    let read = || -> Result<ThemeVars> {
        let loaded = LoadedTheme::load(mypaper_theme::locate(Some(&load_config()?.theme))?)?;
        let mut found = ThemeVars {
            theme: loaded.theme.name.clone(),
            ..ThemeVars::default()
        };
        let Some(css) = loaded.theme.bars.and_then(|skin| skin.css) else {
            return Ok(found);
        };
        let path = loaded.dir.join(&css);
        let sheet = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        found.css = Some(css.display().to_string());
        found.vars = vars::declared(&sheet)
            .with_context(|| format!("in {}", path.display()))?
            .into_iter()
            .map(|(name, decl)| ThemeVar { name, decl })
            .collect();
        Ok(found)
    };
    read().unwrap_or_else(|e| ThemeVars {
        error: Some(text(e)),
        ..ThemeVars::default()
    })
}

/// The icon packs to choose from: bundled with the bars page, then the user's.
#[tauri::command]
pub async fn list_icon_packs(app: AppHandle) -> CmdResult<Vec<PackInfo>> {
    let user = iconpacks::user_root().ok();
    let packs = iconpacks::list_from(iconpacks::bundled_root().as_deref(), user.as_deref());
    let scope = app.asset_protocol_scope();
    for preview in packs.iter().flat_map(|p| &p.previews) {
        let _ = scope.allow_file(preview);
    }
    Ok(packs)
}

/// The per-app icons of one of the user's packs.
#[tauri::command]
pub async fn icon_pack_apps(app: AppHandle, pack: String) -> CmdResult<Vec<AppIcon>> {
    let list = || -> Result<Vec<AppIcon>> {
        let apps = iconpacks::apps(&iconpacks::user_pack_dir(&iconpacks::user_root()?, &pack)?)?;
        let scope = app.asset_protocol_scope();
        for icon in &apps {
            let _ = scope.allow_file(&icon.file);
        }
        Ok(apps)
    };
    list().map_err(text)
}

/// Makes an empty pack of the user's own (and does nothing if it exists).
#[tauri::command]
pub async fn create_icon_pack(name: String) -> CmdResult<()> {
    let make = || -> Result<()> {
        iconpacks::create(&iconpacks::user_root()?, &name)?;
        Ok(())
    };
    make().map_err(text)
}

/// Gives the app at `target` the picture at `image`, in one of the user's packs.
#[tauri::command]
pub async fn set_app_icon(pack: String, target: String, image: PathBuf) -> CmdResult<()> {
    let set = || {
        iconpacks::set_app_icon(
            &iconpacks::user_pack_dir(&iconpacks::user_root()?, &pack)?,
            &target,
            &image,
        )
    };
    set().map_err(text)
}

#[tauri::command]
pub async fn remove_app_icon(pack: String, index: usize) -> CmdResult<()> {
    let remove = || {
        iconpacks::remove_app_icon(
            &iconpacks::user_pack_dir(&iconpacks::user_root()?, &pack)?,
            index,
        )
    };
    remove().map_err(text)
}

/// Whether a web or 3D scene theme forwards the mouse; null for the other kinds.
#[tauri::command]
pub async fn get_interactive(dir: PathBuf) -> CmdResult<Option<bool>> {
    themes::read_interactive(&dir).map_err(text)
}

#[tauri::command]
pub async fn set_interactive(dir: PathBuf, on: bool) -> CmdResult<Option<bool>> {
    let save = || -> Result<Option<bool>> {
        themes::save_interactive(&dir, &mypaper_config::user_themes_dir()?, on)?;
        themes::read_interactive(&dir)
    };
    save().map_err(text)
}

/// The cursor sets in `%APPDATA%\mypaper\cursors`.
#[tauri::command]
pub async fn list_cursor_sets() -> Vec<String> {
    mypaper_config::dir()
        .map(|dir| themes::user_cursor_sets(&dir.join("cursors")))
        .unwrap_or_default()
}

/// A rectangle in physical pixels.
#[derive(Debug, Serialize)]
pub struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
pub struct MonitorInfo {
    /// The device name `bars.json` uses, e.g. `\\.\DISPLAY2`.
    device: String,
    primary: bool,
    /// Physical pixels.
    width: u32,
    height: u32,
    /// The DPI scale (1.5 = 150%): a DIP is `scale` pixels.
    scale: f64,
    /// Where the monitor sits on the virtual screen.
    x: i32,
    y: i32,
    /// The work area (the monitor without the taskbar and app bars), relative to the monitor.
    work: Rect,
}

#[tauri::command]
pub async fn list_monitors(app: AppHandle) -> CmdResult<Vec<MonitorInfo>> {
    let primary = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .and_then(|m| m.name().cloned());
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    Ok(monitors
        .iter()
        .filter_map(|m| {
            let device = m.name()?.clone();
            let (origin, work) = (m.position(), m.work_area());
            Some(MonitorInfo {
                primary: primary.as_ref() == Some(&device),
                device,
                width: m.size().width,
                height: m.size().height,
                scale: m.scale_factor(),
                x: origin.x,
                y: origin.y,
                work: Rect {
                    x: work.position.x - origin.x,
                    y: work.position.y - origin.y,
                    width: work.size.width,
                    height: work.size.height,
                },
            })
        })
        .collect())
}

/// The icon file for a dock item. The webview may then read it through the asset protocol.
#[tauri::command]
pub async fn dock_icon(app: AppHandle, path: String) -> CmdResult<String> {
    let file = tauri::async_runtime::spawn_blocking(move || icons::icon(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(text)?;
    app.asset_protocol_scope()
        .allow_file(&file)
        .map_err(|e| e.to_string())?;
    Ok(file.display().to_string())
}

/// Native widget metadata and settings schemas. Native surfaces have no asset directories.
#[tauri::command]
pub async fn list_widgets() -> Vec<WidgetInfo> {
    widgets::list()
}

/// Calls one of a widget's plugin methods for it: a place setting's town search, and the plugin
/// calls of the widget pages in the live preview.
#[tauri::command]
pub async fn plugin_call(
    widget: String,
    plugin: String,
    method: String,
    params: serde_json::Value,
) -> CmdResult<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let info = widgets::list()
            .into_iter()
            .find(|w| w.id == widget)
            .with_context(|| format!("the {widget} widget isn't installed"))?;
        plugins::call(&info.manifest, &plugin, &method, params)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(text)
}

/// Adds a placed widget or changes some of its fields (cell, size, monitor, settings, own look).
#[tauri::command]
pub async fn put_instance(patch: InstancePatch) -> CmdResult<Layout> {
    widgets::put_instance(&patch).map_err(text)
}

#[tauri::command]
pub async fn get_layout() -> CmdResult<Layout> {
    widgets::load().map_err(text)
}

#[tauri::command]
pub async fn set_widgets_appearance(appearance: Look) -> CmdResult<Layout> {
    widgets::set_appearance(&appearance).map_err(text)
}

#[tauri::command]
pub async fn remove_instance(instance: String) -> CmdResult<Layout> {
    widgets::remove_instance(&instance).map_err(text)
}

/// Drops one placed widget's own look so it follows the global one again.
#[tauri::command]
pub async fn reset_instance_look(instance: String) -> CmdResult<Layout> {
    widgets::clear_instance_look(&instance).map_err(text)
}

/// Shows or hides the topmost overlay widgets.
#[tauri::command]
pub async fn set_widgets_overlay(on: bool) -> CmdResult<Layout> {
    widgets::set_overlay(on).map_err(text)
}

/// Toggles the widget host's edit mode on the desktop.
#[tauri::command]
pub async fn toggle_widget_edit() -> CmdResult<()> {
    widgets::toggle_edit_mode().map_err(text)
}

#[tauri::command]
pub async fn components() -> Vec<Component> {
    engine::components()
}

#[tauri::command]
pub async fn engine_status() -> EngineStatus {
    engine::status()
}

#[tauri::command]
pub async fn start_engine() -> CmdResult<EngineStatus> {
    engine::start().map_err(text)?;
    Ok(engine::status())
}

#[tauri::command]
pub async fn stop_engine() -> CmdResult<EngineStatus> {
    engine::stop().map_err(text)?;
    Ok(engine::status())
}

#[tauri::command]
pub async fn get_autostart() -> Autostart {
    autostart::get()
}

#[tauri::command]
pub async fn set_autostart(enabled: bool) -> CmdResult<Autostart> {
    autostart::set(enabled).map_err(text)
}
