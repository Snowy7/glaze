//! The page ⇄ host protocol (docs/bars.md, "Bridge"; the page's side is
//! `bars/shell/src/protocol.ts`): requests and their parsing, and which events each bar gets.

use std::collections::BTreeSet;

use mypaper_bars::Module;
use mypaper_bars::config::TopConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Which bar a window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BarKind {
    Top,
    Dock,
}

/// `{ id, method, params }` from the page.
#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum Method {
    Hello,
    Activate(Activate),
    Launch {
        path: String,
        #[serde(default, rename = "newInstance")]
        new_instance: bool,
    },
    Minimize {
        hwnd: isize,
    },
    Close {
        hwnd: isize,
    },
    Pin {
        path: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    Unpin {
        path: String,
    },
    Reorder {
        paths: Vec<String>,
    },
    Start,
    ActionCenter {
        panel: Panel,
    },
    Volume {
        #[serde(default)]
        set: Option<f32>,
        #[serde(default)]
        mute: Option<bool>,
        #[serde(default)]
        delta: Option<f32>,
    },
    Media {
        action: MediaAction,
    },
    Tray {
        id: String,
        button: Button,
    },
    Power {
        action: PowerAction,
    },
    Workspace {
        direction: Direction,
    },
    OpenSettings {
        uri: String,
    },
    Region {
        rects: Vec<[f64; 4]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Activate {
    App { app: String },
    Window { hwnd: isize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Panel {
    Notifications,
    Quick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaAction {
    Toggle,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerAction {
    Lock,
    Sleep,
    Signout,
    Restart,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Right,
}

impl Method {
    /// Parses a request's method and params.
    pub fn parse(method: &str, params: Value) -> Result<Self, BarError> {
        let request = if params.is_null() {
            json!({ "method": method })
        } else {
            json!({ "method": method, "params": params })
        };
        let parsed: Self = serde_json::from_value(request)
            .map_err(|e| BarError::invalid(format!("{method}: {e}")))?;
        parsed.check()?;
        Ok(parsed)
    }

    /// Range and shape checks serde can't express.
    fn check(&self) -> Result<(), BarError> {
        match self {
            Self::Volume { set, delta, .. } => {
                if set.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
                    return Err(BarError::invalid("volume.set must be between 0 and 1"));
                }
                if delta.is_some_and(|d| !(-1.0..=1.0).contains(&d)) {
                    return Err(BarError::invalid("volume.delta must be between -1 and 1"));
                }
            }
            Self::OpenSettings { uri } if !uri.starts_with("ms-settings:") => {
                return Err(BarError::invalid(
                    "openSettings takes ms-settings: URIs only",
                ));
            }
            Self::Launch { path, .. } | Self::Pin { path, .. } | Self::Unpin { path }
                if path.trim().is_empty() =>
            {
                return Err(BarError::invalid("a path is required"));
            }
            Self::Region { rects } if rects.len() > 64 => {
                return Err(BarError::invalid("at most 64 region rects"));
            }
            _ => {}
        }
        Ok(())
    }
}

/// `{ code, message }` for a failed request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BarError {
    pub code: &'static str,
    pub message: String,
}

impl BarError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "InvalidParams",
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "NotFound",
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "Unavailable",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "Internal",
            message: message.into(),
        }
    }
}

/// A stream of events a module needs. Events for a topic are only produced while some bar on
/// screen shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    Apps,
    Foreground,
    Clock,
    Media,
    Volume,
    Network,
    Battery,
    Tray,
    Workspaces,
    Stats,
}

impl Topic {
    pub fn event(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Foreground => "foreground",
            Self::Clock => "clock",
            Self::Media => "media",
            Self::Volume => "volume",
            Self::Network => "network",
            Self::Battery => "battery",
            Self::Tray => "tray",
            Self::Workspaces => "workspaces",
            Self::Stats => "stats",
        }
    }

    /// Driven by a timer (not by the system's own notifications), so it stops on bars that are
    /// idle while a fullscreen app is in front on another monitor.
    pub fn periodic(self) -> bool {
        self == Self::Stats
    }
}

/// The topic a top-bar module shows, if any.
pub fn module_topic(module: Module) -> Option<Topic> {
    match module {
        Module::Workspaces => Some(Topic::Workspaces),
        Module::App => Some(Topic::Foreground),
        Module::Clock => Some(Topic::Clock),
        Module::Media => Some(Topic::Media),
        Module::Tray => Some(Topic::Tray),
        Module::Network => Some(Topic::Network),
        Module::Volume => Some(Topic::Volume),
        Module::Battery => Some(Topic::Battery),
        Module::Stats => Some(Topic::Stats),
        Module::Start | Module::Quick | Module::Power => None,
    }
}

