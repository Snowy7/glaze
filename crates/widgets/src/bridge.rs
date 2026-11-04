//! The messages between the shell and the host, and the widget methods they
//! carry (docs/widgets.md, "Wire protocol" and "Host ⇄ shell").
//!
//! Pure: parsing and the permission rule. Every widget call is checked
//! against the manifest the host read from disk, never against anything in
//! the message (the shell checks first too, but the host doesn't rely on it).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::manifest::{Permission, WidgetManifest, valid_id};
use crate::pick;

/// A message from the shell page.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FromShell {
    Ready,
    /// A frame (re)loaded and shook hands with the shell: its old subscriptions are gone.
    Hello {
        instance: String,
    },
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
    /// Edits an appearance layer: an instance's own, or the user's when `instance` is absent.
    /// A `null` value clears that field ("Use default").
    Appearance {
        #[serde(default)]
        instance: Option<String>,
        values: Map<String, Value>,
    },
    Edit {
        on: bool,
    },
    /// A `file`/`image` setting asks for the file dialog; the host sets the setting to the
    /// picked file's URL. The dialog's filter comes from the manifest, never from the message,
    /// so `extensions` is only parsed.
    #[serde(rename = "pickFile")]
    PickFile {
        instance: String,
        key: String,
        #[serde(default)]
        #[allow(dead_code)]
        extensions: Option<Vec<String>>,
    },
    /// The cards of the liquid widgets this shell shows, as they are on screen now (sent when
    /// they change, every frame of a drag at most). See `plates.rs`.
    Plates {
        plates: Vec<crate::plates::Card>,
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

impl ErrorCode {
    /// The code a plugin picked by name in `error.data.code`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "PermissionDenied" => Self::PermissionDenied,
            "NotFound" => Self::NotFound,
            "InvalidParams" => Self::InvalidParams,
            "PluginUnavailable" => Self::PluginUnavailable,
            "Internal" => Self::Internal,
            _ => return None,
        })
    }
}

/// A widget request the host answers: every wire method but `hello`, which
/// the shell answers itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// `shared` names a shared namespace (permission `storage.shared:<ns>`) instead of the
    /// instance's own storage.
    StorageGet {
        key: String,
        shared: Option<String>,
    },
    StorageSet {
        key: String,
        value: Option<Value>,
        shared: Option<String>,
    },
    SystemStats,
    MediaCurrent,
    MediaControl {
        action: MediaAction,
    },
    Launch {
        target: String,
    },
    ThemeCurrent,
    /// The signed-in user's display name and account name.
    UserInfo,
    /// The widget saves some of its own settings (checked against its manifest, like a form edit).
    SettingsSet {
        values: Map<String, Value>,
    },
    /// The file dialog; the pick is copied into the instance's data folder.
    PickFile {
        extensions: Vec<String>,
        title: Option<String>,
    },
    /// The icon of an exe, file, folder or `shell:AppsFolder\<AUMID>`, as a PNG data URL.
    AppIcon {
        target: String,
        size: u32,
    },
    Plugin {
        plugin: String,
        method: String,
        params: Option<Value>,
    },
    /// `sub` is the widget's own id for the subscription.
    Subscribe {
        sub: String,
        topic: Topic,
    },
    Unsubscribe {
        sub: String,
    },
}

/// What a subscription delivers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "topic", rename_all = "lowercase")]
pub enum Topic {
    /// A [`SystemStats`](crate::stats::SystemStats) sample right away, then every interval.
    System {
        #[serde(rename = "intervalMs")]
        interval_ms: u64,
    },
    /// The media state, on change.
    Media,
    /// The theme, on change.
    Theme,
    /// Each matching plugin event's data.
    Plugin { plugin: String, event: String },
    /// The audio spectrum, `bins` floats per frame (clamped to [`AUDIO_BINS`]).
    Audio {
        #[serde(default = "default_bins", deserialize_with = "clamped_bins")]
        bins: usize,
    },
}

/// The fewest, most and default number of bins a spectrum subscriber can ask for.
pub const AUDIO_BINS: std::ops::RangeInclusive<usize> = 64..=128;
const DEFAULT_BINS: usize = 64;

