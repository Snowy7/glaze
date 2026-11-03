//! `widget.json` and `plugin.json`: parsing, validation, permissions and
//! settings (docs/widgets.md).

use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::net::Origin;

pub const WIDGET_MANIFEST: &str = "widget.json";
pub const PLUGIN_MANIFEST: &str = "plugin.json";

/// Widget, plugin and instance ids: lowercase `[a-z0-9-]`, starting with a
/// letter or digit, so they're safe as folder, file and host names.
pub fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A path that stays inside its folder: relative, with no `..`.
fn inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// What a widget may do beyond drawing itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Permission {
    SystemStats,
    Media,
    Launch,
    Theme,
    ClipboardWrite,
    /// `fetch` to one origin.
    Network(Origin),
}

impl TryFrom<String> for Permission {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Ok(match text.as_str() {
            "system.stats" => Self::SystemStats,
            "media" => Self::Media,
            "launch" => Self::Launch,
            "theme" => Self::Theme,
            "clipboard.write" => Self::ClipboardWrite,
            other => match other.strip_prefix("network:").map(Origin::parse) {
                Some(Some(origin)) => Self::Network(origin),
                Some(None) => {
                    return Err(format!("{other:?}: expected network:scheme://host[:port]"));
                }
                None => return Err(format!("unknown permission {other:?}")),
            },
        })
    }
}

impl From<Permission> for String {
    fn from(permission: Permission) -> Self {
        match permission {
            Permission::SystemStats => "system.stats".into(),
            Permission::Media => "media".into(),
            Permission::Launch => "launch".into(),
            Permission::Theme => "theme".into(),
            Permission::ClipboardWrite => "clipboard.write".into(),
            Permission::Network(origin) => format!("network:{origin}"),
        }
    }
}

/// One entry of a manifest's `settings`, rendered as a form by the shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Setting {
    Boolean {
        key: String,
        label: String,
        default: bool,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        default: f64,
    },
    String {
        key: String,
        label: String,
        default: String,
    },
    Select {
        key: String,
        label: String,
        options: Vec<String>,
        default: String,
    },
    Color {
        key: String,
        label: String,
        default: String,
    },
}

/// `#rrggbb`.
fn is_color(text: &str) -> bool {
    text.strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

impl Setting {
    pub fn key(&self) -> &str {
        match self {
            Self::Boolean { key, .. }
            | Self::Number { key, .. }
            | Self::String { key, .. }
            | Self::Select { key, .. }
            | Self::Color { key, .. } => key,
        }
    }

    pub fn default_value(&self) -> Value {
        match self {
            Self::Boolean { default, .. } => (*default).into(),
            Self::Number { default, .. } => (*default).into(),
            Self::String { default, .. }
            | Self::Select { default, .. }
            | Self::Color { default, .. } => default.clone().into(),
        }
    }

    /// Whether `value` is a valid value for this setting.
    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Boolean { .. }, Value::Bool(_)) => true,
            (Self::Number { min, max, .. }, Value::Number(n)) => n.as_f64().is_some_and(|n| {
                n.is_finite() && min.is_none_or(|min| n >= min) && max.is_none_or(|max| n <= max)
            }),
            (Self::String { .. }, Value::String(s)) => s.len() <= 4096,
            (Self::Select { options, .. }, Value::String(s)) => options.contains(s),
            (Self::Color { .. }, Value::String(s)) => is_color(s),
            _ => false,
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.key().is_empty(), "a setting has an empty key");
        if let Self::Number { min, max, step, .. } = self {
            ensure!(
                min.zip(*max).is_none_or(|(min, max)| min <= max),
                "setting {:?}: min > max",
                self.key()
            );
            ensure!(
                step.is_none_or(|step| step > 0.0),
                "setting {:?}: step must be positive",
                self.key()
            );
        }
        if let Self::Select { options, .. } = self {
            ensure!(
                !options.is_empty(),
                "setting {:?} has no options",
                self.key()
            );
        }
        ensure!(
            self.accepts(&self.default_value()),
            "setting {:?}: the default isn't a valid value",
            self.key()
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeSpec {
    pub default: [u32; 2],
    #[serde(default = "one_by_one")]
    pub min: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<[u32; 2]>,
}

fn one_by_one() -> [u32; 2] {
    [1, 1]
}

fn default_entry() -> String {
    "index.html".into()
}

fn yes() -> bool {
    true
}

/// A widget's `widget.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetManifest {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    pub size: SizeSpec,
    #[serde(default = "yes")]
    pub resizable: bool,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub settings: Vec<Setting>,
}

