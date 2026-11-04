//! The page ⇄ host protocol (docs/bars.md, "Bridge"; the page's side is
//! `bars/shell/src/protocol.ts`): requests and their parsing, and which events each bar gets.

use std::collections::BTreeSet;

use mypaper_bars::config::{Extra, Trigger};
use mypaper_bars::{BarsConfig, Module, ModuleRef};
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
    /// A flyout could not open: log it and open the native panel instead (`FlyoutName::native`).
    FlyoutFailed {
        flyout: FlyoutName,
        error: String,
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
        /// The icon's rect in the bar (DIPs from the window's top-left), for apps that place
        /// their popup beside their icon.
        #[serde(default)]
        rect: Option<[f64; 4]>,
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
    /// A flyout's request (`crate::flyouts::Request`, the methods in `flyouts::METHODS`).
    #[serde(skip)]
    Flyout(crate::flyouts::Request),
    /// Runs the `exec` or `open` action `bars.json` binds to a module's input.
    Run {
        module: ModuleRef,
        trigger: Trigger,
    },
    /// Windows Search (Win+S).
    Search,
    /// Minimizes every window, or brings them back (Win+D).
    ShowDesktop,
    Trash {
        action: TrashAction,
    },
    /// Registers thumbnails of these windows into the bar; the result is their sizes.
    PreviewOpen {
        hwnds: Vec<isize>,
    },
    /// Shows each registered thumbnail at a rect (DIPs); unlisted ones hide.
    PreviewPlace {
        windows: Vec<PreviewWindow>,
    },
    PreviewClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewWindow {
    pub hwnd: isize,
    pub rect: [f64; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrashAction {
    Open,
    Empty,
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

/// The flyouts the page opens for module clicks and built-in actions
/// (`bars/shell/src/flyouts/protocol.ts` → `FlyoutName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlyoutName {
    Launcher,
    Quick,
    Network,
    Mixer,
    Calendar,
    Notifications,
}

/// The native Windows panel that stands in for a flyout that could not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    StartMenu,
    ActionCenter(Panel),
    Settings(&'static str),
}

impl FlyoutName {
    pub fn native(self) -> Native {
        match self {
            Self::Launcher => Native::StartMenu,
            Self::Quick => Native::ActionCenter(Panel::Quick),
            // The notification center holds the calendar.
            Self::Notifications | Self::Calendar => Native::ActionCenter(Panel::Notifications),
            Self::Network => Native::Settings("ms-settings:network"),
            Self::Mixer => Native::Settings("ms-settings:sound"),
        }
    }
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
    Middle,
    Double,
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
        if crate::flyouts::METHODS.contains(&method) {
            return crate::flyouts::Request::parse(method, params).map(Self::Flyout);
        }
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
            Self::PreviewOpen { hwnds } if hwnds.len() > 16 => {
                return Err(BarError::invalid("at most 16 previews"));
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
    /// The keyboard layout of the app in front.
    Language,
    /// How many notifications there are.
    Notifications,
    /// Custom modules' outputs.
    Custom,
    /// Whether the Recycle Bin is full (the dock's trash item).
    Trash,
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
            Self::Language => "language",
            Self::Notifications => "notifications",
            Self::Custom => "custom",
            Self::Trash => "trash",
        }
    }

    /// Driven by a timer (not by the system's own notifications), so it stops on bars that are
    /// idle while a fullscreen app is in front on another monitor.
    pub fn periodic(self) -> bool {
        // Notifications are re-counted on a timer: change events need package identity.
        matches!(self, Self::Stats | Self::Custom | Self::Notifications)
    }
}

/// The topic a module shows, if any.
pub fn module_topic(module: &ModuleRef) -> Option<Topic> {
    let ModuleRef::Builtin(module) = module else {
        return matches!(module, ModuleRef::Custom(_)).then_some(Topic::Custom);
    };
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
        Module::Language => Some(Topic::Language),
        Module::Notifications => Some(Topic::Notifications),
        Module::Start | Module::Search | Module::Quick | Module::Power => None,
    }
}

/// The modules a bar of `config` draws: the top bar's zones, or a `taskbar` dock's sides.
pub fn bar_modules(kind: BarKind, config: &BarsConfig) -> impl Iterator<Item = &ModuleRef> + '_ {
    let (top, dock) = match kind {
        BarKind::Top => (Some(config.top.modules()), None),
        BarKind::Dock => (None, Some(config.dock.modules())),
    };
    top.into_iter().flatten().chain(dock.into_iter().flatten())
}