fn default_bins() -> usize {
    DEFAULT_BINS
}

/// Any number is accepted and clamped, as the spec says.
fn clamped_bins<'de, D: serde::Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let wanted = f64::deserialize(d)?;
    Ok((wanted.round() as usize).clamp(*AUDIO_BINS.start(), *AUDIO_BINS.end()))
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
struct KeyParams {
    key: String,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
    #[serde(default)]
    shared: Option<String>,
}

#[derive(Deserialize, Default)]
struct PickParams {
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Deserialize)]
struct IconParams {
    target: String,
    #[serde(default)]
    size: Option<f64>,
}

/// The sizes `appIcon` can make.
const ICON_SIZES: std::ops::RangeInclusive<u32> = 16..=256;
const DEFAULT_ICON_SIZE: u32 = 64;
const MAX_TITLE: usize = 200;
const MAX_EXTENSIONS: usize = 32;

/// Distinguishes `"value": null` (store null) from no `value` (delete).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct SubscribeParams {
    sub: String,
    #[serde(flatten)]
    topic: Topic,
}

#[derive(Deserialize)]
struct SubParams {
    sub: String,
}

#[derive(Deserialize)]
struct ActionParams {
    action: MediaAction,
}

#[derive(Deserialize)]
struct SettingsParams {
    values: Map<String, Value>,
}

#[derive(Deserialize)]
struct LaunchParams {
    target: String,
}

#[derive(Deserialize)]
struct PluginParams {
    plugin: String,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// The shortest `system` subscription interval.
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
            "storage.get" | "storage.set" => {
                let KeyParams { key, value, shared } = take(params)?;
                if key.is_empty() || key.len() > MAX_KEY {
                    return invalid("storage keys are 1 to 256 characters");
                }
                if shared.as_deref().is_some_and(|ns| !valid_id(ns)) {
                    return invalid("shared namespaces are lowercase [a-z0-9-]");
                }
                match method {
                    "storage.get" => Self::StorageGet { key, shared },
                    _ => Self::StorageSet { key, value, shared },
                }
            }
            "user.info" => Self::UserInfo,
            "settings.set" => {
                let SettingsParams { values } = take(params)?;
                Self::SettingsSet { values }
            }
            "pickFile" => {
                let PickParams { extensions, title } = if params.is_null() {
                    PickParams::default()
                } else {
                    take(params)?
                };
                if extensions.len() > MAX_EXTENSIONS
                    || !extensions.iter().all(|e| pick::valid_extension(e))
                {
                    return invalid("extensions are letters and digits without a dot");
                }
                if title
                    .as_ref()
                    .is_some_and(|t| t.chars().count() > MAX_TITLE)
                {
                    return invalid("the title is too long");
                }
                Self::PickFile { extensions, title }
            }
            "appIcon" => {
                let IconParams { target, size } = take(params)?;
                if target.trim().is_empty() || target.len() > 2048 || target.contains('\0') {
                    return invalid("appIcon needs a path or an AppsFolder target");
                }
                let size = size.map_or(DEFAULT_ICON_SIZE, |s| {
                    (s.round() as u32).clamp(*ICON_SIZES.start(), *ICON_SIZES.end())
                });
                Self::AppIcon { target, size }
            }
            "system.stats" => Self::SystemStats,
            "media.current" => Self::MediaCurrent,
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
            "plugin" => {
                let PluginParams {
                    plugin,
                    method,
                    params,
                } = take(params)?;
                Self::Plugin {
                    plugin,
                    method,
                    params,
                }
            }
            "subscribe" => {
                let SubscribeParams { sub, mut topic } = take(params)?;
                if sub.is_empty() || sub.len() > MAX_KEY {
                    return invalid("subscription ids are 1 to 256 characters");
                }
                if let Topic::System { interval_ms } = &mut topic {
                    *interval_ms = (*interval_ms).max(MIN_INTERVAL_MS);
                }
                Self::Subscribe { sub, topic }
            }
            "unsubscribe" => Self::Unsubscribe {
                sub: take::<SubParams>(params)?.sub,
            },
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
            Self::SystemStats
            | Self::Subscribe {
                topic: Topic::System { .. },
                ..
            } => Permission::SystemStats,
            Self::MediaCurrent
            | Self::MediaControl { .. }
            | Self::Subscribe {
                topic: Topic::Media,
                ..
            } => Permission::Media,
            Self::Launch { .. } | Self::AppIcon { .. } => Permission::Launch,
            Self::ThemeCurrent
            | Self::Subscribe {
                topic: Topic::Theme,
                ..
            } => Permission::Theme,
            Self::UserInfo => Permission::User,
            Self::Subscribe {
                topic: Topic::Audio { .. },
                ..
            } => Permission::Audio,
            Self::StorageGet {
                shared: Some(namespace),
                ..
            }
            | Self::StorageSet {
                shared: Some(namespace),
                ..
            } => Permission::StorageShared(namespace.clone()),
            Self::Plugin { plugin, .. }
            | Self::Subscribe {
                topic: Topic::Plugin { plugin, .. },
                ..
            } => {
                return if manifest.plugins.contains(plugin) {
                    Ok(())
                } else {
                    Err(WidgetError::new(
                        ErrorCode::PermissionDenied,
                        format!("{:?} doesn't list the plugin {plugin:?}", manifest.id),
                    ))
                };
            }
            // The widget's own state, letting go, and the dialog (which is the consent).
            Self::StorageGet { .. }
            | Self::StorageSet { .. }
            | Self::SettingsSet { .. }
            | Self::Unsubscribe { .. }
            | Self::PickFile { .. } => return Ok(()),
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
        let hello: FromShell =
            serde_json::from_value(json!({ "type": "hello", "instance": "a-1" })).unwrap();
        assert_eq!(
            hello,
            FromShell::Hello {
                instance: "a-1".into()
            }
        );
        assert!(serde_json::from_value::<FromShell>(json!({ "type": "hack" })).is_err());
        let plates: FromShell = serde_json::from_value(json!({
            "type": "plates", "plates": [{ "instance": "a-1", "rect": [1, 2, 30, 40], "radius": 9 }]
        }))
        .unwrap();
        assert!(matches!(plates, FromShell::Plates { ref plates } if plates[0].rect == [1.0, 2.0, 30.0, 40.0]));

