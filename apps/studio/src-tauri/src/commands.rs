//! The frontend's whole backend surface. Every command is `async` so Tauri
//! runs it off the UI thread (process and registry calls can take a moment),
//! and every error is the full `anyhow` chain as a string.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_theme::LoadedTheme;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::autostart::{self, Autostart};
use crate::engine::{self, EngineStatus};
use crate::shell::ShellInfo;
use crate::source::{self, Kind};
use crate::themes::{self, NewTheme, Origin, ThemeEntry, ThemeList};

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
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    path: String,
    kind: Kind,
    uses_fit: bool,
    suggested_name: String,
}

/// What a picked file or folder would become, or why it can't be a theme.
#[tauri::command]
pub async fn inspect_source(path: PathBuf) -> CmdResult<SourceInfo> {
    let kind = source::check(&path).map_err(text)?;
    Ok(SourceInfo {
        path: path.display().to_string(),
        kind,
        uses_fit: kind.uses_fit(),
        suggested_name: source::suggested_name(&path),
    })
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

#[tauri::command]
pub async fn shell_info(app: AppHandle) -> ShellInfo {
    *app.state::<ShellInfo>()
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