/// What a bar shows: the dock its apps (and a taskbar's modules, the trash), the top bar its
/// modules' topics.
pub fn bar_topics(kind: BarKind, config: &BarsConfig) -> BTreeSet<Topic> {
    let mut topics: BTreeSet<Topic> = bar_modules(kind, config).filter_map(module_topic).collect();
    if kind == BarKind::Dock {
        topics.insert(Topic::Apps);
        if config.dock.extras.contains(&Extra::Trash) {
            topics.insert(Topic::Trash);
        }
    }
    topics
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Method, BarError> {
        let envelope: Envelope = serde_json::from_str(json).unwrap();
        Method::parse(&envelope.method, envelope.params)
    }

    #[test]
    fn a_failed_flyout_falls_back_to_its_native_panel() {
        let fallback = |flyout: &str| match parse(&format!(
            r#"{{ "id": 1, "method": "flyoutFailed", "params": {{ "flyout": "{flyout}", "error": "boom" }} }}"#
        )) {
            Ok(Method::FlyoutFailed { flyout, .. }) => flyout.native(),
            other => panic!("{flyout}: {other:?}"),
        };
        assert_eq!(fallback("launcher"), Native::StartMenu);
        assert_eq!(fallback("quick"), Native::ActionCenter(Panel::Quick));
        assert_eq!(
            fallback("notifications"),
            Native::ActionCenter(Panel::Notifications)
        );
        assert_eq!(
            fallback("calendar"),
            Native::ActionCenter(Panel::Notifications)
        );
        assert_eq!(fallback("network"), Native::Settings("ms-settings:network"));
        assert_eq!(fallback("mixer"), Native::Settings("ms-settings:sound"));
        assert!(
            parse(r#"{ "id": 1, "method": "flyoutFailed", "params": { "flyout": "tray", "error": "" } }"#)
                .is_err(),
            "only the flyouts a module or action opens have a native panel"
        );
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
    fn new_requests_parse() {
        assert_eq!(
            parse(
                r#"{ "id": 1, "method": "run", "params": { "module": "custom:temp", "trigger": "wheelUp" } }"#
            ),
            Ok(Method::Run {
                module: ModuleRef::Custom("temp".into()),
                trigger: Trigger::WheelUp
            })
        );
        assert_eq!(
            parse(
                r#"{ "id": 2, "method": "previewPlace", "params": { "windows": [ { "hwnd": 7, "rect": [1, 2, 3, 4] } ] } }"#
            ),
            Ok(Method::PreviewPlace {
                windows: vec![PreviewWindow {
                    hwnd: 7,
                    rect: [1.0, 2.0, 3.0, 4.0]
                }]
            })
        );
        assert_eq!(
            parse(r#"{ "id": 3, "method": "trash", "params": { "action": "empty" } }"#),
            Ok(Method::Trash {
                action: TrashAction::Empty
            })
        );
        assert_eq!(
            parse(r#"{ "id": 4, "method": "showDesktop" }"#),
            Ok(Method::ShowDesktop)
        );
        assert!(parse(r#"{ "id": 5, "method": "run", "params": { "module": "rm -rf", "trigger": "left" } }"#).is_err());
    }

    #[test]
    fn modules_route_to_their_topics() {
        let config = BarsConfig::default();
        let topics = bar_topics(BarKind::Top, &config);
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
        let minimal = BarsConfig::parse(
            r#"{ "top": { "left": ["start"], "center": ["clock"], "right": ["stats", "|", "custom:x", "power"] },
                 "modules": { "custom:x": { "command": "x.exe" } } }"#,
        )
        .unwrap();
        assert_eq!(
            bar_topics(BarKind::Top, &minimal),
            BTreeSet::from([Topic::Clock, Topic::Stats, Topic::Custom])
        );
        assert_eq!(
            bar_topics(BarKind::Dock, &minimal),
            BTreeSet::from([Topic::Apps]),
            "a floating dock draws no modules"
        );
        let taskbar = BarsConfig::parse(
            r#"{ "dock": { "style": "taskbar", "left": ["start"], "right": ["clock"], "extras": ["trash"] } }"#,
        )
        .unwrap();
        assert_eq!(
            bar_topics(BarKind::Dock, &taskbar),
            BTreeSet::from([Topic::Apps, Topic::Clock, Topic::Trash])
        );
        assert!(Topic::Stats.periodic() && Topic::Custom.periodic() && !Topic::Clock.periodic());
    }
}
