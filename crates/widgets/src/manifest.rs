//! `widget.json` and `plugin.json`: parsing, validation, permissions and
//! settings (docs/widgets.md).

use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::net::Origin;
use crate::pick;

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
    /// The audio spectrum of the default output device.
    Audio,
    /// The signed-in user's name.
    User,
    /// `fetch` to one origin.
    Network(Origin),
    /// Read and write one shared storage namespace (`storage.shared:<namespace>`).
    StorageShared(String),
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
            "audio" => Self::Audio,
            "user" => Self::User,
            other => match other.strip_prefix("network:").map(Origin::parse) {
                Some(Some(origin)) => Self::Network(origin),
                Some(None) => {
                    return Err(format!("{other:?}: expected network:scheme://host[:port]"));
                }
                None => match other.strip_prefix("storage.shared:") {
                    Some(namespace) if valid_id(namespace) => {
                        Self::StorageShared(namespace.to_owned())
                    }
                    Some(_) => {
                        return Err(format!("{other:?}: expected storage.shared:<[a-z0-9-]+>"));
                    }
                    None => return Err(format!("unknown permission {other:?}")),
                },
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
            Permission::Audio => "audio".into(),
            Permission::User => "user".into(),
            Permission::Network(origin) => format!("network:{origin}"),
            Permission::StorageShared(namespace) => format!("storage.shared:{namespace}"),
        }
    }
}

/// Where a widget lives (docs/widgets.md, "Placement presets").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// On the desktop layer, below app windows, on the grid.
    #[default]
    Desktop,
    /// A small always-on-top widget in the topmost layer.
    Overlay,
    /// Opened from a bar module; the bars host places it, not this one.
    Popup,
}

/// How many instances of a widget the user may add.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceRule {
    /// One in total, on any monitor.
    Single,
    #[default]
    Multiple,
    /// At most one per monitor.
    PerMonitor,
}

impl InstanceRule {
    /// Whether one more instance may be added, given how many exist in total and on the
    /// monitor it would go to.
    pub fn allows(self, total: usize, on_monitor: usize) -> bool {
        match self {
            Self::Single => total == 0,
            Self::Multiple => true,
            Self::PerMonitor => on_monitor == 0,
        }
    }
}

/// A `select` option: a plain value, or an object with a label and an icon glyph. The wire
/// shape is the one the manifest was written in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectOption {
    Plain(String),
    Detailed(SelectChoice),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectChoice {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A short glyph: an emoji or a symbol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl SelectOption {
    pub fn value(&self) -> &str {
        match self {
            Self::Plain(value) => value,
            Self::Detailed(choice) => &choice.value,
        }
    }
}

/// What the `image` setting offers in the file dialog unless the manifest says otherwise.
const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif", "svg"];
/// The longest `group` heading.
const MAX_GROUP: usize = 64;
/// The longest `font` value.
const MAX_FONT: usize = 200;

fn is_false(flag: &bool) -> bool {
    !*flag
}

/// One entry of a manifest's `settings`, rendered as a form by the shell. Every entry may carry
/// a `group` (a section heading in the form).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Setting {
    Boolean {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        default: bool,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        default: f64,
    },
    /// A number shown as a slider; `min` and `max` are required.
    Slider {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        min: f64,
        max: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        /// A suffix for the readout, such as `"px"`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        default: f64,
    },
    String {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        default: String,
    },
    Select {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        options: Vec<SelectOption>,
        default: String,
    },
    /// `#rrggbb`, or `#rrggbbaa` as well when `alpha` is set.
    Color {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        alpha: bool,
        default: String,
    },
    /// `""`, or the served URL of a file the host copied into the widget's data folder.
    File {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        /// Allowed extensions, without dots.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extensions: Option<Vec<String>>,
        default: String,
    },
    /// A `file` that shows a thumbnail; the extensions default to the common image formats.
    Image {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extensions: Option<Vec<String>>,
        default: String,
    },
    /// A CSS font-family name.
    Font {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        default: String,
    },
    /// An editable list of records, one value per field.
    List {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        fields: Vec<ListField>,
        #[serde(rename = "maxItems", default, skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
        default: Vec<Map<String, Value>>,
    },
    /// A place: `null` for "where this PC is" (automatic), or `{ name, lat?, lon? }`. The form
    /// offers search-as-you-type suggestions from `search`, a method of one of this widget's
    /// plugins. A plain string (an older city setting) reads as `{ name }`.
    Place {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search: Option<PlaceSearch>,
        default: Option<Place>,
    },
}

/// Where a `place` setting's suggestions come from: `plugin`'s `method`, called with
/// `{ query }`, answering a list of `{ name, admin1?, admin2?, country?, country_code?, lat,
/// lon, population? }` (docs/widgets.md, "Places").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceSearch {
    pub plugin: String,
    pub method: String,
}

/// A `place` value other than automatic: a name, its coordinates when known (without them the
/// widget looks the name up), and where it is, for showing (`region` is the state or province).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl Place {
    /// The longest place name kept.
    const MAX_NAME: usize = 200;

    /// A saved value as a place: `None` for anything that isn't one. Coordinates come in pairs
    /// and in range, or not at all; unknown fields go.
    fn read(value: &Value) -> Option<Self> {
        let name = value.get("name")?.as_str()?.trim();
        if name.is_empty() || name.chars().count() > Self::MAX_NAME {
            return None;
        }
        let coord = |key: &str, limit: f64| -> Option<Option<f64>> {
            match value.get(key) {
                None | Some(Value::Null) => Some(None),
                Some(v) => v.as_f64().filter(|n| n.abs() <= limit).map(Some),
            }
        };
        let (lat, lon) = (coord("lat", 90.0)?, coord("lon", 180.0)?);
        if lat.is_some() != lon.is_some() {
            return None;
        }
        // Display text: kept when it is text of a sane length, else just left out.
        let text = |key: &str| {
            value
                .get(key)?
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= Self::MAX_NAME)
                .map(str::to_owned)
        };
        Some(Self {
            name: name.to_owned(),
            lat,
            lon,
            region: text("region"),
            country: text("country"),
        })
    }

    fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// A column of a `list` setting: a listable scalar setting (everything but `file` and `image`)
