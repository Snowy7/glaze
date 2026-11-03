//! The frontend's whole backend surface. Every command is `async` so Tauri
//! runs it off the UI thread (process and registry calls can take a moment),
//! and every error is the full `anyhow` chain as a string.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_theme::LoadedTheme;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::autostart::{self, Autostart};
use crate::bars::{self, BarsConfig, BarsState};
use crate::engine::{self, Component, EngineStatus};
use crate::icons;
use crate::look::Look;
use crate::manifest::{SkinEdit, SkinKind, WindowsStyle};
use crate::source::{self, Kind};
use crate::themes::{self, NewTheme, Origin, ThemeEntry, ThemeList};
use crate::widgets::{self, Layout, WidgetInfo};

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

/// A partial config update; absent fields are left alone.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ConfigPatch {
    theme: Option<String>,
    paused: Option<bool>,
    pause_on_battery: Option<bool>,
    pause_on_fullscreen: Option<bool>,
}

impl ConfigPatch {
    fn apply(self, config: &mut Config) {
        if let Some(theme) = self.theme {
            config.theme = theme;
        }
        config.paused = self.paused.unwrap_or(config.paused);
        config.pause_on_battery = self.pause_on_battery.unwrap_or(config.pause_on_battery);
        config.pause_on_fullscreen = self.pause_on_fullscreen.unwrap_or(config.pause_on_fullscreen);
    }
}

/// Refuses themes the engine couldn't load, since it stops (rather than
/// crash-loops) on a config it can't use.
fn check_theme(theme: &str) -> Result<()> {
    let dir = mypaper_theme::locate(Some(theme))?;
    LoadedTheme::load(dir)?;
    Ok(())
}

fn update(patch: ConfigPatch) -> Result<Config> {
    let mut config = load_config()?;
    let before = config.clone();
    if let Some(theme) = &patch.theme {
        check_theme(theme).with_context(|| format!("can't apply theme {theme:?}"))?;
    }
    patch.apply(&mut config);
    // Skip no-op saves: each real save makes the engine reload.
    if config != before {
        config.save()?;
    }
    Ok(config)
}

#[tauri::command]
pub async fn get_config() -> CmdResult<Config> {
    load_config().map_err(text)
}

#[tauri::command]
pub async fn update_config(patch: ConfigPatch) -> CmdResult<Config> {
    update(patch).map_err(text)
}

/// Writes the defaults, for when the file on disk can't be parsed.
#[tauri::command]
pub async fn reset_config() -> CmdResult<Config> {
    let config = Config::default();
    config.save().map_err(text)?;
    Ok(config)
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
                    .map(|p| PresetInfo { id: p.id, label: p.label, args: source::join_args(p.args) })
                    .collect()
            } else {
                Vec::new()
            },
            default_args: source::suggested_preset(path).filter(|_| is_app).map_or_else(String::new, |p| source::join_args(p.args)),
            kind,
            source,
        }
    }
}

/// What a picked file or folder would become, or why it can't be a theme.
#[tauri::command]
pub async fn inspect_source(path: PathBuf) -> CmdResult<SourceInfo> {
    source::resolve(&path.to_string_lossy()).map(Into::into).map_err(text)
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
            update(ConfigPatch { theme: Some(entry.apply_as.clone()), ..Default::default() })
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
    let import = || themes::import_skin_css(&dir, &mypaper_config::user_themes_dir()?, kind, &source);
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
        let look = |skin: Option<mypaper_theme::Skin>| Look(skin.and_then(|s| s.appearance).unwrap_or_default());
        Ok(ActiveSkins { widgets: look(theme.widgets), bars: look(theme.bars) })
    };
    skins().unwrap_or_default()
}

#[tauri::command]
pub async fn get_bars() -> CmdResult<BarsState> {
    bars::path().and_then(|path| bars::load_from(&path)).map_err(text)
}

#[tauri::command]
pub async fn set_bars(config: BarsConfig) -> CmdResult<BarsState> {
    bars::path().and_then(|path| bars::save_to(&path, &config)).map_err(text)
}

/// Writes the default `bars.json`, for when the file on disk can't be parsed.
#[tauri::command]
pub async fn reset_bars() -> CmdResult<BarsState> {
    bars::path().and_then(|path| bars::reset_at(&path)).map_err(text)
}

#[derive(Debug, Serialize)]
pub struct MonitorInfo {
    /// The device name `bars.json` uses, e.g. `\\.\DISPLAY2`.
    device: String,
    primary: bool,
    width: u32,
    height: u32,
}

#[tauri::command]
pub async fn list_monitors(app: AppHandle) -> CmdResult<Vec<MonitorInfo>> {
    let primary = app.primary_monitor().map_err(|e| e.to_string())?.and_then(|m| m.name().cloned());
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    Ok(monitors
        .iter()
        .filter_map(|m| {
            let device = m.name()?.clone();
            Some(MonitorInfo { primary: primary.as_ref() == Some(&device), device, width: m.size().width, height: m.size().height })
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
    app.asset_protocol_scope().allow_file(&file).map_err(|e| e.to_string())?;
    Ok(file.display().to_string())
}

#[tauri::command]
pub async fn list_widgets() -> Vec<WidgetInfo> {
    widgets::list()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_changes_only_the_fields_it_names() {
        let mut config = Config::default();
        ConfigPatch { paused: Some(true), ..Default::default() }.apply(&mut config);
        assert_eq!(config, Config { paused: true, ..Config::default() });

        ConfigPatch { theme: Some("photo".into()), pause_on_battery: Some(false), ..Default::default() }
            .apply(&mut config);
        assert_eq!(
            config,
            Config { theme: "photo".into(), paused: true, pause_on_battery: false, pause_on_fullscreen: true }
        );
    }

    #[test]
    fn patches_deserialize_from_partial_json() {
        let patch: ConfigPatch = serde_json::from_str(r#"{ "pause_on_fullscreen": false }"#).unwrap();
        assert_eq!(patch.pause_on_fullscreen, Some(false));
        assert!(patch.theme.is_none() && patch.paused.is_none());
    }
}
