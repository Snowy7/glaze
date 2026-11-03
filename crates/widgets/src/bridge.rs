//! The messages between the shell and the host, and the widget methods they
//! carry (docs/widgets.md, "Widget JS API" and "Host ⇄ shell").
//!
//! Pure: parsing and the permission rule. Every widget call is checked
//! against the manifest the host read from disk, never against anything in
//! the message.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::manifest::{Permission, WidgetManifest};

/// A message from the shell page.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FromShell {
    Ready,
    Call {
        /// Attached by the shell from the sender's iframe, never by the widget.
        instance: String,
        id: Value,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Layout {
        changes: Vec<Change>,
    },
    Add {
        widget: String,
        cell: [u32; 2],
        size: [u32; 2],
    },
    Duplicate {
        instance: String,
        cell: [u32; 2],
    },
    Remove {
        instance: String,
    },
    Settings {
        instance: String,
        values: Map<String, Value>,
    },
    Edit {
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub instance: String,
    pub cell: [u32; 2],
    pub size: [u32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    PermissionDenied,
    NotFound,
    InvalidParams,
    PluginUnavailable,
    Internal,
}

/// The `error` of a failed widget call.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WidgetError {
    pub code: ErrorCode,
    pub message: String,
}

impl WidgetError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type Reply = Result<Value, WidgetError>;

/// Who waits for the reply to a widget call.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub instance: String,
    /// The bridge's call id, echoed back.
    pub id: Value,
}

/// What a background service (plugins, media) hands back to the host.
#[derive(Debug)]
pub enum Out {
    Reply(Target, Reply),
    /// A widget event for one instance.
    Event {
        instance: String,
        event: &'static str,
        data: Value,
    },
}

/// A widget call that the host answers (everything but `size.get`, which the
/// shell answers itself).
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    Hello,
    SettingsGet,
    StorageGet {
        key: String,
    },
    StorageSet {
        key: String,
        value: Option<Value>,
    },
    SystemStats,
    SystemSubscribe {
        interval_ms: u64,
    },
    SystemUnsubscribe {
        sub: u64,
    },
    MediaCurrent,
    MediaSubscribe,
    MediaUnsubscribe,
    MediaControl {
        action: MediaAction,
    },
    Launch {
        target: String,
    },
    ThemeCurrent,
    ThemeSubscribe,
    ThemeUnsubscribe,
    PluginCall {
        plugin: String,
        method: String,
        params: Option<Value>,
    },
    PluginSubscribe {
        plugin: String,
        event: String,
    },
    PluginUnsubscribe {
        plugin: String,
        event: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyParams {
    key: String,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
}

/// Distinguishes `"value": null` (store null) from no `value` (delete).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscribeParams {
    interval_ms: u64,
}

#[derive(Deserialize)]
struct SubParams {
    sub: u64,
}

#[derive(Deserialize)]
struct ActionParams {
    action: MediaAction,
}

#[derive(Deserialize)]
struct LaunchParams {
    target: String,
}

#[derive(Deserialize)]
struct PluginCallParams {
    plugin: String,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Deserialize)]
struct PluginEventParams {
    plugin: String,
    event: String,
}

/// The shortest `system.subscribe` interval.
pub const MIN_INTERVAL_MS: u64 = 1000;
const MAX_KEY: usize = 256;