/// whose default is optional and which has no group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ListField {
    Boolean {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    Slider {
        key: String,
        label: String,
        min: f64,
        max: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    String {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Select {
        key: String,
        label: String,
        options: Vec<SelectOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Color {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "is_false")]
        alpha: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Font {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
}

impl ListField {
    /// The field as a scalar setting; a missing default becomes `false`,
    /// `0` (or `min`), `""`, the first option, or `#ffffff`.
    fn setting(&self) -> Setting {
        match self.clone() {
            Self::Boolean {
                key,
                label,
                default,
            } => Setting::Boolean {
                key,
                label,
                group: None,
                default: default.unwrap_or(false),
            },
            Self::Number {
                key,
                label,
                min,
                max,
                step,
                default,
            } => Setting::Number {
                key,
                label,
                group: None,
                min,
                max,
                step,
                default: default.or(min).unwrap_or(0.0),
            },
            Self::Slider {
                key,
                label,
                min,
                max,
                step,
                unit,
                default,
            } => Setting::Slider {
                key,
                label,
                group: None,
                min,
                max,
                step,
                unit,
                default: default.unwrap_or(min),
            },
            Self::String {
                key,
                label,
                default,
            } => Setting::String {
                key,
                label,
                group: None,
                default: default.unwrap_or_default(),
            },
            Self::Select {
                key,
                label,
                options,
                default,
            } => Setting::Select {
                key,
                label,
                group: None,
                default: default
                    .or_else(|| options.first().map(|o| o.value().to_owned()))
                    .unwrap_or_default(),
                options,
            },
            Self::Color {
                key,
                label,
                alpha,
                default,
            } => Setting::Color {
                key,
                label,
                group: None,
                alpha,
                default: default.unwrap_or_else(|| "#ffffff".into()),
            },
            Self::Font {
                key,
                label,
                default,
            } => Setting::Font {
                key,
                label,
                group: None,
                default: default.unwrap_or_default(),
            },
        }
    }
}

/// `#rrggbb` or, with `alpha`, `#rrggbbaa`.
fn is_color_with(text: &str, alpha: bool) -> bool {
    is_hex(text, 6) || (alpha && is_hex(text, 8))
}

fn is_hex(text: &str, digits: usize) -> bool {
    text.strip_prefix('#')
        .is_some_and(|hex| hex.len() == digits && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A font-family value: short, and nothing that could end a CSS declaration or open markup.
fn is_font(text: &str) -> bool {
    text.chars().count() <= MAX_FONT && !text.contains([';', '{', '}', '<', '>', '\\'])
}

/// A number in `min..=max` (either may be absent).
fn in_range(value: &Value, min: Option<f64>, max: Option<f64>) -> bool {
    value.as_f64().is_some_and(|n| {
        n.is_finite() && min.is_none_or(|min| n >= min) && max.is_none_or(|max| n <= max)
    })
}

impl Setting {
    pub fn key(&self) -> &str {
        match self {
            Self::Boolean { key, .. }
            | Self::Number { key, .. }
            | Self::Slider { key, .. }
            | Self::String { key, .. }
            | Self::Select { key, .. }
            | Self::Color { key, .. }
            | Self::File { key, .. }
            | Self::Image { key, .. }
            | Self::Font { key, .. }
            | Self::List { key, .. }
            | Self::Place { key, .. } => key,
        }
    }

    fn group(&self) -> Option<&str> {
        match self {
            Self::Boolean { group, .. }
            | Self::Number { group, .. }
            | Self::Slider { group, .. }
            | Self::String { group, .. }
            | Self::Select { group, .. }
            | Self::Color { group, .. }
            | Self::File { group, .. }
            | Self::Image { group, .. }
            | Self::Font { group, .. }
            | Self::List { group, .. }
            | Self::Place { group, .. } => group.as_deref(),
        }
    }

    /// The extensions the file dialog offers for a `file` or `image` setting: what the manifest
    /// lists, else (for images only) the common image formats. Empty means any file.
    pub fn extensions(&self) -> Vec<String> {
        let listed = |extensions: &Option<Vec<String>>| extensions.clone().unwrap_or_default();
        match self {
            Self::File { extensions, .. } => listed(extensions),
            Self::Image { extensions, .. } => extensions
                .clone()
                .unwrap_or_else(|| IMAGE_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect()),
            _ => Vec::new(),
        }
    }

    /// Whether this is a `file` or `image` setting (its value is a served URL).
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. } | Self::Image { .. })
    }

    /// The default, as stored (a list's items get their missing fields).
    pub fn default_value(&self) -> Value {
        match self {
            Self::Boolean { default, .. } => (*default).into(),
            Self::Number { default, .. } | Self::Slider { default, .. } => (*default).into(),
            Self::String { default, .. }
            | Self::Select { default, .. }
            | Self::Color { default, .. }
            | Self::File { default, .. }
            | Self::Image { default, .. }
            | Self::Font { default, .. } => default.clone().into(),
            Self::List { default, .. } => {
                let raw: Value = default.iter().cloned().map(Value::Object).collect();
                self.fit(&raw).unwrap_or(raw)
            }
            Self::Place { default, .. } => default.as_ref().map_or(Value::Null, Place::to_value),
        }
    }

    /// `value` as this setting stores it, or `None` if it isn't valid. Only
    /// lists change: missing fields get their defaults and unknown ones go.
    ///
    /// A `file` or `image` value must be `""` or *a* widget data URL; that it is this widget's
    /// own is checked by [`WidgetManifest`], which knows the id.
    pub fn fit(&self, value: &Value) -> Option<Value> {
        if let Self::Place { .. } = self {
            return match value {
                Value::Null => Some(Value::Null),
                // An older plain-text city: "" was "not set", anything else a name to look up.
                Value::String(s) if s.trim().is_empty() => Some(Value::Null),
                Value::String(s) => {
                    Place::read(&serde_json::json!({ "name": s })).map(|p| p.to_value())
                }
                _ => Place::read(value).map(|p| p.to_value()),
            };
        }
        let ok = match (self, value) {
            (Self::Boolean { .. }, Value::Bool(_)) => true,
            (Self::Number { min, max, .. }, Value::Number(_)) => in_range(value, *min, *max),
            (Self::Slider { min, max, .. }, Value::Number(_)) => {
                in_range(value, Some(*min), Some(*max))
            }
            (Self::String { .. }, Value::String(s)) => s.len() <= 4096,
            (Self::Select { options, .. }, Value::String(s)) => {
                options.iter().any(|o| o.value() == s)
            }
            (Self::Color { alpha, .. }, Value::String(s)) => is_color_with(s, *alpha),
            (Self::File { .. } | Self::Image { .. }, Value::String(s)) => {
                s.is_empty() || pick::parse_data_url(s).is_some()
            }
            (Self::Font { .. }, Value::String(s)) => is_font(s),
            (
                Self::List {
                    fields, max_items, ..
                },
                Value::Array(items),
            ) => {
                if max_items.is_some_and(|max| items.len() > max) {
                    return None;
                }
                let fields: Vec<Setting> = fields.iter().map(ListField::setting).collect();
                let fit_item = |item: &Value| -> Option<Value> {
                    let item = item.as_object()?;
                    let record = fields.iter().map(|field| {
                        let value = match item.get(field.key()) {
                            Some(value) => field.fit(value)?,
                            None => field.default_value(),
                        };
                        Some((field.key().to_owned(), value))
                    });
                    record.collect::<Option<Map<_, _>>>().map(Value::Object)
                };
                return items
                    .iter()
                    .map(fit_item)
                    .collect::<Option<Vec<_>>>()
                    .map(Value::Array);
            }
            _ => false,
        };
        ok.then(|| value.clone())
    }

    fn validate(&self) -> Result<()> {
        let key = self.key();
        ensure!(!key.is_empty(), "a setting has an empty key");
        ensure!(
            self.group().is_none_or(|g| g.chars().count() <= MAX_GROUP),
            "setting {key:?}: group is longer than {MAX_GROUP} characters"
        );
        match self {
            Self::Number { min, max, step, .. } => {
                ensure!(
                    min.zip(*max).is_none_or(|(min, max)| min <= max),
                    "setting {key:?}: min > max"
                );
                ensure!(
                    step.is_none_or(|step| step > 0.0),
                    "setting {key:?}: step must be positive"
                );
            }
            Self::Slider { min, max, step, .. } => {
                ensure!(min < max, "setting {key:?}: min must be below max");
                ensure!(
                    step.is_none_or(|step| step > 0.0),
                    "setting {key:?}: step must be positive"
                );
            }
            Self::Select { options, .. } => {
                ensure!(!options.is_empty(), "setting {key:?} has no options");
            }
            Self::Place {
                search, default, ..
            } => {
                if let Some(search) = search {
                    ensure!(
                        valid_id(&search.plugin) && !search.method.is_empty(),
                        "setting {key:?}: search needs a plugin id and a method"
                    );
                }
                if let Some(place) = default {
                    ensure!(
                        Place::read(&place.to_value()).as_ref() == Some(place),
                        "setting {key:?}: the default is not a valid place"
                    );
                }
            }
            Self::File {
                extensions,
                default,
                ..
            }
            | Self::Image {
                extensions,
                default,
                ..
            } => {
                if let Some(extensions) = extensions {
                    ensure!(
                        !extensions.is_empty(),
                        "setting {key:?}: extensions is empty"
                    );
                    if let Some(bad) = extensions.iter().find(|e| !pick::valid_extension(e)) {
                        bail!(
                            "setting {key:?}: extension {bad:?} must be letters and digits, no dot"
                        );
                    }
                }
                ensure!(
                    default.is_empty(),
                    "setting {key:?}: the default of a file or image must be \"\""
                );
            }
            Self::List {
                fields, max_items, ..
            } => {
                ensure!(!fields.is_empty(), "setting {key:?} has no fields");
                ensure!(
                    max_items.is_none_or(|max| max >= 1),
                    "setting {key:?}: maxItems must be at least 1"
                );
                let mut keys = std::collections::HashSet::new();
                for field in fields.iter().map(ListField::setting) {
                    field
                        .validate()
                        .with_context(|| format!("in setting {key:?}"))?;
                    ensure!(
                        keys.insert(field.key().to_owned()),
                        "setting {key:?}: field {:?} appears twice",
                        field.key()
                    );
                }
            }
            _ => {}
        }
        ensure!(
            self.fit(&self.default_value()).is_some(),
            "setting {key:?}: the default isn't a valid value"
        );
        Ok(())
    }
}

/// Sizes in grid cells (the fine grid of layout version 2: half a desktop-icon cell). Every
/// size from `min` to `max` is allowed, one cell at a time; views adapt to any of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeSpec {
    pub default: [u32; 2],
    #[serde(default = "one_by_one")]
    pub min: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<[u32; 2]>,
}

impl SizeSpec {
    /// Every allowed size, row by row (for size sheets and pickers); `max` defaults to
    /// `limit` when the manifest leaves it open.
    pub fn sizes(&self, limit: [u32; 2]) -> impl Iterator<Item = [u32; 2]> + use<> {
        let (min, max) = (self.min, self.max.unwrap_or(limit));
        (min[1]..=max[1]).flat_map(move |h| (min[0]..=max[0]).map(move |w| [w, h]))
    }
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
    /// For editors (the JSON Schema); ignored.
    #[serde(rename = "$schema", default, skip_serializing)]
    _schema: Option<IgnoredAny>,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The widget picker's group ("Time", "Media", ...); none lands in "Other".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    pub size: SizeSpec,
    #[serde(default = "yes")]
    pub resizable: bool,
    #[serde(default)]
    pub preset: Preset,
    #[serde(default)]
    pub instances: InstanceRule,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub settings: Vec<Setting>,
    /// Widget ids this one took over, each with the settings an instance of the old widget gets
    /// (docs/widgets.md, "Replaced widgets"). Host-only: not sent to the shell.
    #[serde(default, skip_serializing)]
    pub replaces: std::collections::BTreeMap<String, Map<String, Value>>,
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
        for setting in &self.settings {
            if let Setting::Place {
                key,
                search: Some(search),
                ..
            } = setting
            {
                ensure!(
                    self.plugins.contains(&search.plugin),
                    "setting {key:?} searches with {:?}, which isn't in \"plugins\"",
                    search.plugin
                );
            }
        }
        for (old, settings) in &self.replaces {
            ensure!(
                valid_id(old) && *old != self.id,
                "replaces: {old:?} must be another widget's id"
            );
            self.check_settings(settings.clone())
                .with_context(|| format!("in replaces.{old}"))?;
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
        self.fit_size(size.map(f64::from), [u32::MAX; 2])
            .expect("validated manifest has a default size")
    }

    /// The closest allowed size (every size from min to max, rounded per axis) that fits in
    /// `limits` cells; None when even `min` doesn't fit.
    pub fn fit_size(&self, requested: [f64; 2], limits: [u32; 2]) -> Option<[u32; 2]> {
        if !self.resizable {
            return (0..2)
                .all(|i| self.size.default[i] <= limits[i])
                .then_some(self.size.default);
        }
        let max = self.size.max.unwrap_or([u32::MAX; 2]);
        let hi = [0, 1].map(|i| max[i].min(limits[i]));
        (0..2).all(|i| self.size.min[i] <= hi[i]).then(|| {
            [0, 1].map(|i| (requested[i].round().max(0.0) as u32).clamp(self.size.min[i], hi[i]))
        })
    }

    pub fn setting(&self, key: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.key() == key)
    }

    /// [`Setting::fit`], plus: a `file` or `image` value must be one of *this* widget's data
    /// URLs (or `""`), never another widget's.
    fn fit(&self, setting: &Setting, value: &Value) -> Option<Value> {
        let fitted = setting.fit(value)?;
        let own = !setting.is_file()
            || fitted
                .as_str()
                .and_then(pick::parse_data_url)
                .is_none_or(|url| url.widget == self.id);
        own.then_some(fitted)
    }

    /// Every declared setting with its value: the saved one if it's valid,
    /// else the default. Unknown saved keys are dropped.
    pub fn resolve_settings(&self, saved: &Map<String, Value>) -> Map<String, Value> {
        self.settings
            .iter()
            .map(|setting| {
                let value = saved
                    .get(setting.key())
                    .and_then(|v| self.fit(setting, v))
                    .unwrap_or_else(|| setting.default_value());
                (setting.key().to_owned(), value)
            })
            .collect()
    }

    /// Checks a settings edit from the shell (every key declared, every value
    /// valid) and returns the values as they're stored.
    pub fn check_settings(&self, values: Map<String, Value>) -> Result<Map<String, Value>> {
        values
            .into_iter()
            .map(|(key, value)| {
                let Some(setting) = self.setting(&key) else {
                    bail!("{:?} has no setting {key:?}", self.id);
                };
                let fitted = self
                    .fit(setting, &value)
                    .with_context(|| format!("{value} isn't a valid value for {key:?}"))?;
                Ok((key, fitted))
            })
            .collect()
    }
}

