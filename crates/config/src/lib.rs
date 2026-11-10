//! User settings, stored as JSON at `%APPDATA%\glaze\config.json`.
//!
//! The studio writes this file; the wallpaper engine watches it and applies
//! changes live (theme switch, pause), so there is no IPC to keep in sync.
//! Missing fields take their defaults, so older files keep working.

pub mod apps;
pub mod desktop_menu;
mod engine;
mod system;
pub mod look;
mod windows;
pub use engine::{DuringGames, EngineConfig, GpuPreference, RenderOptions, RenderSettings};
pub use system::{PowerMode, SystemConfig};
pub use apps::{AppRules, AppsStyle};
pub use look::Look;
pub use windows::{Backdrop, Color, Corners, Paint, WindowsStyle};
pub use glaze_tiling::Settings as Tiling;

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub use desktop_menu::DesktopMenu;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Reversible per-user Windows preferences, independent of the wallpaper's styling.
    #[serde(skip_serializing_if = "SystemConfig::is_empty")]
    pub system: SystemConfig,
    /// Optional native services. Every experiment is off until explicitly enabled.
    pub experimental: Experimental,
    /// Scrolling layout preferences; unused while the experiment is off.
    #[serde(skip_serializing_if = "Tiling::is_default")]
    pub tiling: Tiling,
    /// File-name index rules layered over its built-in cache/build exclusions.
    #[serde(skip_serializing_if = "IndexConfig::is_empty")]
    pub index: IndexConfig,
    /// Wallpaper GPU preference and frame cap, with optional overrides keyed by theme folder ID.
    pub engine: EngineConfig,
    /// Theme name (looked up like `glaze_theme::locate`) or folder path.
    pub theme: String,
    /// Paused by the user (tray or studio).
    pub paused: bool,
    /// Pause while running on battery.
    pub pause_on_battery: bool,
    /// Pause every screen while a fullscreen app or game is in front, so
    /// games get the whole GPU.
    pub pause_on_fullscreen: bool,
    /// Glaze's own right-click menu for the bare desktop (on by default). Left out
    /// of the file while it is all defaults, so saving settings never freezes them.
    #[serde(skip_serializing_if = "DesktopMenu::is_default")]
    pub desktop_menu: DesktopMenu,
    /// Windows 11: use the classic full context menu everywhere (undocumented
    /// registry switch, opt-in, reversible; Explorer needs a restart to show it).
    pub classic_context_menu: bool,
    /// Also set a still of the theme as the Windows desktop wallpaper, so Windows shows the
    /// theme's look before Glaze starts (docs/performance.md, "Static snapshot"). Turning it
    /// off puts the user's own wallpaper back.
    pub static_snapshot: bool,
    /// The user's look (D2): style, material and colour over the wallpaper's, and how Windows
    /// draws its own windows. Left out of the file while it is empty.
    #[serde(skip_serializing_if = "Look::is_empty")]
    pub look: Look,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            system: SystemConfig::default(),
            experimental: Experimental::default(),
            tiling: Tiling::default(),
            index: IndexConfig::default(),
            engine: EngineConfig::default(),
            theme: "aurora".into(),
            paused: false,
            pause_on_battery: true,
            pause_on_fullscreen: true,
            desktop_menu: DesktopMenu::default(),
            classic_context_menu: false,
            static_snapshot: true,
            look: Look::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndexConfig {
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    /// Minimum age of a volume's last complete reconciliation, in seconds.
    pub startup_rescan_after: u64,
    /// Give login/deployment foreground work time to settle before offline catch-up.
    pub startup_rescan_delay: u64,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            include: Vec::new(),
            startup_rescan_after: 6 * 60 * 60,
            startup_rescan_delay: 120,
        }
    }
}

