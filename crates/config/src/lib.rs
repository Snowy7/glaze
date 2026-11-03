//! User settings, stored as JSON at `%APPDATA%\mypaper\config.json`.
//!
//! The studio writes this file; the wallpaper engine watches it and applies
//! changes live (theme switch, pause), so there is no IPC to keep in sync.
//! Missing fields take their defaults, so older files keep working.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "aurora".into(),
            paused: false,
            pause_on_battery: true,
            pause_on_fullscreen: true,
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
        Ok(serde_json::from_str(json)?)
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
    fn round_trips() {
        let config = Config {
            paused: true,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(Config::parse(&json).unwrap(), config);
    }
}
