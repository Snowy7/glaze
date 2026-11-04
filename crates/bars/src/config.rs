//! `%APPDATA%\mypaper\bars.json`: which bars show where, their modules, the dock's pins and the
//! native taskbar's mode (docs/bars.md, "Config").
//!
//! Every field is optional and unknown fields are errors. Writers (core's tray, Studio, the dock's
//! pin edits) change the file with [`edit`], which touches only the fields it sets, so a file
//! keeps what its author wrote and never gains the defaults.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_theme::Appearance;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const FILE: &str = "bars.json";

/// `%APPDATA%\mypaper\bars.json`.
pub fn path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(FILE))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarsConfig {
    pub version: u32,
    pub taskbar: TaskbarConfig,
    pub top: TopConfig,
    pub dock: DockConfig,
    /// The user's appearance layer; the page gets the resolved look instead.
    #[serde(skip_serializing)]
    pub appearance: Appearance,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            version: 1,
            taskbar: TaskbarConfig::default(),
            top: TopConfig::default(),
            dock: DockConfig::default(),
            appearance: Appearance::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskbarConfig {
    pub mode: TaskbarMode,
    /// `None`: leave Windows' own setting alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskbarMode {
    /// Untouched.
    #[default]
    Keep,
    Autohide,
    /// Hidden on every monitor (and set to auto-hide, so its space is free).
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    Left,
    Center,
}

impl Alignment {
    /// The `TaskbarAl` registry value Settings writes.
    pub fn value(self) -> u32 {
        match self {
            Self::Left => 0,
            Self::Center => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TopConfig {
    pub enabled: bool,
    pub monitors: Monitors,
    /// DIPs.
    pub height: f64,
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
    pub clock: ClockConfig,
}

impl Default for TopConfig {
    fn default() -> Self {
        use Module::*;
        Self {
            enabled: true,
            monitors: Monitors::All,
            height: 32.0,
            left: vec![Start, Workspaces, App],
            center: vec![Clock],
            right: vec![Media, Tray, Network, Volume, Battery, Quick, Power],
            clock: ClockConfig::default(),
        }
    }
}

impl TopConfig {
    /// Every module, left to right.
    pub fn modules(&self) -> impl Iterator<Item = Module> + '_ {
        self.left
            .iter()
            .chain(&self.center)
            .chain(&self.right)
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClockConfig {
    /// Tick every second instead of every minute.
    pub seconds: bool,
    /// See `bars/shell/src/clock-format.ts` for the tokens. Missing: [`ClockConfig::default_format`].
    pub format: Option<String>,
}

impl ClockConfig {
    pub fn default_format(seconds: bool) -> &'static str {
        if seconds {
            "ddd d MMM  HH:mm:ss"
        } else {
            "ddd d MMM  HH:mm"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockConfig {
    pub enabled: bool,
    pub monitors: Monitors,
    /// Which dock layout the page draws; the host sizes the band for it.
    pub style: DockStyle,
    pub position: DockPosition,
    /// The icon size in DIPs; the band follows from the style ([`DockStyle::band`]).
    pub size: f64,
    pub autohide: bool,
    pub magnify: bool,
    /// Also show running apps that aren't pinned.
    pub running: bool,
    /// Missing: the native taskbar's pinned shortcuts (see [`BarsConfig::resolve`]).
    pub pinned: Option<Vec<Pinned>>,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            monitors: Monitors::Primary,
            style: DockStyle::Floating,
            position: DockPosition::Bottom,
            size: 48.0,
            autohide: false,
            magnify: true,
            running: true,
            pinned: None,
        }
    }
}

/// A dock layout. Each style is a page layout (`bars/shell/src/dock/styles.ts`) plus a band size
/// here ([`DockStyle::band`]); a new style adds one of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockStyle {
    /// A pill of icons floating centered on its edge, macOS-style.
    #[default]
    Floating,
}

impl DockStyle {
    /// The band (DIPs) the dock reserves for icons of `size`: for `floating`, a 6 DIP gap from
    /// the screen edge plus a pill with 5 DIP of padding.
    pub fn band(self, size: f64) -> f64 {
        match self {
            Self::Floating => size + 2.0 * 5.0 + 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockPosition {
    #[default]
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    /// An exe, a `.lnk`, a folder, or `shell:AppsFolder\<AUMID>`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A top-bar module (docs/bars.md, "Modules").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Which monitors a bar shows on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Monitors {
    All,
    Primary,
    /// Device names, e.g. `\\.\DISPLAY2`.
    Named(Vec<String>),
}

impl Monitors {
    pub fn includes(&self, device: &str, primary: bool) -> bool {
        match self {
            Self::All => true,
            Self::Primary => primary,
            Self::Named(names) => names.iter().any(|n| n.eq_ignore_ascii_case(device)),
        }
    }
}

impl Serialize for Monitors {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str("all"),
            Self::Primary => serializer.serialize_str("primary"),
            Self::Named(names) => names.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Monitors {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            List(Vec<String>),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Word(word) if word == "all" => Ok(Self::All),
            Raw::Word(word) if word == "primary" => Ok(Self::Primary),
            Raw::Word(word) => Err(serde::de::Error::custom(format!(
                "monitors: expected \"all\", \"primary\" or a list of device names, got {word:?}"
            ))),
            Raw::List(names) => Ok(Self::Named(names)),
        }
    }
}

impl BarsConfig {
    pub fn parse(json: &str) -> Result<Self> {
        let config: Self = serde_json::from_str(json)?;
        config.validate()?;
        Ok(config)
    }

    /// Loads `path`, or the defaults when there is no file.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("version must be 1, got {}", self.version);
        }
        if !(20.0..=80.0).contains(&self.top.height) {
            bail!(
                "top.height must be between 20 and 80, got {}",
                self.top.height
            );
        }
        if !(24.0..=128.0).contains(&self.dock.size) {
            bail!(
                "dock.size must be between 24 and 128, got {}",
                self.dock.size
            );
        }
        let mut seen = BTreeSet::new();
        if let Some(twice) = self.top.modules().find(|m| !seen.insert(*m)) {
            bail!("top: module {twice:?} is listed twice");
        }
        if self.top.clock.format.as_ref().is_some_and(|f| f.len() > 64) {
            bail!("top.clock.format is longer than 64 characters");
        }
        let pinned = self.dock.pinned.iter().flatten();
        if pinned.clone().any(|p| p.path.trim().is_empty()) {
            bail!("dock.pinned: every item needs a path");
        }
        self.appearance.validate()
    }

    /// Fills the defaults that depend on the machine: the clock format and, when `pinned` is
    /// missing, `default_pins` (the native taskbar's pins).
    pub fn resolve(&mut self, default_pins: impl FnOnce() -> Vec<Pinned>) {
        let clock = &mut self.top.clock;
        clock
            .format
            .get_or_insert_with(|| ClockConfig::default_format(clock.seconds).to_owned());
        self.dock.pinned.get_or_insert_with(default_pins);
    }
}

/// Edits `path` in place: `change` gets the file's JSON object (`{ "version": 1 }` when there is
/// no file) and sets what it needs. The result must still be a valid config, and is written
/// atomically. A file that doesn't parse is refused, so a typo is never overwritten.
pub fn edit(path: &Path, change: impl FnOnce(&mut Map<String, Value>)) -> Result<()> {
    let mut root = match std::fs::read_to_string(path) {
        Ok(text) => {
            BarsConfig::parse(&text).with_context(|| format!("not editing {}", path.display()))?;
            match serde_json::from_str(&text)? {
                Value::Object(map) => map,
                _ => bail!("{} isn't a JSON object", path.display()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Map::from_iter([("version".to_owned(), Value::from(1))])
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    change(&mut root);
    let text = serde_json::to_string_pretty(&root)? + "\n";
    BarsConfig::parse(&text).context("the edited config is invalid")?;
    crate::files::write_atomic(path, text.as_bytes())
}

/// Sets one field, `<section>.<key>`, of the file at `path` (see [`edit`]).
pub fn set_field(path: &Path, section: &str, key: &str, value: impl Serialize) -> Result<()> {
    let value = serde_json::to_value(value)?;
    edit(path, |root| set(root, section, key, value))
}

/// Sets `root.<section>.<key>` (creating the section).
pub fn set(root: &mut Map<String, Value>, section: &str, key: &str, value: Value) {
    let entry = root
        .entry(section.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(map) = entry {
        map.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_and_empty_object_mean_the_defaults() {
        let config = BarsConfig::parse("{}").unwrap();
        assert_eq!(config, BarsConfig::default());
        assert!(config.top.enabled);
        assert!(!config.dock.enabled);
        assert_eq!(config.taskbar.mode, TaskbarMode::Keep);
        assert_eq!(config.taskbar.alignment, None);
        assert_eq!(config.dock.monitors, Monitors::Primary);
        assert_eq!(config.top.monitors, Monitors::All);
    }

    #[test]
    fn parses_the_documented_example() {
        let json = r#"{
          "version": 1,
          "taskbar": { "mode": "hide", "alignment": "left" },
          "top": { "enabled": true, "monitors": ["\\\\.\\DISPLAY2"], "height": 28,
                   "left": ["start"], "center": ["clock"], "right": ["volume", "power"],
                   "clock": { "seconds": true } },
          "dock": { "enabled": true, "position": "left", "size": 56, "autohide": true,
                    "pinned": [ { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" } ] },
          "appearance": { "surface": "solid", "opacity": 0.9 }
        }"#;
        let config = BarsConfig::parse(json).unwrap();
        assert_eq!(config.taskbar.mode, TaskbarMode::Hide);
        assert_eq!(config.taskbar.alignment.map(Alignment::value), Some(0));
        assert!(config.top.monitors.includes(r"\\.\display2", false));
        assert!(!config.top.monitors.includes(r"\\.\DISPLAY1", true));
        assert_eq!(config.dock.position, DockPosition::Left);
        assert_eq!(config.dock.pinned.as_ref().map(Vec::len), Some(1));
        assert!(config.dock.magnify, "unset fields keep their defaults");
        assert_eq!(config.appearance.opacity, Some(0.9));
    }

    #[test]
    fn rejects_typos_bad_ranges_and_duplicates() {
        for bad in [
            r#"{ "dock": { "enabeld": true } }"#,
            r#"{ "taskbar": { "mode": "gone" } }"#,
            r#"{ "top": { "monitors": "secondary" } }"#,
            r#"{ "top": { "right": ["volume", "wifi"] } }"#,
            r#"{ "top": { "left": ["clock"], "right": ["clock"] } }"#,
            r#"{ "top": { "height": 4 } }"#,
            r#"{ "dock": { "size": 500 } }"#,
            r#"{ "dock": { "style": "ribbon" } }"#,
            r#"{ "dock": { "pinned": [ { "path": " " } ] } }"#,
            r#"{ "version": 2 }"#,
            r#"{ "appearance": { "opacity": 3 } }"#,
        ] {
            assert!(BarsConfig::parse(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn resolve_fills_the_clock_format_and_missing_pins_only() {
        let pins = || {
            vec![Pinned {
                path: "a.exe".into(),
                name: None,
            }]
        };
        let mut config = BarsConfig::default();
        config.resolve(pins);
        assert_eq!(config.top.clock.format.as_deref(), Some("ddd d MMM  HH:mm"));
        assert_eq!(config.dock.pinned, Some(pins()));

        let mut config = BarsConfig::parse(
            r#"{ "top": { "clock": { "seconds": true } }, "dock": { "pinned": [] } }"#,
        )
        .unwrap();
        config.resolve(pins);
        assert_eq!(
            config.top.clock.format.as_deref(),
            Some("ddd d MMM  HH:mm:ss")
        );
        assert_eq!(
            config.dock.pinned,
            Some(Vec::new()),
            "an empty list is a choice"
        );
    }

    #[test]
    fn serializes_for_the_page_without_the_user_appearance() {
        let mut config = BarsConfig::default();
        config.resolve(Vec::new);
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["top"]["monitors"], "all");
        assert_eq!(json["dock"]["position"], "bottom");
        assert_eq!(json["dock"]["style"], "floating");
        assert_eq!(
            DockStyle::Floating.band(48.0),
            64.0,
            "icons plus pill padding plus the edge gap"
        );
        assert_eq!(json["top"]["right"][0], "media");
        assert!(json.get("appearance").is_none());
        assert!(json["taskbar"].get("alignment").is_none());
    }

    #[test]
    fn edits_touch_only_what_they_set() {
        let dir = crate::files::temp_dir("config-edit");
        let path = dir.join(FILE);
        edit(&path, |root| {
            set(root, "dock", "enabled", Value::from(true))
        })
        .unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written,
            serde_json::json!({ "version": 1, "dock": { "enabled": true } })
        );

        std::fs::write(
            &path,
            r#"{ "top": { "height": 28 }, "dock": { "size": 40 } }"#,
        )
        .unwrap();
        set_field(&path, "taskbar", "mode", TaskbarMode::Hide).unwrap();
        let config = BarsConfig::load(&path).unwrap();
        assert_eq!(config.top.height, 28.0);
        assert_eq!(config.dock.size, 40.0);
        assert_eq!(config.taskbar.mode, TaskbarMode::Hide);

        // A broken file is never overwritten, and an edit can't produce one.
        std::fs::write(&path, r#"{ "top": { "heigth": 28 } }"#).unwrap();
        assert!(
            edit(&path, |root| set(
                root,
                "dock",
                "enabled",
                Value::from(true)
            ))
            .is_err()
        );
        std::fs::write(&path, "{}").unwrap();
        assert!(edit(&path, |root| set(root, "dock", "size", Value::from(1))).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