impl WidgetManifest {
    /// Reads and validates `<dir>/widget.json`; `dir`'s name must be the id.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(WIDGET_MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        Self::parse(&text, folder).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(json: &str, folder: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate(folder)?;
        Ok(manifest)
    }

    fn validate(&self, folder: &str) -> Result<()> {
        ensure!(
            valid_id(&self.id),
            "id {:?} must be lowercase [a-z0-9-]",
            self.id
        );
        ensure!(
            self.id == folder,
            "id {:?} must match its folder name {folder:?}",
            self.id
        );
        ensure!(!self.name.trim().is_empty(), "name is empty");
        ensure!(
            inside(&self.entry),
            "entry {:?} must be a path inside the widget folder",
            self.entry
        );
        let SizeSpec { default, min, max } = &self.size;
        let fits = |low: &[u32; 2], high: &[u32; 2]| low[0] <= high[0] && low[1] <= high[1];
        ensure!(
            min[0] >= 1 && min[1] >= 1,
            "size.min must be at least [1, 1]"
        );
        ensure!(fits(min, default), "size: min must be <= default");
        ensure!(
            max.is_none_or(|max| fits(default, &max)),
            "size: default must be <= max"
        );
        for plugin in &self.plugins {
            ensure!(
                valid_id(plugin),
                "plugin id {plugin:?} must be lowercase [a-z0-9-]"
            );
        }
        let mut keys = std::collections::HashSet::new();
        for setting in &self.settings {
            setting.validate()?;
            ensure!(
                keys.insert(setting.key()),
                "setting {:?} appears twice",
                setting.key()
            );
        }
        Ok(())
    }

    pub fn has(&self, permission: &Permission) -> bool {
        self.permissions.contains(permission)
    }

    /// The origins of every `network:<origin>` permission.
    pub fn network_origins(&self) -> impl Iterator<Item = &Origin> {
        self.permissions.iter().filter_map(|p| match p {
            Permission::Network(origin) => Some(origin),
            _ => None,
        })
    }

    /// A size in cells the manifest allows: `default` when not resizable,
    /// else clamped to `min`/`max`.
    pub fn clamp_size(&self, size: [u32; 2]) -> [u32; 2] {
        if !self.resizable {
            return self.size.default;
        }
        let max = self.size.max.unwrap_or([u32::MAX; 2]);
        [0, 1].map(|i| size[i].clamp(self.size.min[i], max[i]))
    }

    /// Every declared setting with its value: the saved one if it's valid,
    /// else the default. Unknown saved keys are dropped.
    pub fn resolve_settings(&self, saved: &Map<String, Value>) -> Map<String, Value> {
        self.settings
            .iter()
            .map(|setting| {
                let value = saved
                    .get(setting.key())
                    .filter(|v| setting.accepts(v))
                    .cloned()
                    .unwrap_or_else(|| setting.default_value());
                (setting.key().to_owned(), value)
            })
            .collect()
    }

    /// Checks a settings edit from the shell: every key declared, every value valid.
    pub fn check_settings(&self, values: &Map<String, Value>) -> Result<()> {
        for (key, value) in values {
            let Some(setting) = self.settings.iter().find(|s| s.key() == key) else {
                bail!("{:?} has no setting {key:?}", self.id);
            };
            ensure!(
                setting.accepts(value),
                "{value} isn't a valid value for {key:?}"
            );
        }
        Ok(())
    }
}