/// What a bar shows: the dock its apps, the top bar its modules' topics.
pub fn bar_topics(kind: BarKind, top: &TopConfig) -> BTreeSet<Topic> {
    match kind {
        BarKind::Dock => BTreeSet::from([Topic::Apps]),
        BarKind::Top => top.modules().filter_map(module_topic).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Method, BarError> {
        let envelope: Envelope = serde_json::from_str(json).unwrap();
        Method::parse(&envelope.method, envelope.params)
    }

    #[test]
    fn parses_every_request_shape() {
        assert_eq!(
            parse(r#"{ "id": 1, "method": "hello" }"#),
            Ok(Method::Hello)
        );
        assert_eq!(
            parse(r#"{ "id": 1, "method": "start", "params": null }"#),
            Ok(Method::Start)
        );
        assert_eq!(
            parse(r#"{ "id": 2, "method": "activate", "params": { "app": "pin:x" } }"#),
            Ok(Method::Activate(Activate::App {
                app: "pin:x".into()
            }))
        );
        assert_eq!(
            parse(r#"{ "id": 3, "method": "activate", "params": { "hwnd": 1234 } }"#),
            Ok(Method::Activate(Activate::Window { hwnd: 1234 }))
        );
        assert_eq!(
            parse(
                r#"{ "id": 4, "method": "launch", "params": { "path": "a.exe", "newInstance": true } }"#
            ),
            Ok(Method::Launch {
                path: "a.exe".into(),
                new_instance: true
            })
        );
        assert_eq!(
            parse(r#"{ "id": 5, "method": "actionCenter", "params": { "panel": "quick" } }"#),
            Ok(Method::ActionCenter {
                panel: Panel::Quick
            })
        );
        assert_eq!(
            parse(r#"{ "id": 6, "method": "volume", "params": { "delta": -0.02 } }"#),
            Ok(Method::Volume {
                set: None,
                mute: None,
                delta: Some(-0.02)
            })
        );
        assert_eq!(
            parse(r#"{ "id": 7, "method": "pin", "params": { "path": "a.lnk", "index": 0 } }"#),
            Ok(Method::Pin {
                path: "a.lnk".into(),
                name: None,
                index: Some(0)
            })
        );
        assert_eq!(
            parse(r#"{ "id": 8, "method": "region", "params": { "rects": [[0, 0, 100, 32]] } }"#),
            Ok(Method::Region {
                rects: vec![[0.0, 0.0, 100.0, 32.0]]
            })
        );
    }

    #[test]
    fn refuses_unknown_methods_and_bad_params() {
        for bad in [
            r#"{ "id": 1, "method": "format-c" }"#,
            r#"{ "id": 1, "method": "power", "params": { "action": "explode" } }"#,
            r#"{ "id": 1, "method": "volume", "params": { "set": 3 } }"#,
            r#"{ "id": 1, "method": "openSettings", "params": { "uri": "file:///C:/x.exe" } }"#,
            r#"{ "id": 1, "method": "launch", "params": { "path": "" } }"#,
            r#"{ "id": 1, "method": "minimize" }"#,
        ] {
            assert_eq!(
                parse(bad).map_err(|e| e.code),
                Err("InvalidParams"),
                "{bad}"
            );
        }
    }

    #[test]
    fn modules_route_to_their_topics() {
        let top = TopConfig::default();
        let topics = bar_topics(BarKind::Top, &top);
        assert_eq!(
            topics,
            BTreeSet::from([
                Topic::Workspaces,
                Topic::Foreground,
                Topic::Clock,
                Topic::Media,
                Topic::Tray,
                Topic::Network,
                Topic::Volume,
                Topic::Battery,
            ]),
            "start, quick and power need no data; stats isn't in the default layout"
        );
        let minimal = TopConfig {
            left: vec![Module::Start],
            center: vec![Module::Clock],
            right: vec![Module::Stats, Module::Power],
            ..TopConfig::default()
        };
        assert_eq!(
            bar_topics(BarKind::Top, &minimal),
            BTreeSet::from([Topic::Clock, Topic::Stats])
        );
        assert_eq!(
            bar_topics(BarKind::Dock, &minimal),
            BTreeSet::from([Topic::Apps])
        );
        assert!(Topic::Stats.periodic() && !Topic::Clock.periodic());
    }
}
