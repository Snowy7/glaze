//! `plugin.json` as plugins and tooling see it (docs/plugins.md, "Folder and manifest").
//! The host parses the same file with `glaze_widgets::manifest::PluginManifest`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A grid size in cells: `[columns, rows]`.
pub type Cells = [u8; 2];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceSize {
    pub default: Cells,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Cells>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Cells>,
}

/// A widget, panel or bar module the plugin offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceSpec {
    pub id: String,
    pub name: String,
    pub size: FaceSize,
    /// A kit icon name for the picker and the header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A bar module's panel (one of `panels`): clicking the module opens it as a flyout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    /// Per-instance options (docs/plugins.md, "Face options"): entries of the widget settings
    /// schema (`boolean`, `number`, `select`, `string`, each with `key`, `label`, `default`).
    /// Kept raw; the shell validates them as the `<plugin>--<face>` widget's `settings`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `"<major>.<minor>"` of the SDK it was built with; absent for protocol-1 data plugins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub faces: Vec<FaceSpec>,
    #[serde(default)]
    pub panels: Vec<FaceSpec>,
    #[serde(default)]
    pub bars: Vec<FaceSpec>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The widget setting schema (docs/widgets.md); kept raw so the SDK needn't mirror it.
    #[serde(default)]
    pub settings: Vec<Value>,
}

/// A declared permission (docs/plugins.md, "Permissions").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    Secrets,
    Launch,
    Notifications,
    Location,
    Network(String),
    Process(String),
    Pipe(String),
}

impl Permission {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.split_once(':') {
            None => match text {
                "secrets" => Self::Secrets,
                "launch" => Self::Launch,
                "notifications" => Self::Notifications,
                "location" => Self::Location,
                _ => return None,
            },
            Some(("network", origin)) => Self::Network(origin.trim_end_matches('/').into()),
            Some(("process", name)) => Self::Process(name.to_ascii_lowercase()),
            Some(("pipe", name)) => Self::Pipe(name.to_ascii_lowercase()),
            Some(_) => return None,
        })
    }
}

/// Whether `declared` grants `wanted`. Network origins grant their own URLs and any port on a
/// declared bare host (`ws://127.0.0.1` grants `ws://127.0.0.1:3773/ws`).
pub fn allows(declared: &[String], wanted: &Permission) -> bool {
    declared
        .iter()
        .filter_map(|p| Permission::parse(p))
        .any(|have| match (&have, wanted) {
            (Permission::Network(origin), Permission::Network(url)) => {
                url.strip_prefix(origin.as_str()).is_some_and(|rest| {
                    rest.is_empty() || rest.starts_with(['/', ':', '?'])
                })
            }
            (Permission::Process(a), Permission::Process(b))
            | (Permission::Pipe(a), Permission::Pipe(b)) => a.eq_ignore_ascii_case(b),
            _ => have == *wanted,
        })
}

impl Manifest {
    pub fn allows(&self, wanted: &Permission) -> bool {
        allows(&self.permissions, wanted)
    }
}

/// Whether a host on SDK `major` can run a plugin built against `sdk` (`"1.2"`).
pub fn compatible(sdk: &str, major: u32) -> bool {
    sdk.split('.').next().and_then(|m| m.parse::<u32>().ok()) == Some(major)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_grant_only_what_was_declared() {
        let declared = ["secrets".into(), "network:ws://127.0.0.1".into(), "process:t3".into()];
        assert!(allows(&declared, &Permission::Secrets));
        assert!(allows(&declared, &Permission::Network("ws://127.0.0.1:3773/ws".into())));
        assert!(allows(&declared, &Permission::Process("T3".into())));
        assert!(!allows(&declared, &Permission::Launch));
        assert!(!allows(&declared, &Permission::Network("ws://127.0.0.10".into())));
        assert!(!allows(&declared, &Permission::Network("wss://evil.example".into())));
        assert!(!allows(&declared, &Permission::Process("cmd".into())));
        assert_eq!(Permission::parse("teleport"), None);
    }

    #[test]
    fn sdk_majors_must_match() {
        assert!(compatible("1.0", 1));
        assert!(compatible("1.7", 1));
        assert!(!compatible("2.0", 1));
        assert!(!compatible("one", 1));
    }
}