impl Method {
    pub fn parse(method: &str, params: Value) -> Result<Self, WidgetError> {
        fn take<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, WidgetError> {
            serde_json::from_value(params)
                .map_err(|e| WidgetError::new(ErrorCode::InvalidParams, e.to_string()))
        }
        let invalid = |why: &str| Err(WidgetError::new(ErrorCode::InvalidParams, why));
        Ok(match method {
            "hello" => Self::Hello,
            "settings.get" => Self::SettingsGet,
            "storage.get" | "storage.set" => {
                let KeyParams { key, value } = take(params)?;
                if key.is_empty() || key.len() > MAX_KEY {
                    return invalid("storage keys are 1 to 256 characters");
                }
                match method {
                    "storage.get" => Self::StorageGet { key },
                    _ => Self::StorageSet { key, value },
                }
            }
            "system.stats" => Self::SystemStats,
            "system.subscribe" => {
                let SubscribeParams { interval_ms } = take(params)?;
                Self::SystemSubscribe {
                    interval_ms: interval_ms.max(MIN_INTERVAL_MS),
                }
            }
            "system.unsubscribe" => Self::SystemUnsubscribe {
                sub: take::<SubParams>(params)?.sub,
            },
            "media.current" => Self::MediaCurrent,
            "media.subscribe" => Self::MediaSubscribe,
            "media.unsubscribe" => Self::MediaUnsubscribe,
            "media.control" => Self::MediaControl {
                action: take::<ActionParams>(params)?.action,
            },
            "launch" => {
                let LaunchParams { target } = take(params)?;
                if target.trim().is_empty() || target.len() > 2048 || target.contains('\0') {
                    return invalid("launch needs a path or URL");
                }
                Self::Launch { target }
            }
            "theme.current" => Self::ThemeCurrent,
            "theme.subscribe" => Self::ThemeSubscribe,
            "theme.unsubscribe" => Self::ThemeUnsubscribe,
            "plugin.call" => {
                let PluginCallParams {
                    plugin,
                    method,
                    params,
                } = take(params)?;
                Self::PluginCall {
                    plugin,
                    method,
                    params,
                }
            }
            "plugin.subscribe" | "plugin.unsubscribe" => {
                let PluginEventParams { plugin, event } = take(params)?;
                match method {
                    "plugin.subscribe" => Self::PluginSubscribe { plugin, event },
                    _ => Self::PluginUnsubscribe { plugin, event },
                }
            }
            other => {
                return Err(WidgetError::new(
                    ErrorCode::NotFound,
                    format!("no method {other:?}"),
                ));
            }
        })
    }