/// Names the host uses itself, so plugins can't offer them as methods.
const RESERVED_METHODS: [&str; 5] = ["initialize", "shutdown", "suspend", "resume", "event"];

/// A plugin's `plugin.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// For editors (the JSON Schema); ignored.
    #[serde(rename = "$schema", default)]
    _schema: Option<IgnoredAny>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// The executable: in the plugin folder, else on `PATH`. Empty for component-only plugins.
    #[serde(default)]
    pub command: String,
    /// The WebAssembly component (docs/plugins.md); its faces run in `mypaper-plugins`.
    #[serde(default)]
    pub component: Option<String>,
    /// The WIT package `"<major>.<minor>"` a component was built against.
    #[serde(default)]
    pub api: Option<String>,
    /// Surfaces a component may restyle, desktop-menu items, commands and its own limits; the
    /// runtime checks these (`crates/plugins`).
    #[serde(default)]
    pub styles: Vec<String>,
    #[serde(default)]
    pub menu: Vec<Value>,
    #[serde(default)]
    pub commands: Vec<Value>,
    #[serde(default)]
    pub limits: Option<Value>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    /// Shown before install; the host enforces what it brokers (docs/plugins.md, "Permissions").
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The SDK `"<major>.<minor>"` it was built with; absent for protocol-1 data plugins.
    #[serde(default)]
    pub sdk: Option<String>,
    /// Widgets it draws with declarative views (SDK plugins only).
    #[serde(default)]
    pub faces: Vec<mypaper_plugin_sdk::FaceSpec>,
    #[serde(default)]
    pub panels: Vec<mypaper_plugin_sdk::FaceSpec>,
    #[serde(default)]
    pub bars: Vec<mypaper_plugin_sdk::FaceSpec>,
    #[serde(default)]
    pub settings: Vec<Value>,
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
            !manifest.command.is_empty() || manifest.component.is_some(),
            "a plugin needs a \"command\" or a \"component\""
        );
        ensure!(
            manifest.command.is_empty() || inside(&manifest.command),
            "command {:?} must be a path inside the plugin folder or a program on PATH",
            manifest.command
        );
        if let Some(component) = &manifest.component {
            ensure!(
                component.ends_with(".wasm") && !component.contains(['/', '\\']),
                "component {component:?} must be a .wasm file in the plugin folder"
            );
        }
        if let Some(reserved) = manifest
            .methods
            .iter()
            .find(|m| RESERVED_METHODS.contains(&m.as_str()))
        {
            bail!("{reserved:?} is reserved and can't be a plugin method");
        }
        if let Some(sdk) = &manifest.sdk {
            ensure!(
                mypaper_plugin_sdk::compatible(sdk, mypaper_plugin_sdk::SDK_MAJOR),
                "built for plugin SDK {sdk}; this mypaper needs SDK {}.x",
                mypaper_plugin_sdk::SDK_MAJOR
            );
        }
        ensure!(
            manifest.sdk.is_some() || manifest.component.is_some() || manifest.faces.is_empty(),
            "faces need an \"sdk\" version or a \"component\""
        );
        for face in &manifest.faces {
            ensure!(valid_id(&face.id), "face id {:?} must be lowercase [a-z0-9-]", face.id);
        }
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
        assert_eq!(clock.preset, Preset::Desktop);
        assert_eq!(clock.instances, InstanceRule::Multiple);
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
    fn a_place_is_automatic_or_a_name_with_coordinates() {
        let manifest = |plugins: &str, default: &str| {
            format!(
                r#"{{ "id": "weather", "name": "Weather", "size": {{ "default": [2, 1] }}, "plugins": {plugins},
                     "settings": [{{ "key": "city", "type": "place", "label": "Location",
                                    "search": {{ "plugin": "weather", "method": "search" }}, "default": {default} }}] }}"#
            )
        };
        let weather =
            WidgetManifest::parse(&manifest(r#"["weather"]"#, "null"), "weather").unwrap();
        let resolve = |saved: Value| {
            weather.resolve_settings(&Map::from_iter([("city".to_owned(), saved)]))["city"].clone()
        };
        assert_eq!(resolve(Value::Null), Value::Null, "automatic");
        assert_eq!(
            resolve(json!("")),
            Value::Null,
            "an empty old city is automatic"
        );
        assert_eq!(
            resolve(json!(" Oslo ")),
            json!({ "name": "Oslo" }),
            "an old city is a name to look up"
        );
        assert_eq!(
            resolve(
                json!({ "name": "Bergen", "lat": 60.39, "lon": 5.32, "region": "Vestland", "country": "Norway", "population": 1 })
            ),
            json!({ "name": "Bergen", "lat": 60.39, "lon": 5.32, "region": "Vestland", "country": "Norway" }),
            "display fields stay, unknown fields go"
        );
        for bad in [
            json!({ "name": "X", "lat": 91.0, "lon": 0.0 }),
            json!({ "name": "X", "lat": 1.0 }),
            json!({ "lat": 1.0, "lon": 2.0 }),
            json!(3),
        ] {
            assert_eq!(
                resolve(bad.clone()),
                Value::Null,
                "{bad} falls back to the default"
            );
        }
        assert!(
            WidgetManifest::parse(&manifest("[]", "null"), "weather").is_err(),
            "the search plugin must be one the widget may call"
        );
        assert!(
            WidgetManifest::parse(
                &manifest(r#"["weather"]"#, r#"{ "name": "Oslo", "lat": 99 }"#),
                "weather"
            )
            .is_err()
        );
    }

    #[test]
    fn replaces_needs_other_ids_and_valid_settings() {
        let with =
            |replaces: &str| CLOCK.replacen('{', &format!("{{ \"replaces\": {replaces},"), 1);
        let ok = WidgetManifest::parse(
            &with(r#"{ "analog-clock": { "density": "compact" } }"#),
            "clock",
        )
        .unwrap();
        assert_eq!(ok.replaces["analog-clock"]["density"], "compact");
        // Not sent to the shell: it is the host's business.
        assert!(serde_json::to_value(&ok).unwrap().get("replaces").is_none());
        for (bad, why) in [
            (r#"{ "clock": {} }"#, "itself"),
            (r#"{ "Analog": {} }"#, "invalid id"),
            (r#"{ "analog-clock": { "nope": 1 } }"#, "undeclared setting"),
            (
                r#"{ "analog-clock": { "density": "roomy" } }"#,
                "invalid value",
            ),
        ] {
            assert!(WidgetManifest::parse(&with(bad), "clock").is_err(), "{why}");
        }
    }

    #[test]
    fn permissions_round_trip_as_strings() {
        let json = serde_json::to_value(clock().permissions).unwrap();
        assert_eq!(
            json,
            json!(["system.stats", "network:https://api.open-meteo.com"])
        );
        let new: Vec<Permission> =
            serde_json::from_value(json!(["audio", "user", "storage.shared:pins"])).unwrap();
        assert_eq!(
            new,
            [
                Permission::Audio,
                Permission::User,
                Permission::StorageShared("pins".into())
            ]
        );
        assert_eq!(
            serde_json::to_value(&new).unwrap(),
            json!(["audio", "user", "storage.shared:pins"])
        );
        for bad in [
            "storage.shared:",
            "storage.shared:Pins",
            "storage.shared:a/b",
            "storage.shared",
        ] {
            assert!(
                serde_json::from_value::<Permission>(json!(bad)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn presets_and_instance_rules_parse() {
        let with = |extra: &str| {
            let json = CLOCK.replacen("\"version\"", &format!("{extra}, \"version\""), 1);
            WidgetManifest::parse(&json, "clock")
        };
        let m = with("\"preset\": \"overlay\", \"instances\": \"per-monitor\"").unwrap();
        assert_eq!(
            (m.preset, m.instances),
            (Preset::Overlay, InstanceRule::PerMonitor)
        );
        let m = with("\"preset\": \"popup\", \"instances\": \"single\"").unwrap();
        assert_eq!(
            (m.preset, m.instances),
            (Preset::Popup, InstanceRule::Single)
        );
        for bad in [
            "\"preset\": \"floating\"",
            "\"preset\": \"Overlay\"",
            "\"instances\": \"many\"",
            "\"instances\": \"perMonitor\"",
        ] {
            assert!(with(bad).is_err(), "{bad}");
        }
        // The shell gets both, spelled as in the manifest.
        let json = serde_json::to_value(
            with("\"preset\": \"overlay\", \"instances\": \"per-monitor\"").unwrap(),
        )
        .unwrap();
        assert_eq!(
            (&json["preset"], &json["instances"]),
            (&json!("overlay"), &json!("per-monitor"))
        );
    }

    #[test]
    fn the_instance_rule_limits_adding() {
        use InstanceRule::*;
        assert!(Multiple.allows(9, 9));
        assert!(Single.allows(0, 0) && !Single.allows(1, 0) && !Single.allows(1, 1));
        assert!(PerMonitor.allows(3, 0) && !PerMonitor.allows(3, 1));
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
        let check = |values: Value| clock().check_settings(values.as_object().unwrap().clone());
        assert!(check(json!({ "opacity": 0.5, "accent": "#00ff00" })).is_ok());
        for bad in [
            json!({ "opacity": 2 }),
            json!({ "nope": 1 }),
            json!({ "format24": "yes" }),
        ] {
            assert!(check(bad.clone()).is_err(), "{bad}");
        }
    }

    const LAUNCHER: &str = r#"{
        "$schema": "../../packages/widget-sdk/schema/widget.schema.json",
        "id": "launcher", "name": "Launcher", "size": { "default": [2, 1] },
        "settings": [{
            "key": "shortcuts", "type": "list", "label": "Shortcuts", "maxItems": 2,
            "fields": [
                { "key": "label", "type": "string", "label": "Label" },
                { "key": "pinned", "type": "boolean", "label": "Pinned", "default": true }
            ],
            "default": [{ "label": "Files" }]
        }]
    }"#;

    #[test]
    fn list_settings_fill_fields_and_respect_their_limits() {
        let launcher = WidgetManifest::parse(LAUNCHER, "launcher").unwrap();
        let resolved = launcher.resolve_settings(&Map::new());
        assert_eq!(
            resolved["shortcuts"],
            json!([{ "label": "Files", "pinned": true }])
        );

        let edit = |value: Value| {
            let values = json!({ "shortcuts": value });
            launcher.check_settings(values.as_object().unwrap().clone())
        };
        let stored = edit(json!([{ "label": "A", "extra": 1 }, { "pinned": false }])).unwrap();
        assert_eq!(
            stored["shortcuts"],
            json!([{ "label": "A", "pinned": true }, { "label": "", "pinned": false }])
        );
        assert!(edit(json!([{}, {}, {}])).is_err(), "over maxItems");
        assert!(edit(json!([{ "pinned": "yes" }])).is_err(), "a bad field");
        assert!(edit(json!({ "label": "A" })).is_err(), "not a list");

        let saved = json!({ "shortcuts": [1, 2] });
        let fallback = launcher.resolve_settings(saved.as_object().unwrap());
        assert_eq!(fallback["shortcuts"], resolved["shortcuts"]);
        let nested = LAUNCHER.replace("\"type\": \"boolean\"", "\"type\": \"list\"");
        assert!(
            WidgetManifest::parse(&nested, "launcher").is_err(),
            "fields are scalar"
        );
    }

    /// A manifest whose settings are `settings` (a JSON array body).
    fn with_settings(settings: &str) -> Result<WidgetManifest> {
        let json = format!(
            r#"{{ "id": "viz", "name": "Viz", "size": {{ "default": [2, 1] }}, "settings": [{settings}] }}"#
        );
        WidgetManifest::parse(&json, "viz")
    }

    #[test]
    fn every_new_setting_type_is_accepted() {
        let manifest = with_settings(
            r##"
            { "key": "gain", "type": "slider", "label": "Gain", "min": 0, "max": 10, "step": 0.5, "unit": "px", "default": 2, "group": "Audio" },
            { "key": "glow", "type": "color", "label": "Glow", "alpha": true, "default": "#ff008080" },
            { "key": "tint", "type": "color", "label": "Tint", "alpha": true, "default": "#ff0080" },
            { "key": "photo", "type": "image", "label": "Photo", "default": "" },
            { "key": "sound", "type": "file", "label": "Sound", "extensions": ["wav", "mp3"], "default": "" },
            { "key": "face", "type": "font", "label": "Font", "default": "Cascadia Code" },
            { "key": "mode", "type": "select", "label": "Mode", "default": "bars",
              "options": ["bars", { "value": "wave", "label": "Wave", "icon": "~" }, { "value": "dots", "icon": "." }] }
        "##,
        )
        .unwrap();
        let resolved = manifest.resolve_settings(&Map::new());
        assert_eq!(
            Value::Object(resolved),
            json!({ "gain": 2.0, "glow": "#ff008080", "tint": "#ff0080", "photo": "", "sound": "",
                    "face": "Cascadia Code", "mode": "bars" })
        );
        assert_eq!(
            manifest.setting("photo").unwrap().extensions().len(),
            IMAGE_EXTENSIONS.len()
        );
        assert_eq!(
            manifest.setting("sound").unwrap().extensions(),
            ["wav", "mp3"]
        );
        assert!(manifest.setting("gain").unwrap().extensions().is_empty());
    }

    #[test]
    fn invalid_new_settings_are_rejected() {
        for (json, why) in [
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "min": 0, "default": 0 }"#,
                "slider needs max",
            ),
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "max": 1, "default": 0 }"#,
                "slider needs min",
            ),
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "min": 1, "max": 1, "default": 1 }"#,
                "min = max",
            ),
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "min": 0, "max": 1, "step": 0, "default": 0 }"#,
                "step 0",
            ),
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "min": 0, "max": 1, "default": 2 }"#,
                "default out of range",
            ),
            (
                r##"{ "key": "a", "type": "color", "label": "A", "default": "#ff008080" }"##,
                "8 hex without alpha",
            ),
            (
                r##"{ "key": "a", "type": "color", "label": "A", "alpha": true, "default": "#ff00" }"##,
                "4 hex",
            ),
            (
                r#"{ "key": "a", "type": "image", "label": "A", "default": "x.png" }"#,
                "image default not empty",
            ),
            (
                r#"{ "key": "a", "type": "file", "label": "A", "extensions": [".png"], "default": "" }"#,
                "dotted extension",
            ),
            (
                r#"{ "key": "a", "type": "file", "label": "A", "extensions": ["p/g"], "default": "" }"#,
                "bad extension",
            ),
            (
                r#"{ "key": "a", "type": "file", "label": "A", "extensions": [], "default": "" }"#,
                "empty extensions",
            ),
            (
                r#"{ "key": "a", "type": "font", "label": "A", "default": "A; color: red" }"#,
                "font with ;",
            ),
            (
                r#"{ "key": "a", "type": "font", "label": "A", "default": "A{}" }"#,
                "font with braces",
            ),
            (
                r#"{ "key": "a", "type": "boolean", "label": "A", "default": true, "group": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" }"#,
                "group over 64",
            ),
            (
                r#"{ "key": "a", "type": "select", "label": "A", "default": "x", "options": [{ "value": "x", "color": "red" }] }"#,
                "unknown option field",
            ),
            (
                r#"{ "key": "a", "type": "select", "label": "A", "default": "y", "options": [{ "value": "x" }] }"#,
                "default not in object options",
            ),
            (
                r#"{ "key": "a", "type": "select", "label": "A", "default": "x", "options": [{ "label": "X" }] }"#,
                "option without value",
            ),
            (
                r#"{ "key": "a", "type": "slider", "label": "A", "min": 0, "max": 1, "default": 0, "unit": "px", "extra": 1 }"#,
                "unknown field",
            ),
        ] {
            assert!(with_settings(json).is_err(), "{why}");
        }
        let too_long_font = format!(
            r#"{{ "key": "a", "type": "font", "label": "A", "default": "{}" }}"#,
            "x".repeat(201)
        );
        assert!(with_settings(&too_long_font).is_err(), "font over 200");
        let exactly = too_long_font.replace(&"x".repeat(201), &"x".repeat(200));
        assert!(with_settings(&exactly).is_ok());
    }

    #[test]
    fn values_are_fitted_per_type() {
        let manifest = with_settings(
            r##"
            { "key": "gain", "type": "slider", "label": "Gain", "min": 0, "max": 10, "default": 2 },
            { "key": "glow", "type": "color", "label": "Glow", "alpha": true, "default": "#ff008080" },
            { "key": "flat", "type": "color", "label": "Flat", "default": "#ff0080" },
            { "key": "face", "type": "font", "label": "Font", "default": "" },
            { "key": "mode", "type": "select", "label": "Mode", "default": "a",
              "options": [{ "value": "a" }, { "value": "b", "icon": "b" }] }
        "##,
        )
        .unwrap();
        let fit = |key: &str, value: Value| manifest.setting(key).unwrap().fit(&value);
        assert_eq!(fit("gain", json!(10)), Some(json!(10)));
        assert_eq!(fit("gain", json!(10.5)), None, "above max");
        assert_eq!(fit("gain", json!(-1)), None, "below min");
        assert_eq!(fit("gain", json!("5")), None, "not a number");
        assert_eq!(fit("glow", json!("#ff008080")), Some(json!("#ff008080")));
        assert_eq!(
            fit("glow", json!("#ff0080")),
            Some(json!("#ff0080")),
            "plain also fine"
        );
        assert_eq!(fit("glow", json!("#ff00808")), None, "7 digits");
        assert_eq!(fit("flat", json!("#ff008080")), None, "alpha not enabled");
        assert_eq!(fit("face", json!("Segoe UI")), Some(json!("Segoe UI")));
        assert_eq!(fit("face", json!("a;b")), None);
        assert_eq!(fit("face", json!("a\\b")), None);
        assert_eq!(fit("face", json!("<b>")), None);
        assert_eq!(fit("mode", json!("b")), Some(json!("b")));
        assert_eq!(fit("mode", json!("c")), None);
    }

    #[test]
    fn file_values_are_this_widgets_data_urls_or_empty() {
        let manifest = with_settings(
            r#"{ "key": "photo", "type": "image", "label": "Photo", "default": "" }"#,
        )
        .unwrap();
        let own = pick::data_url("viz", "viz-1", "My Photo.png");
        let other = pick::data_url("clock", "clock-1", "a.png");
        let saved = |value: &str| {
            let saved = json!({ "photo": value });
            manifest.resolve_settings(saved.as_object().unwrap())["photo"].clone()
        };
        assert_eq!(saved(&own), json!(own));
        assert_eq!(saved(""), json!(""));
        for bad in [other.as_str(), "https://evil.com/a.png", "C:\\a.png", "x"] {
            assert_eq!(saved(bad), json!(""), "{bad}");
        }
        let edit = |value: &str| {
            let values = json!({ "photo": value });
            manifest.check_settings(values.as_object().unwrap().clone())
        };
        assert!(edit(&own).is_ok() && edit("").is_ok());
        assert!(edit(&other).is_err() && edit("https://evil.com/a.png").is_err());
    }

    #[test]
    fn select_options_keep_their_wire_shape() {
        let manifest = with_settings(
            r#"{ "key": "m", "type": "select", "label": "M", "default": "a",
                 "options": ["a", { "value": "b", "label": "Bee" }, { "value": "c", "icon": "c" }] }"#,
        )
        .unwrap();
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            json["settings"][0]["options"],
            json!(["a", { "value": "b", "label": "Bee" }, { "value": "c", "icon": "c" }])
        );
        let again = WidgetManifest::parse(&json.to_string(), "viz").unwrap();
        assert_eq!(again, manifest, "serde round trip");
    }

    #[test]
    fn new_settings_round_trip_through_json() {
        let manifest = with_settings(
            r##"
            { "key": "gain", "type": "slider", "label": "Gain", "min": 0, "max": 10, "step": 1, "unit": "%", "default": 2, "group": "G" },
            { "key": "glow", "type": "color", "label": "Glow", "alpha": true, "default": "#ff008080" },
            { "key": "flat", "type": "color", "label": "Flat", "default": "#ff0080" },
            { "key": "photo", "type": "image", "label": "Photo", "extensions": ["png"], "default": "" },
            { "key": "face", "type": "font", "label": "Font", "default": "x" }
        "##,
        )
        .unwrap();
        let json = serde_json::to_value(&manifest).unwrap();
        // Unset optionals and a false `alpha` are not written.
        assert_eq!(
            json["settings"][2],
            json!({ "type": "color", "key": "flat", "label": "Flat", "default": "#ff0080" })
        );
        assert_eq!(json["settings"][0]["group"], json!("G"));
        assert_eq!(
            WidgetManifest::parse(&json.to_string(), "viz").unwrap(),
            manifest
        );
    }

    #[test]
    fn lists_take_the_listable_types_but_not_files() {
        let list = |field: &str| {
            with_settings(&format!(
                r#"{{ "key": "l", "type": "list", "label": "L", "default": [],
                      "fields": [{field}] }}"#
            ))
        };
        for ok in [
            r#"{ "key": "n", "type": "slider", "label": "N", "min": 1, "max": 5 }"#,
            r##"{ "key": "c", "type": "color", "label": "C", "alpha": true }"##,
            r#"{ "key": "f", "type": "font", "label": "F" }"#,
            r#"{ "key": "s", "type": "select", "label": "S", "options": [{ "value": "a", "icon": "a" }] }"#,
        ] {
            assert!(list(ok).is_ok(), "{ok}");
        }
        for bad in [
            r#"{ "key": "p", "type": "image", "label": "P" }"#,
            r#"{ "key": "p", "type": "file", "label": "P" }"#,
            r#"{ "key": "p", "type": "string", "label": "P", "group": "G" }"#,
        ] {
            assert!(list(bad).is_err(), "{bad}");
        }
        // Missing fields get slider's min, the first object option, and so on.
        let manifest = with_settings(
            r#"{ "key": "l", "type": "list", "label": "L", "default": [{}],
                 "fields": [{ "key": "n", "type": "slider", "label": "N", "min": 1, "max": 5 },
                            { "key": "s", "type": "select", "label": "S", "options": [{ "value": "a" }, "b"] },
                            { "key": "f", "type": "font", "label": "F" }] }"#,
        )
        .unwrap();
        assert_eq!(
            manifest.resolve_settings(&Map::new())["l"],
            json!([{ "n": 1.0, "s": "a", "f": "" }])
        );
        let edit = |value: Value| {
            let values = json!({ "l": value });
            manifest.check_settings(values.as_object().unwrap().clone())
        };
        assert!(
            edit(json!([{ "n": 9 }])).is_err(),
            "slider range applies in lists"
        );
        assert!(edit(json!([{ "n": 3, "s": "b" }])).is_ok());
    }

    #[test]
    fn groups_are_presentation_only() {
        let manifest = with_settings(
            r#"{ "key": "a", "type": "boolean", "label": "A", "default": true, "group": "Look" },
               { "key": "b", "type": "string", "label": "B", "default": "" }"#,
        )
        .unwrap();
        let resolved = manifest.resolve_settings(&Map::new());
        assert_eq!(Value::Object(resolved), json!({ "a": true, "b": "" }));
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["settings"][0]["group"], json!("Look"));
        assert!(json["settings"][1].get("group").is_none());
    }

    #[test]
    fn plugin_manifests() {
        let json = r#"{ "$schema": "x", "id": "weather", "name": "Weather", "command": "weather.exe",
                        "methods": ["current"], "events": ["updated"],
                        "permissions": ["network:https://api.open-meteo.com"] }"#;
        let plugin = PluginManifest::parse(json, "weather").unwrap();
        assert_eq!(plugin.methods, ["current"]);
        assert!(PluginManifest::parse(&json.replace("weather.exe", "python"), "weather").is_ok());
        let sdk = |version: &str| {
            json.replacen("{", &format!(r#"{{ "sdk": "{version}", "faces": [{{ "id": "now", "name": "Now", "size": {{ "default": [2, 2] }} }}],"#), 1)
        };
        assert!(PluginManifest::parse(&sdk("1.3"), "weather").is_ok());
        let refused = PluginManifest::parse(&sdk("2.0"), "weather").unwrap_err();
        assert!(format!("{refused:#}").contains("SDK 2.0"), "{refused:#}");
        for bad in [
            json.replace("weather.exe", "..\\x.exe"),
            json.replace("weather.exe", "C:\\x.exe"),
            json.replace("\"current\"", "\"shutdown\""),
        ] {
            assert!(PluginManifest::parse(&bad, "weather").is_err(), "{bad}");
        }
    }

    /// The bundled examples are the contract in practice: they must all load.
    #[test]
    fn bundled_manifests_load() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let folders = |dir: &str, manifest: &str| -> Vec<std::path::PathBuf> {
            std::fs::read_dir(repo.join(dir))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join(manifest).is_file())
                .collect()
        };
        let widgets = crate::registry::builtins();
        for dir in folders("plugins", PLUGIN_MANIFEST) {
            PluginManifest::load(&dir).unwrap();
        }
        // Every manifest in the catalog folder made it into the registry.
        for entry in std::fs::read_dir(repo.join("crates/widgets/src/catalog")).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let id = json["id"].as_str().unwrap_or_default();
            assert!(widgets.widgets.contains_key(id), "{} is not registered", path.display());
        }
    }
}
