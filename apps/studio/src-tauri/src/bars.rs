//! `%APPDATA%\mypaper\bars.json`: the top bar, dock and native-taskbar settings
//! (docs/bars.md, "Config"). The structs mirror the document. `mypaper-bars`
//! rejects unknown fields, but Studio keeps them: every struct collects what it
//! doesn't know in `extra` (a `serde_json::Map`) and writes it back, so editing
//! never drops something another tool or a newer version put there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::fsio;
use crate::look::Look;

pub const FILE: &str = "bars.json";

type Extra = Map<String, Value>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskbarMode {
    /// Untouched.
    #[default]
    Keep,
    Autohide,
    /// Hidden on every monitor; its space goes to our bars.
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    Center,
    Left,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Taskbar {
    pub mode: TaskbarMode,
    /// Omitted: leave Windows' own setting alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The top-bar items (docs/bars.md, "Modules").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Module {
    Start,
    Workspaces,
    App,
    Clock,
    Media,
    Tray,
    Network,
    Volume,
    Battery,
    Quick,
    Power,
    Stats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitorKeyword {
    All,
    Primary,
}

/// `"all"`, `"primary"`, or device names such as `\\.\DISPLAY2`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Monitors {
    Keyword(MonitorKeyword),
    Devices(Vec<String>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clock {
    pub seconds: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TopBar {
    pub enabled: bool,
    pub monitors: Monitors,
    /// DIPs.
    pub height: u32,
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<Clock>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for TopBar {
    fn default() -> Self {
        use Module::*;
        Self {
            enabled: true,
            monitors: Monitors::Keyword(MonitorKeyword::All),
            height: 32,
            left: vec![Start, Workspaces, App],
            center: vec![Module::Clock],
            right: vec![Media, Tray, Network, Volume, Battery, Quick, Power],
            clock: None,
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockPosition {
    #[default]
    Bottom,
    Left,
    Right,
}

/// A pinned dock item: an exe, `.lnk`, folder or `shell:AppsFolder\<AUMID>` path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pinned {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dock {
    pub enabled: bool,
    pub monitors: Monitors,
    pub position: DockPosition,
    /// DIPs.
    pub size: u32,
    pub autohide: bool,
    pub magnify: bool,
    /// Show running apps that aren't pinned.
    pub running: bool,
    pub pinned: Vec<Pinned>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            enabled: false,
            monitors: Monitors::Keyword(MonitorKeyword::Primary),
            position: DockPosition::Bottom,
            size: 48,
            autohide: false,
            magnify: true,
            running: true,
            pinned: Vec::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BarsConfig {
    pub version: u32,
    pub taskbar: Taskbar,
    pub top: TopBar,
    pub dock: Dock,
    pub appearance: Look,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            version: 1,
            taskbar: Taskbar::default(),
            top: TopBar::default(),
            dock: Dock::default(),
            appearance: Look::default(),
            extra: Extra::new(),
        }
    }
}

/// DIP ranges the editor offers; the bars page uses whatever is in the file.
pub const TOP_HEIGHT: std::ops::RangeInclusive<u32> = 20..=64;
pub const DOCK_SIZE: std::ops::RangeInclusive<u32> = 24..=128;

impl BarsConfig {
    /// What `mypaper-bars` would refuse or misread. Run before every write.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "bars.json version must be 1, got {}", self.version);
        ensure!(TOP_HEIGHT.contains(&self.top.height), "top.height must be between {} and {}", TOP_HEIGHT.start(), TOP_HEIGHT.end());
        ensure!(DOCK_SIZE.contains(&self.dock.size), "dock.size must be between {} and {}", DOCK_SIZE.start(), DOCK_SIZE.end());

        let mut placed = HashSet::new();
        for module in self.top.left.iter().chain(&self.top.center).chain(&self.top.right) {
            ensure!(placed.insert(*module), "the {module:?} module is placed twice");
        }
        for (what, monitors) in [("top", &self.top.monitors), ("dock", &self.dock.monitors)] {
            if let Monitors::Devices(devices) = monitors {
                ensure!(!devices.is_empty() && devices.iter().all(|d| !d.trim().is_empty()), "{what}.monitors needs at least one monitor");
            }
        }
        let mut paths = HashSet::new();
        for item in &self.dock.pinned {
            ensure!(!item.path.trim().is_empty(), "a pinned item has no path");
            ensure!(paths.insert(item.path.to_lowercase()), "{} is pinned twice", item.path);
        }
        self.appearance.validate()
    }

    /// Dotted paths of every field Studio doesn't know. `mypaper-bars` rejects
    /// the whole file while any are present, so the editor says so.
    pub fn unknown_fields(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut note = |prefix: &str, extra: &Extra| found.extend(extra.keys().map(|key| format!("{prefix}{key}")));
        note("", &self.extra);
        note("taskbar.", &self.taskbar.extra);
        note("top.", &self.top.extra);
        if let Some(clock) = &self.top.clock {
            note("top.clock.", &clock.extra);
        }
        note("dock.", &self.dock.extra);
        for item in &self.dock.pinned {
            note("dock.pinned[].", &item.extra);
        }
        found
    }
}

pub fn parse(text: &str) -> Result<BarsConfig> {
    serde_json::from_str(text).map_err(Into::into)
}

pub fn render(config: &BarsConfig) -> Result<String> {
    Ok(serde_json::to_string_pretty(config)? + "\n")
}

/// What the Bars page needs to show the file.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarsState {
    pub config: BarsConfig,
    /// False: these are the built-in defaults, and no file has been written yet.
    pub exists: bool,
    pub path: String,
    /// Problems that don't stop the file from loading but that the bars would trip on.
    pub warnings: Vec<String>,
}

pub fn path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(FILE))
}

pub fn load_from(path: &Path) -> Result<BarsState> {
    let text = fsio::read_optional(path)?;
    let config = match &text {
        Some(text) => parse(text).with_context(|| format!("in {}", path.display()))?,
        None => BarsConfig::default(),
    };
    let mut warnings = Vec::new();
    if !config.unknown_fields().is_empty() {
        warnings.push(format!(
            "{} has fields mypaper doesn't know ({}). Studio keeps them, but the bars ignore the whole file until they are removed.",
            FILE,
            config.unknown_fields().join(", ")
        ));
    }
    if let Err(e) = config.validate() {
        warnings.push(format!("{e:#}"));
    }
    Ok(BarsState { config, exists: text.is_some(), path: path.display().to_string(), warnings })
}

/// Validates and writes `config`. A file that doesn't parse is never replaced
/// (it may be mid-edit in a text editor): fix or reset it first.
pub fn save_to(path: &Path, config: &BarsConfig) -> Result<BarsState> {
    config.validate()?;
    let existing = fsio::read_optional(path)?;
    if let Some(text) = &existing {
        parse(text).with_context(|| format!("{} can't be read, so Studio won't overwrite it", path.display()))?;
    }
    let rendered = render(config)?;
    if existing.as_deref() != Some(rendered.as_str()) {
        fsio::write_atomic(path, rendered)?;
    }
    load_from(path)
}

/// Replaces the file with the defaults.
pub fn reset_at(path: &Path) -> Result<BarsState> {
    fsio::write_atomic(path, render(&BarsConfig::default())?)?;
    load_from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    const FULL: &str = r#"{
  "version": 1,
  "taskbar": { "mode": "hide", "alignment": "left" },
  "top": {
    "enabled": true, "monitors": ["\\\\.\\DISPLAY2"], "height": 36,
    "left": ["start"], "center": ["clock"], "right": ["volume", "power"],
    "clock": { "seconds": true, "format": "HH:mm", "tz": "UTC" },
    "theme_hint": 7
  },
  "dock": {
    "enabled": true, "monitors": "primary", "position": "left", "size": 56,
    "autohide": true, "magnify": false, "running": false,
    "pinned": [ { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer", "badge": "pro" } ]
  },
  "appearance": { "surface": "glass", "opacity": 0.6 },
  "future": { "a": [1, 2] }
}"#;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_full_file_round_trips_and_keeps_fields_it_does_not_know() {
        let config = parse(FULL).unwrap();
        assert_eq!(config.taskbar.mode, TaskbarMode::Hide);
        assert_eq!(config.top.monitors, Monitors::Devices(vec![r"\\.\DISPLAY2".into()]));
        assert_eq!(config.dock.position, DockPosition::Left);
        assert_eq!(config.top.right, [Module::Volume, Module::Power]);
        assert_eq!(
            config.unknown_fields(),
            ["future", "top.theme_hint", "top.clock.tz", "dock.pinned[].badge"]
        );

        // Edit through the typed model, then compare with the original document.
        let mut edited = config.clone();
        edited.top.height = 40;
        edited.dock.pinned[0].name = Some("Explorer".into());
        let text = render(&edited).unwrap();
        let mut expected = json(FULL);
        expected["top"]["height"] = 40.into();
        expected["dock"]["pinned"][0]["name"] = "Explorer".into();
        assert_eq!(json(&text), expected, "{text}");
        assert_eq!(parse(&text).unwrap(), edited);
    }

    #[test]
    fn unset_optionals_are_left_out_of_the_file() {
        let mut config = BarsConfig::default();
        config.appearance.0.opacity = Some(0.55);
        let value = json(&render(&config).unwrap());
        assert!(value["taskbar"].get("alignment").is_none());
        assert!(value["top"].get("clock").is_none());
        assert_eq!(value["appearance"], json(r#"{ "opacity": 0.55 }"#));
        assert_eq!(parse(&render(&config).unwrap()).unwrap(), config);
    }

    #[test]
    fn missing_pieces_take_the_documented_defaults() {
        let config = parse("{}").unwrap();
        assert_eq!(config, BarsConfig::default());
        assert!(config.top.enabled && !config.dock.enabled);
        assert_eq!(config.taskbar.mode, TaskbarMode::Keep);
        assert_eq!(config.top.height, 32);
        assert_eq!((config.dock.size, config.dock.magnify, config.dock.running), (48, true, true));
        assert_eq!(config.top.center, [Module::Clock]);

        // A partial section keeps its other defaults.
        let partial = parse(r#"{ "dock": { "enabled": true } }"#).unwrap();
        assert!(partial.dock.enabled && partial.dock.running && partial.top.enabled);
        config.validate().unwrap();
    }

    #[test]
    fn no_file_shows_the_defaults_and_saving_creates_it() {
        let dir = temp_dir("bars-missing");
        let path = dir.join("mypaper").join(FILE);
        let state = load_from(&path).unwrap();
        assert!(!state.exists && state.warnings.is_empty());
        assert_eq!(state.config, BarsConfig::default());

        let mut config = state.config;
        config.dock.enabled = true;
        let saved = save_to(&path, &config).unwrap();
        assert!(saved.exists);
        assert_eq!(load_from(&path).unwrap().config, config);
        assert!(!path.with_file_name("bars.json.studio.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_keeps_unknown_fields_in_the_file_on_disk() {
        let dir = temp_dir("bars-keep");
        let path = dir.join(FILE);
        std::fs::write(&path, FULL).unwrap();
        let state = load_from(&path).unwrap();
        assert_eq!(state.warnings.len(), 1, "{:?}", state.warnings);

        let mut config = state.config;
        config.top.right.pop();
        save_to(&path, &config).unwrap();
        let on_disk = json(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(on_disk["future"], json(r#"{ "a": [1, 2] }"#));
        assert_eq!(on_disk["top"]["theme_hint"], 7);
        assert_eq!(on_disk["top"]["right"], json(r#"["volume"]"#));
        assert_eq!(on_disk["dock"]["pinned"][0]["badge"], "pro");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_values_are_refused_before_anything_is_written() {
        let dir = temp_dir("bars-invalid");
        let path = dir.join(FILE);
        let refused = |change: fn(&mut BarsConfig)| {
            let mut config = BarsConfig::default();
            change(&mut config);
            save_to(&path, &config).is_err()
        };
        assert!(refused(|c| c.top.height = 4), "top height");
        assert!(refused(|c| c.dock.size = 400), "dock size");
        assert!(refused(|c| c.top.left.push(Module::Clock)), "a module placed twice");
        assert!(refused(|c| c.dock.monitors = Monitors::Devices(vec![])), "no monitors");
        assert!(refused(|c| c.dock.pinned = vec![pin("C:\\a.exe"), pin("c:\\A.exe")]), "pinned twice");
        assert!(refused(|c| c.appearance.0.opacity = Some(2.0)), "opacity");
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn pin(path: &str) -> Pinned {
        Pinned { path: path.into(), name: None, extra: Extra::new() }
    }

    #[test]
    fn a_broken_file_is_reported_and_never_overwritten() {
        let dir = temp_dir("bars-broken");
        let path = dir.join(FILE);
        std::fs::write(&path, r#"{ "top": { "height": "tall" } }"#).unwrap();
        assert!(load_from(&path).is_err());
        assert!(save_to(&path, &BarsConfig::default()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{ "top": { "height": "tall" } }"#);

        let reset = reset_at(&path).unwrap();
        assert_eq!(reset.config, BarsConfig::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