    /// Whether `manifest` allows this call.
    pub fn check(&self, manifest: &WidgetManifest) -> Result<(), WidgetError> {
        let needs = match self {
            Self::SystemStats | Self::SystemSubscribe { .. } | Self::SystemUnsubscribe { .. } => {
                Permission::SystemStats
            }
            Self::MediaCurrent
            | Self::MediaSubscribe
            | Self::MediaUnsubscribe
            | Self::MediaControl { .. } => Permission::Media,
            Self::Launch { .. } => Permission::Launch,
            Self::ThemeCurrent | Self::ThemeSubscribe | Self::ThemeUnsubscribe => Permission::Theme,
            Self::PluginCall { plugin, .. }
            | Self::PluginSubscribe { plugin, .. }
            | Self::PluginUnsubscribe { plugin, .. } => {
                return if manifest.plugins.contains(plugin) {
                    Ok(())
                } else {
                    Err(WidgetError::new(
                        ErrorCode::PermissionDenied,
                        format!("{:?} doesn't list the plugin {plugin:?}", manifest.id),
                    ))
                };
            }
            Self::Hello | Self::SettingsGet | Self::StorageGet { .. } | Self::StorageSet { .. } => {
                return Ok(());
            }
        };
        if manifest.has(&needs) {
            Ok(())
        } else {
            Err(WidgetError::new(
                ErrorCode::PermissionDenied,
                format!(
                    "{:?} doesn't declare the {:?} permission",
                    manifest.id,
                    String::from(needs)
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn manifest(permissions: &str, plugins: &str) -> WidgetManifest {
        let json = format!(
            r#"{{ "id": "w", "name": "W", "size": {{ "default": [1, 1] }},
                  "permissions": {permissions}, "plugins": {plugins} }}"#
        );
        WidgetManifest::parse(&json, "w").unwrap()
    }

    fn parse(method: &str, params: Value) -> Method {
        Method::parse(method, params).unwrap()
    }

    fn code(result: Result<impl std::fmt::Debug, WidgetError>) -> ErrorCode {
        result.unwrap_err().code
    }

    #[test]
    fn shell_messages_parse() {
        let call: FromShell = serde_json::from_value(json!({
            "type": "call", "instance": "clock-1", "id": 4, "method": "storage.get", "params": { "key": "k" }
        }))
        .unwrap();
        assert!(matches!(call, FromShell::Call { ref instance, .. } if instance == "clock-1"));
        let layout: FromShell = serde_json::from_value(json!({
            "type": "layout", "changes": [{ "instance": "a-1", "cell": [1, 2], "size": [2, 1] }]
        }))
        .unwrap();
        assert!(matches!(layout, FromShell::Layout { ref changes } if changes[0].cell == [1, 2]));
        assert!(serde_json::from_value::<FromShell>(json!({ "type": "hack" })).is_err());
    }

    #[test]
    fn methods_parse_their_params() {
        assert_eq!(
            parse("storage.set", json!({ "key": "k", "value": null })),
            Method::StorageSet {
                key: "k".into(),
                value: Some(Value::Null)
            }
        );
        assert_eq!(
            parse("storage.set", json!({ "key": "k" })),
            Method::StorageSet {
                key: "k".into(),
                value: None
            }
        );
        assert_eq!(
            parse("system.subscribe", json!({ "intervalMs": 10 })),
            Method::SystemSubscribe {
                interval_ms: MIN_INTERVAL_MS
            }
        );
        assert_eq!(
            parse("media.control", json!({ "action": "next" })),
            Method::MediaControl {
                action: MediaAction::Next
            }
        );
        assert_eq!(parse("settings.get", Value::Null), Method::SettingsGet);
        assert_eq!(
            code(Method::parse("storage.get", json!({ "key": "" }))),
            ErrorCode::InvalidParams
        );
        assert_eq!(
            code(Method::parse("media.control", json!({ "action": "eject" }))),
            ErrorCode::InvalidParams
        );
        assert_eq!(
            code(Method::parse("launch", json!({ "target": " " }))),
            ErrorCode::InvalidParams
        );
        assert_eq!(
            code(Method::parse("fs.read", Value::Null)),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn undeclared_capabilities_are_denied() {
        let bare = manifest("[]", "[]");
        for method in [
            parse("system.stats", Value::Null),
            parse("media.current", Value::Null),
            parse("launch", json!({ "target": "https://example.com" })),
            parse("theme.current", Value::Null),
            parse(
                "plugin.call",
                json!({ "plugin": "weather", "method": "current" }),
            ),
        ] {
            assert_eq!(
                code(method.check(&bare)),
                ErrorCode::PermissionDenied,
                "{method:?}"
            );
        }
        // Always allowed: the widget's own state.
        for method in [
            parse("hello", Value::Null),
            parse("settings.get", Value::Null),
            parse("storage.get", json!({ "key": "k" })),
        ] {
            assert!(method.check(&bare).is_ok());
        }
    }

    #[test]
    fn declared_capabilities_are_allowed() {
        let full = manifest(
            r#"["system.stats", "media", "launch", "theme"]"#,
            r#"["weather"]"#,
        );
        assert!(
            parse("system.subscribe", json!({ "intervalMs": 2000 }))
                .check(&full)
                .is_ok()
        );
        assert!(
            parse("media.control", json!({ "action": "toggle" }))
                .check(&full)
                .is_ok()
        );
        assert!(
            parse(
                "plugin.subscribe",
                json!({ "plugin": "weather", "event": "updated" })
            )
            .check(&full)
            .is_ok()
        );
        let other_plugin = parse(
            "plugin.call",
            json!({ "plugin": "stocks", "method": "quote" }),
        );
        assert_eq!(code(other_plugin.check(&full)), ErrorCode::PermissionDenied);
    }
}