/// A plugin's `plugin.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// The executable, relative to the plugin folder.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    /// Documentation for users; plugins are trusted native code.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl PluginManifest {
    /// Reads and validates `<dir>/plugin.json`; `dir`'s name must be the id.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(PLUGIN_MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        Self::parse(&text, folder).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(json: &str, folder: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(json)?;
        ensure!(
            valid_id(&manifest.id),
            "id {:?} must be lowercase [a-z0-9-]",
            manifest.id
        );
        ensure!(
            manifest.id == folder,
            "id {:?} must match its folder name {folder:?}",
            manifest.id
        );
        ensure!(
            inside(&manifest.command),
            "command {:?} must be a path inside the plugin folder",
            manifest.command
        );
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CLOCK: &str = r##"{
        "id": "clock", "name": "Clock", "version": "1.0.0",
        "size": { "default": [2, 1], "min": [1, 1], "max": [4, 2] },
        "permissions": ["system.stats", "network:https://api.open-meteo.com"],
        "settings": [
            { "key": "format24", "type": "boolean", "label": "24-hour", "default": true },
            { "key": "accent", "type": "color", "label": "Accent", "default": "#ffffff" },
            { "key": "density", "type": "select", "label": "Density", "options": ["cozy", "compact"], "default": "cozy" },
            { "key": "opacity", "type": "number", "label": "Opacity", "min": 0, "max": 1, "step": 0.05, "default": 1 }
        ]
    }"##;

    fn clock() -> WidgetManifest {
        WidgetManifest::parse(CLOCK, "clock").unwrap()
    }

    #[test]
    fn ids() {
        assert!(valid_id("system-monitor") && valid_id("clock-12"));
        for bad in ["", "-x", "Clock", "a_b", "a.b", "a b", &"x".repeat(65)] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn parses_with_defaults() {
        let clock = clock();
        assert_eq!(clock.entry, "index.html");
        assert!(clock.resizable && clock.plugins.is_empty());
        assert!(clock.has(&Permission::SystemStats) && !clock.has(&Permission::Media));
        let origins: Vec<&Origin> = clock.network_origins().collect();
        assert_eq!(
            origins,
            [&Origin::parse("https://api.open-meteo.com:443").unwrap()]
        );
    }

    #[test]
    fn typos_and_bad_values_fail_loudly() {
        let with = |field: &str| CLOCK.replacen("\"version\": \"1.0.0\"", field, 1);
        for (json, why) in [
            (with("\"permisions\": []"), "unknown field"),
            (with("\"entry\": \"../x.html\""), "entry escapes"),
            (
                CLOCK.replace("system.stats", "system.stat"),
                "unknown permission",
            ),
            (
                CLOCK.replace("https://api.open-meteo.com", "https://a.com/path"),
                "origin with a path",
            ),
            (CLOCK.replace("[4, 2]", "[1, 2]"), "default > max"),
            (
                CLOCK.replace("\"default\": \"cozy\"", "\"default\": \"roomy\""),
                "select default",
            ),
            (CLOCK.replace("#ffffff", "white"), "color default"),
            (
                CLOCK.replace("\"type\": \"boolean\"", "\"type\": \"bool\""),
                "setting type",
            ),
        ] {
            assert!(WidgetManifest::parse(&json, "clock").is_err(), "{why}");
        }
        assert!(
            WidgetManifest::parse(CLOCK, "other").is_err(),
            "folder name must match"
        );
    }

    #[test]
    fn permissions_round_trip_as_strings() {
        let json = serde_json::to_value(clock().permissions).unwrap();
        assert_eq!(
            json,
            json!(["system.stats", "network:https://api.open-meteo.com"])
        );
    }

    #[test]
    fn sizes_clamp_to_the_manifest() {
        let mut clock = clock();
        assert_eq!(clock.clamp_size([9, 0]), [4, 1]);
        assert_eq!(clock.clamp_size([3, 2]), [3, 2]);
        clock.size.max = None;
        assert_eq!(clock.clamp_size([9, 9]), [9, 9]);
        clock.resizable = false;
        assert_eq!(clock.clamp_size([3, 2]), [2, 1]);
    }

    #[test]
    fn settings_resolve_to_valid_values() {
        let saved = json!({ "format24": false, "accent": "red", "opacity": 3, "density": "compact", "gone": 1 });
        let resolved = clock().resolve_settings(saved.as_object().unwrap());
        assert_eq!(
            Value::Object(resolved),
            json!({ "format24": false, "accent": "#ffffff", "opacity": 1.0, "density": "compact" })
        );
    }

    #[test]
    fn settings_edits_are_checked() {
        let ok = json!({ "opacity": 0.5, "accent": "#00ff00" });
        assert!(clock().check_settings(ok.as_object().unwrap()).is_ok());
        for bad in [
            json!({ "opacity": 2 }),
            json!({ "nope": 1 }),
            json!({ "format24": "yes" }),
        ] {
            assert!(
                clock().check_settings(bad.as_object().unwrap()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn plugin_manifests() {
        let json = r#"{ "id": "weather", "name": "Weather", "command": "weather.exe",
                        "methods": ["current"], "events": ["updated"],
                        "permissions": ["network:https://api.open-meteo.com"] }"#;
        let plugin = PluginManifest::parse(json, "weather").unwrap();
        assert_eq!(plugin.methods, ["current"]);
        assert!(
            PluginManifest::parse(&json.replace("weather.exe", "..\\x.exe"), "weather").is_err()
        );
        assert!(
            PluginManifest::parse(&json.replace("weather.exe", "C:\\x.exe"), "weather").is_err()
        );
    }
}