impl IndexConfig {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Unproven features stay opt-in and incur no runtime cost while disabled.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Experimental {
    /// Replace Snap with a scrolling strip. No tiler exists while disabled.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tiling: bool,
    /// Route folder launches and Win+E to the on-demand native Files app.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub files_default: bool,
    pub file_index: bool,
    /// Restyle the XAML inside File Explorer and Settings (docs/windows-styling.md, "App
    /// styler"): core runs `glaze-styler` only while this is on. Written only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub app_styler: bool,
}

/// `%APPDATA%\glaze`: config plus the user's own `themes\`.
pub fn dir() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("glaze"))
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
        config.engine.validate()?;
        config.look.validate()?;
        config.tiling.validate().map_err(anyhow::Error::msg)?;
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
    #[test]
    fn tiling_is_opt_in_and_validates_geometry_and_keys() {
        let off = super::Config::parse("{}").unwrap();
        assert!(!off.experimental.tiling);
        let json = serde_json::to_value(&off).unwrap();
        assert!(json["experimental"].get("tiling").is_none());
        assert!(json.get("tiling").is_none());
        let on = super::Config::parse(r#"{"experimental":{"tiling":true},"tiling":{"gaps":8,"center_focused_column":"on-overflow"}}"#).unwrap();
        assert!(on.experimental.tiling);
        assert_eq!(super::Config::parse(&serde_json::to_string(&on).unwrap()).unwrap(), on);
        assert!(super::Config::parse(r#"{"tiling":{"preset_widths":[]}}"#).is_err());
        assert!(super::Config::parse(r#"{"tiling":{"default_width":0}}"#).is_err());
        assert!(super::Config::parse(r#"{"tiling":{"gaps":-1}}"#).is_err());
    }
    #[test]
    fn index_startup_policy_defaults_and_explicit_seconds_round_trip() {
        let config = super::Config::parse("{}").unwrap();
        assert_eq!(config.index.startup_rescan_after, 21_600);
        assert_eq!(config.index.startup_rescan_delay, 120);
        assert!(config.index.is_empty());
        let config = super::Config::parse(r#"{"index":{"startup_rescan_after":0,"startup_rescan_delay":3}}"#).unwrap();
        assert!(!config.index.is_empty());
        assert_eq!(config, super::Config::parse(&serde_json::to_string(&config).unwrap()).unwrap());
        assert!(super::Config::parse(r#"{"index":{"startup_rescan_after":-1}}"#).is_err());
    }
    use super::*;

    #[test]
    fn folder_routing_is_explicitly_opt_in() {
        let off = Config::parse("{}").unwrap();
        assert!(!off.experimental.files_default);
        assert!(!serde_json::to_string(&off).unwrap().contains("files_default"));
        let on = Config::parse(r#"{"experimental":{"files_default":true}}"#).unwrap();
        assert!(on.experimental.files_default);
        assert_eq!(Config::parse(&serde_json::to_string(&on).unwrap()).unwrap(), on);
    }

    #[test]
    fn file_index_is_explicitly_opt_in() {
        assert!(!Config::parse("{}").unwrap().experimental.file_index);
        let config = Config::parse(r#"{"experimental":{"file_index":true}}"#).unwrap();
        assert!(Config::parse(&serde_json::to_string(&config).unwrap()).unwrap().experimental.file_index);
        assert!(Config::parse(r#"{"experimental":{"file_indx":true}}"#).is_err());
    }

    #[test]
    fn the_app_styler_is_opt_in_and_only_written_when_on() {
        let off = Config::parse("{}").unwrap();
        assert!(!off.experimental.app_styler);
        assert!(!serde_json::to_string(&off).unwrap().contains("app_styler"));
        let on = Config::parse(r#"{"experimental":{"app_styler":true}}"#).unwrap();
        assert!(on.experimental.app_styler);
        assert_eq!(Config::parse(&serde_json::to_string(&on).unwrap()).unwrap(), on);
    }

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
    fn the_desktop_menu_is_on_by_default_the_classic_menu_is_opt_in_and_both_are_checked() {
        let config = Config::default();
        assert!(config.desktop_menu.enabled && !config.classic_context_menu);
        assert!(Config::parse("{}").unwrap().desktop_menu.enabled);
        // Off stays off: an explicit `false` is written (it isn't the default) and read back.
        let off = Config::parse(r#"{ "desktop_menu": { "enabled": false } }"#).unwrap();
        assert!(!off.desktop_menu.enabled);
        let written = serde_json::to_string(&off).unwrap();
        assert!(!Config::parse(&written).unwrap().desktop_menu.enabled);

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

    #[test]
    fn renderer_keys_parse_round_trip_and_reject_bad_caps_and_choices() {
        let config=Config::parse(r#"{"engine":{"gpu":"high-performance","fps":60,"wallpapers":{"rain-city":{"gpu":"integrated","fps":30}}}}"#).unwrap();
        let chosen = config.engine.resolve("rain-city", RenderOptions::default());
        assert_eq!(chosen.gpu, GpuPreference::Integrated);
        assert_eq!(chosen.fps, Some(30));
        assert_eq!(
            Config::parse(&serde_json::to_string(&config).unwrap()).unwrap(),
            config
        );
        assert!(Config::parse(r#"{"engine":{"fps":0}}"#).is_err());
        assert!(Config::parse(r#"{"engine":{"gpu":"fastest"}}"#).is_err());
    }
}
