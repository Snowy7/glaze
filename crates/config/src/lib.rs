//! User settings, stored as JSON at `%APPDATA%\mypaper\config.json`.
//!
//! The studio writes this file; the wallpaper engine watches it and applies
//! changes live (theme switch, pause), so there is no IPC to keep in sync.
//! Missing fields take their defaults, so older files keep working.

pub mod desktop_menu;

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub use desktop_menu::DesktopMenu;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Theme name (looked up like `mypaper_theme::locate`) or folder path.
    pub theme: String,
    /// Paused by the user (tray or studio).
    pub paused: bool,
    /// Pause while running on battery.
    pub pause_on_battery: bool,
    /// Pause every screen while a fullscreen app or game is in front, so
    /// games get the whole GPU.
    pub pause_on_fullscreen: bool,
    /// mypaper's own right-click menu for the bare desktop (opt-in). Left out
    /// of the file while it is all defaults, so saving settings never freezes them.
    #[serde(skip_serializing_if = "DesktopMenu::is_default")]
    pub desktop_menu: DesktopMenu,
    /// Windows 11: use the classic full context menu everywhere (undocumented
    /// registry switch, opt-in, reversible; Explorer needs a restart to show it).
    pub classic_context_menu: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "aurora".into(),
            paused: false,
            pause_on_battery: true,
            pause_on_fullscreen: true,
            desktop_menu: DesktopMenu::default(),
            classic_context_menu: false,
        }
    }
}

/// `%APPDATA%\mypaper`: config plus the user's own `themes\`.
pub fn dir() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("mypaper"))
}

/// The config file path.
pub fn path() -> Result<PathBuf> {
    Ok(dir()?.join("config.json"))
}

/// Where user-created themes live.
pub fn user_themes_dir() -> Result<PathBuf> {
    Ok(dir()?.join("themes"))
}

impl Config {
    /// Loads the config, or the defaults if there is no file yet.
    pub fn load() -> Result<Self> {
        let path = path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn parse(json: &str) -> Result<Self> {
        let config: Self = serde_json::from_str(json)?;
        config.desktop_menu.validate()?;
        Ok(config)
    }

    /// Writes atomically (temp file + rename), so watchers never see a
    /// half-written file.
    pub fn save(&self) -> Result<()> {
        let path = path()?;
        std::fs::create_dir_all(path.parent().context("config path has no parent")?)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_string_pretty(self)? + "\n")?;
        std::fs::rename(&temp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default_and_typos_fail() {
        let config = Config::parse(r#"{ "theme": "photo" }"#).unwrap();
        assert_eq!(
            config,
            Config {
                theme: "photo".into(),
                ..Config::default()
            }
        );
        assert!(Config::parse(r#"{ "pause_on_batery": true }"#).is_err());
    }

    #[test]
    fn the_desktop_menu_and_classic_menu_are_opt_in_and_checked() {
        let config = Config::default();
        assert!(!config.desktop_menu.enabled && !config.classic_context_menu);

        let on = Config::parse(
            r#"{ "desktop_menu": { "enabled": true, "items": ["refresh"] }, "classic_context_menu": true }"#,
        )
        .unwrap();
        assert!(on.desktop_menu.enabled && on.classic_context_menu);
        assert_eq!(on.desktop_menu.items.len(), 1);
        // A bad custom item fails the whole config, like any other typo.
        assert!(
            Config::parse(r#"{ "desktop_menu": { "items": [{ "label": "", "command": "x" }] } }"#)
                .is_err()
        );
    }

    #[test]
    fn round_trips() {
        let config = Config {
            paused: true,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(Config::parse(&json).unwrap(), config);
    }
}