        // Appearance edits: the user's layer has no instance; null clears a field.
        let user: FromShell = serde_json::from_value(
            json!({ "type": "appearance", "values": { "blur": 8, "radius": null } }),
        )
        .unwrap();
        assert!(
            matches!(user, FromShell::Appearance { instance: None, ref values } if values.len() == 2)
        );
        let own: FromShell = serde_json::from_value(
            json!({ "type": "appearance", "instance": "a-1", "values": { "text": "dark" } }),
        )
        .unwrap();
        assert!(matches!(own, FromShell::Appearance { instance: Some(ref i), .. } if i == "a-1"));
    }

    #[test]
    fn methods_parse_their_params() {
        assert_eq!(
            parse("settings.set", json!({ "values": { "city": null } })),
            Method::SettingsSet {
                values: Map::from_iter([("city".to_owned(), Value::Null)])
            }
        );
        assert!(Method::parse("settings.set", json!({ "city": null })).is_err());
        assert_eq!(
            parse("storage.set", json!({ "key": "k", "value": null })),
            Method::StorageSet {
                key: "k".into(),
                value: Some(Value::Null),
                shared: None
            }
        );
        assert_eq!(
            parse("storage.set", json!({ "key": "k" })),
            Method::StorageSet {
                key: "k".into(),
                value: None,
                shared: None
            }
        );
        assert_eq!(
            parse(
                "subscribe",
                json!({ "sub": "s1", "topic": "system", "intervalMs": 10 })
            ),
            Method::Subscribe {
                sub: "s1".into(),
                topic: Topic::System {
                    interval_ms: MIN_INTERVAL_MS
                }
            }
        );
        assert_eq!(
            parse(
                "subscribe",
                json!({ "sub": "s2", "topic": "plugin", "plugin": "weather", "event": "updated" })
            ),
            Method::Subscribe {
                sub: "s2".into(),
                topic: Topic::Plugin {
                    plugin: "weather".into(),
                    event: "updated".into()
                }
            }
        );
        assert_eq!(
            parse("unsubscribe", json!({ "sub": "s1" })),
            Method::Unsubscribe { sub: "s1".into() }
        );
        assert_eq!(
            parse(
                "plugin",
                json!({ "plugin": "weather", "method": "current" })
            ),
            Method::Plugin {
                plugin: "weather".into(),
                method: "current".into(),
                params: None
            }
        );
        assert_eq!(
            parse("media.control", json!({ "action": "next" })),
            Method::MediaControl {
                action: MediaAction::Next
            }
        );
        assert_eq!(
            code(Method::parse("storage.get", json!({ "key": "" }))),
            ErrorCode::InvalidParams
        );
        assert_eq!(
            code(Method::parse(
                "subscribe",
                json!({ "sub": "s", "topic": "disk" })
            )),
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
        // The shell answers the handshake; it never reaches the host.
        assert_eq!(
            code(Method::parse("hello", json!({ "sdk": 1 }))),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn undeclared_capabilities_are_denied() {
        let bare = manifest("[]", "[]");
        for method in [
            parse("system.stats", Value::Null),
            parse(
                "subscribe",
                json!({ "sub": "s", "topic": "system", "intervalMs": 1000 }),
            ),
            parse("media.current", Value::Null),
            parse("subscribe", json!({ "sub": "s", "topic": "media" })),
            parse("launch", json!({ "target": "https://example.com" })),
            parse("theme.current", Value::Null),
            parse("subscribe", json!({ "sub": "s", "topic": "theme" })),
            parse(
                "plugin",
                json!({ "plugin": "weather", "method": "current" }),
            ),
            parse(
                "subscribe",
                json!({ "sub": "s", "topic": "plugin", "plugin": "weather", "event": "updated" }),
            ),
        ] {
            assert_eq!(
                code(method.check(&bare)),
                ErrorCode::PermissionDenied,
                "{method:?}"
            );
        }
        // Always allowed: the widget's own state, and letting go.
        for method in [
            parse("storage.get", json!({ "key": "k" })),
            parse("storage.set", json!({ "key": "k", "value": 1 })),
            parse("unsubscribe", json!({ "sub": "s" })),
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
        for method in [
            parse(
                "subscribe",
                json!({ "sub": "s", "topic": "system", "intervalMs": 2000 }),
            ),
            parse("media.control", json!({ "action": "toggle" })),
            parse(
                "subscribe",
                json!({ "sub": "s", "topic": "plugin", "plugin": "weather", "event": "updated" }),
            ),
        ] {
            assert!(method.check(&full).is_ok(), "{method:?}");
        }
        let other_plugin = parse("plugin", json!({ "plugin": "stocks", "method": "quote" }));
        assert_eq!(code(other_plugin.check(&full)), ErrorCode::PermissionDenied);
    }

    #[test]
    fn new_methods_parse_their_params() {
        assert_eq!(parse("user.info", Value::Null), Method::UserInfo);
        // pickFile: no params at all, or any subset.
        let default = Method::PickFile {
            extensions: vec![],
            title: None,
        };
        assert_eq!(parse("pickFile", Value::Null), default);
        assert_eq!(parse("pickFile", json!({})), default);
        assert_eq!(
            parse(
                "pickFile",
                json!({ "extensions": ["jpg", "png"], "title": "Pick" })
            ),
            Method::PickFile {
                extensions: vec!["jpg".into(), "png".into()],
                title: Some("Pick".into())
            }
        );
        for bad in [
            json!({ "extensions": [".png"] }),
            json!({ "extensions": ["*"] }),
            json!({ "extensions": "png" }),
            json!({ "title": "x".repeat(201) }),
        ] {
            assert_eq!(
                code(Method::parse("pickFile", bad.clone())),
                ErrorCode::InvalidParams,
                "{bad}"
            );
        }
        // appIcon: default 64, clamped to 16..=256.
        let icon = |params: Value| match parse("appIcon", params) {
            Method::AppIcon { target, size } => (target, size),
            other => panic!("{other:?}"),
        };
        assert_eq!(icon(json!({ "target": "a.exe" })), ("a.exe".into(), 64));
        assert_eq!(icon(json!({ "target": "a.exe", "size": 8 })).1, 16);
        assert_eq!(icon(json!({ "target": "a.exe", "size": 1000 })).1, 256);
        assert_eq!(icon(json!({ "target": "a.exe", "size": 48 })).1, 48);
        for bad in [
            json!({ "target": " " }),
            json!({}),
            json!({ "target": "a\0b" }),
        ] {
            assert_eq!(
                code(Method::parse("appIcon", bad)),
                ErrorCode::InvalidParams
            );
        }
        // Shared storage needs a valid namespace.
        assert_eq!(
            parse("storage.get", json!({ "key": "k", "shared": "pins" })),
            Method::StorageGet {
                key: "k".into(),
                shared: Some("pins".into())
            }
        );
        assert_eq!(
            code(Method::parse(
                "storage.get",
                json!({ "key": "k", "shared": "../x" })
            )),
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn audio_subscriptions_clamp_their_bins() {
        let bins = |params: Value| match parse("subscribe", params) {
            Method::Subscribe {
                topic: Topic::Audio { bins },
                ..
            } => bins,
            other => panic!("{other:?}"),
        };
        assert_eq!(bins(json!({ "sub": "a", "topic": "audio" })), 64);
        assert_eq!(
            bins(json!({ "sub": "a", "topic": "audio", "bins": 100 })),
            100
        );
        assert_eq!(
            bins(json!({ "sub": "a", "topic": "audio", "bins": 10 })),
            64
        );
        assert_eq!(
            bins(json!({ "sub": "a", "topic": "audio", "bins": 5000 })),
            128
        );
        assert_eq!(
            bins(json!({ "sub": "a", "topic": "audio", "bins": -3 })),
            64
        );
        assert_eq!(
            code(Method::parse(
                "subscribe",
                json!({ "sub": "a", "topic": "audio", "bins": "x" })
            )),
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn new_capabilities_need_their_permissions() {
        let bare = manifest("[]", "[]");
        let methods = [
            parse("user.info", Value::Null),
            parse("appIcon", json!({ "target": "a.exe" })),
            parse("subscribe", json!({ "sub": "s", "topic": "audio" })),
            parse("storage.get", json!({ "key": "k", "shared": "pins" })),
            parse(
                "storage.set",
                json!({ "key": "k", "value": 1, "shared": "pins" }),
            ),
        ];
        for method in &methods {
            assert_eq!(
                code(method.check(&bare)),
                ErrorCode::PermissionDenied,
                "{method:?}"
            );
        }
        // pickFile needs none: the dialog is the consent.
        assert!(parse("pickFile", Value::Null).check(&bare).is_ok());

        let full = manifest(
            r#"["user", "launch", "audio", "storage.shared:pins"]"#,
            "[]",
        );
        for method in &methods {
            assert!(method.check(&full).is_ok(), "{method:?}");
        }
        // Only the namespace it declared.
        let other = manifest(r#"["storage.shared:other"]"#, "[]");
        assert_eq!(code(methods[3].check(&other)), ErrorCode::PermissionDenied);
        // Each permission opens only its own door.
        let audio_only = manifest(r#"["audio"]"#, "[]");
        assert_eq!(
            code(methods[0].check(&audio_only)),
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            code(methods[1].check(&audio_only)),
            ErrorCode::PermissionDenied
        );
    }

    #[test]
    fn pick_file_messages_parse() {
        let message: FromShell = serde_json::from_value(
            json!({ "type": "pickFile", "instance": "viz-1", "key": "photo", "extensions": ["png"] }),
        )
        .unwrap();
        assert!(
            matches!(message, FromShell::PickFile { ref instance, ref key, .. } if instance == "viz-1" && key == "photo")
        );
        let bare: FromShell =
            serde_json::from_value(json!({ "type": "pickFile", "instance": "a", "key": "k" }))
                .unwrap();
        assert!(matches!(
            bare,
            FromShell::PickFile {
                extensions: None,
                ..
            }
        ));
    }

    #[test]
    fn error_codes_by_name() {
        assert_eq!(ErrorCode::from_name("NotFound"), Some(ErrorCode::NotFound));
        assert_eq!(ErrorCode::from_name("notfound"), None);
    }
}
