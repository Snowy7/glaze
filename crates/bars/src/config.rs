//! `%APPDATA%\mypaper\bars.json`: which bars show where, their designs, layout, modules, look and
//! motion, the dock's pins and the native taskbar's mode (docs/bars.md, "Config").
//!
//! Every field is optional and unknown fields are errors. The file is merged over its preset and
//! each bar's design (`presets.rs`), and per-monitor `overrides` are merged over that for their monitor
//! ([`BarsConfig::for_monitor`]). Writers (core's tray, Studio, the dock's pin edits) change the
//! file with [`edit`], which touches only the fields it sets, so a file keeps what its author
//! wrote and never gains the defaults.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_theme::Appearance;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

pub use crate::flyout_config::{
    AudioConfig, CalendarConfig, LauncherConfig, LauncherLayout, TrayConfig, Weekday, Zone,
};

pub use crate::presets::{DockDesign, Preset, TopDesign};
use crate::presets::{layered, merge};
use crate::style::{BorderStyle, ColorRef, Look, Motion, Paint, check};

pub const FILE: &str = "bars.json";

/// `%APPDATA%\mypaper\bars.json`.
pub fn path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(FILE))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarsConfig {
    pub version: u32,
    /// Which design each bar starts from (and which bars are on); a bar's own `design` beats it.
    pub preset: Preset,
    pub taskbar: TaskbarConfig,
    pub top: TopConfig,
    pub dock: DockConfig,
    pub launcher: LauncherConfig,
    pub tray: TrayConfig,
    pub calendar: CalendarConfig,
    pub audio: AudioConfig,
    /// The shared look: both bars', over their designs' and under their own (`top.look`,
    /// `dock.look`), which is what they use. The page only gets those.
    #[serde(skip_serializing)]
    pub look: Look,
    /// The shared motion, layered like `look`.
    #[serde(skip_serializing)]
    pub animation: Motion,
    /// Per module (`clock`, `custom:<id>`…): click actions, style, icon; a custom module's command.
    pub modules: BTreeMap<ModuleRef, ModuleOptions>,
    /// An icon pack: a folder under `%APPDATA%\mypaper\icons\`, or a bundled one (`mono`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<String>,
    /// Blur and animation off: `on`, `off`, or `auto` (on battery).
    pub performance: Performance,
    /// Values for the active theme's declared variables (`vars.rs`); the page gets them resolved.
    #[serde(skip_serializing)]
    pub vars: BTreeMap<String, Value>,
    /// The user's appearance layer; the page gets the resolved look instead.
    #[serde(skip_serializing)]
    pub appearance: Appearance,
    /// Per monitor (a device name like `DISPLAY2`, `primary` or `secondary`): a partial config
    /// merged over the rest for that monitor's bars.
    #[serde(skip_serializing)]
    pub overrides: BTreeMap<String, Value>,
    /// The user's file as JSON, for per-monitor resolution.
    #[serde(skip)]
    raw: Map<String, Value>,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            version: 1,
            preset: Preset::default(),
            taskbar: TaskbarConfig::default(),
            top: TopConfig::default(),
            dock: DockConfig::default(),
            launcher: LauncherConfig::default(),
            tray: TrayConfig::default(),
            calendar: CalendarConfig::default(),
            audio: AudioConfig::default(),
            look: Look::default(),
            animation: Motion::default(),
            modules: BTreeMap::new(),
            icons: None,
            performance: Performance::default(),
            vars: BTreeMap::new(),
            appearance: Appearance::default(),
            overrides: BTreeMap::new(),
            raw: Map::new(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Performance {
    /// On while on battery.
    #[default]
    Auto,
    On,
    Off,
}

// --- Top bar -----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TopLayout {
    /// Edge to edge.
    #[default]
    Strip,
    /// Detached from the edges by `margin`, rounded.
    Floating,
    /// A transparent rail with one pill per module group (groups split by `|`).
    Islands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TopPosition {
    #[default]
    Top,
    Bottom,
}

/// A bar's width: the whole edge, hugging its content, DIPs, or a share of the monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dimension {
    Full,
    Auto,
    Dips(f64),
    Percent(f64),
}

impl Serialize for Dimension {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Full => serializer.serialize_str("full"),
            Self::Auto => serializer.serialize_str("auto"),
            Self::Dips(dips) => serializer.serialize_f64(*dips),
            Self::Percent(percent) => serializer.serialize_str(&format!("{percent}%")),
        }
    }
}

impl<'de> Deserialize<'de> for Dimension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match Value::deserialize(deserializer)? {
            Value::String(word) if word == "full" => Ok(Self::Full),
            Value::String(word) if word == "auto" => Ok(Self::Auto),
            Value::String(word) => word
                .strip_suffix('%')
                .and_then(|n| n.trim().parse::<f64>().ok())
                .filter(|n| (10.0..=100.0).contains(n))
                .map(Self::Percent)
                .ok_or_else(|| {
                    D::Error::custom(format!(
                        "width: expected \"full\", \"auto\", DIPs or \"10%\"..\"100%\", got {word:?}"
                    ))
                }),
            Value::Number(n) => n
                .as_f64()
                .filter(|n| (120.0..=8192.0).contains(n))
                .map(Self::Dips)
                .ok_or_else(|| D::Error::custom("width in DIPs must be between 120 and 8192")),
            other => Err(D::Error::custom(format!("width: unexpected {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TopConfig {
    /// The design it starts from (`presets/top/<id>.json`): layout, metrics, modules, look and
    /// motion, all under the fields set here.
    pub design: TopDesign,
    pub enabled: bool,
    pub monitors: Monitors,
    pub layout: TopLayout,
    pub position: TopPosition,
    /// The bar's own thickness, DIPs.
    pub height: f64,
    /// From the screen edges (and the gap between islands), DIPs. Missing: by layout.
    pub margin: Option<f64>,
    /// Inside the bar (or each island), DIPs. Missing: by layout.
    pub padding: Option<f64>,
    /// Between modules, DIPs. Missing: by layout.
    pub spacing: Option<f64>,
    /// Missing: `full`.
    pub width: Option<Dimension>,
    pub left: Vec<ModuleRef>,
    pub center: Vec<ModuleRef>,
    pub right: Vec<ModuleRef>,
    pub clock: ClockConfig,
    /// This bar's look: its design's, then the shared `look`, then this.
    pub look: Look,
    /// This bar's motion, layered like `look`.
    pub animation: Motion,
}

impl Default for TopConfig {
    fn default() -> Self {
        use Module::*;
        let modules = |list: &[Module]| list.iter().copied().map(ModuleRef::Builtin).collect();
        Self {
            design: TopDesign::default(),
            enabled: true,
            monitors: Monitors::All,
            layout: TopLayout::Strip,
            position: TopPosition::Top,
            height: 32.0,
            margin: None,
            padding: None,
            spacing: None,
            width: None,
            left: modules(&[Start, Workspaces, App]),
            center: modules(&[Clock]),
            right: modules(&[Media, Tray, Network, Volume, Battery, Quick, Power]),
            clock: ClockConfig::default(),
            look: Look::default(),
            animation: Motion::default(),
        }
    }
}

impl TopConfig {
    /// Every module and break, left to right.
    pub fn modules(&self) -> impl Iterator<Item = &ModuleRef> + '_ {
        self.left.iter().chain(&self.center).chain(&self.right)
    }

    pub fn margin(&self) -> f64 {
        self.margin.unwrap_or(match self.layout {
            TopLayout::Strip => 0.0,
            TopLayout::Floating => 8.0,
            TopLayout::Islands => 6.0,
        })
    }

    /// What the top bar reserves: its height plus its margin on both sides.
    pub fn band(&self) -> f64 {
        self.height + 2.0 * self.margin()
    }

    fn fill_defaults(&mut self) {
        let (padding, spacing) = match self.layout {
            TopLayout::Strip => (5.0, 1.0),
            TopLayout::Floating => (6.0, 2.0),
            TopLayout::Islands => (4.0, 2.0),
        };
        self.margin = Some(self.margin());
        self.padding.get_or_insert(padding);
        self.spacing.get_or_insert(spacing);
        self.width.get_or_insert(Dimension::Full);
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

// --- Dock --------------------------------------------------------------------------------------

/// A dock layout. Each style is a page layout (`bars/shell/src/dock/styles.ts`); the band follows
/// from size, padding and margin ([`DockConfig::band`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockStyle {
    /// A pill of icons floating centered on its edge, macOS-style.
    #[default]
    Floating,
    /// A full-width, Windows-style taskbar: `left` modules, task buttons, `right` modules.
    Taskbar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockPosition {
    #[default]
    Bottom,
    Top,
    Left,
    Right,
}

/// Which running apps that aren't pinned the dock shows: `true` (all), `false` (none), or
/// `"monitor"` (those with a window on the dock's monitor; pins then count only those windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Running {
    #[default]
    All,
    Monitor,
    None,
}

impl Serialize for Running {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_bool(true),
            Self::None => serializer.serialize_bool(false),
            Self::Monitor => serializer.serialize_str("monitor"),
        }
    }
}

impl<'de> Deserialize<'de> for Running {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Bool(true) => Ok(Self::All),
            Value::Bool(false) => Ok(Self::None),
            Value::String(word) if word == "monitor" => Ok(Self::Monitor),
            other => Err(serde::de::Error::custom(format!(
                "running: expected true, false or \"monitor\", got {other}"
            ))),
        }
    }
}

/// macOS-like hover zoom: `true`/`false`, or `{ "scale", "range" }`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Magnify {
    pub enabled: bool,
    /// The icon under the pointer, 1..3.
    pub scale: f64,
    /// How far the zoom reaches, in icons, 1..8.
    pub range: f64,
}

impl Default for Magnify {
    fn default() -> Self {
        Self {
            enabled: true,
            scale: 1.5,
            range: 3.0,
        }
    }
}

impl<'de> Deserialize<'de> for Magnify {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(default, deny_unknown_fields)]
        struct Full {
            enabled: bool,
            scale: f64,
            range: f64,
        }
        impl Default for Full {
            fn default() -> Self {
                let Magnify {
                    enabled,
                    scale,
                    range,
                } = Magnify::default();
                Self {
                    enabled,
                    scale,
                    range,
                }
            }
        }
        match Value::deserialize(deserializer)? {
            Value::Bool(enabled) => Ok(Self {
                enabled,
                ..Self::default()
            }),
            other => {
                let full = Full::deserialize(other).map_err(serde::de::Error::custom)?;
                Ok(Self {
                    enabled: full.enabled,
                    scale: full.scale,
                    range: full.range,
                })
            }
        }
    }
}

/// Window thumbnails over a running item: `true`/`false`, or `{ "delay" }` (ms).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Preview {
    pub enabled: bool,
    pub delay: u32,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            enabled: true,
            delay: 450,
        }
    }
}

impl<'de> Deserialize<'de> for Preview {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Full {
            #[serde(default = "yes")]
            enabled: bool,
            #[serde(default = "delay")]
            delay: u32,
        }
        fn yes() -> bool {
            true
        }
        fn delay() -> u32 {
            Preview::default().delay
        }
        match Value::deserialize(deserializer)? {
            Value::Bool(enabled) => Ok(Self {
                enabled,
                ..Self::default()
            }),
            other => {
                let full = Full::deserialize(other).map_err(serde::de::Error::custom)?;
                Ok(Self {
                    enabled: full.enabled,
                    delay: full.delay,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndicatorStyle {
    #[default]
    Dot,
    Bar,
    Glow,
    Pill,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttentionStyle {
    #[default]
    Pulse,
    Bounce,
    None,
}

/// Items the dock can show after its apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Extra {
    /// Minimizes everything (Win+D) and back.
    Desktop,
    /// The Recycle Bin: open it, empty it, full or empty icon.
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemsAlign {
    Start,
    #[default]
    Center,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockConfig {
    /// The design it starts from (`presets/dock/<id>.json`), under the fields set here.
    pub design: DockDesign,
    pub enabled: bool,
    pub monitors: Monitors,
    /// Which dock layout the page draws; the host sizes the band for it.
    pub style: DockStyle,
    pub position: DockPosition,
    /// The icon size in DIPs. Missing: 48 (`floating`) or 24 (`taskbar`).
    pub size: Option<f64>,
    /// From the screen edge (and the room kept toward the windows), DIPs. Missing: 6
    /// (`floating`) or 0.
    pub margin: Option<f64>,
    /// Around the icons, DIPs. Missing: 5 (`floating`) or 12.
    pub padding: Option<f64>,
    /// Between items, DIPs. Missing: 6 (`floating`) or 4.
    pub spacing: Option<f64>,
    pub autohide: bool,
    pub magnify: Magnify,
    pub running: Running,
    /// One item per window instead of per app.
    pub split: bool,
    /// A count badge on apps with more than one window.
    pub counter: bool,
    /// Window titles beside the icons (task buttons).
    pub titles: bool,
    pub indicator: IndicatorStyle,
    pub preview: Preview,
    /// Bounce while a launched app starts.
    pub bounce: bool,
    /// How an app asking for attention (a flashing window) shows.
    pub attention: AttentionStyle,
    pub extras: Vec<Extra>,
    /// Where the task buttons sit in a `taskbar` (centered like Windows 11, or after `left`).
    pub align: ItemsAlign,
    /// The `taskbar` style's modules before and after the task buttons.
    pub left: Vec<ModuleRef>,
    pub right: Vec<ModuleRef>,
    /// A `taskbar` task button's length along the bar, DIPs (16..240). Missing: the icon plus 16,
    /// or 168 with `titles`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button: Option<f64>,
    /// The `taskbar` style's clock (its own format: a taskbar's is often two lines).
    pub clock: ClockConfig,
    /// Missing: the native taskbar's pinned shortcuts (see [`BarsConfig::resolve`]).
    pub pinned: Option<Vec<Pinned>>,
    /// This bar's look: its design's, then the shared `look`, then this.
    pub look: Look,
    /// This bar's motion, layered like `look`.
    pub animation: Motion,
}

impl Default for DockConfig {
    fn default() -> Self {
        use Module::*;
        let modules = |list: &[Module]| list.iter().copied().map(ModuleRef::Builtin).collect();
        Self {
            design: DockDesign::default(),
            enabled: false,
            monitors: Monitors::Primary,
            style: DockStyle::Floating,
            position: DockPosition::Bottom,
            size: None,
            margin: None,
            padding: None,
            spacing: None,
            autohide: false,
            magnify: Magnify::default(),
            running: Running::All,
            split: false,
            counter: true,
            titles: false,
            indicator: IndicatorStyle::Dot,
            preview: Preview::default(),
            bounce: true,
            attention: AttentionStyle::Pulse,
            extras: Vec::new(),
            align: ItemsAlign::Center,
            left: modules(&[Start, Search]),
            right: modules(&[Tray, Network, Volume, Battery, Clock]),
            button: None,
            clock: ClockConfig::default(),
            pinned: None,
            look: Look::default(),
            animation: Motion::default(),
        }
    }
}

impl DockConfig {
    pub fn size(&self) -> f64 {
        self.size.unwrap_or(match self.style {
            DockStyle::Floating => 48.0,
            DockStyle::Taskbar => 24.0,
        })
    }

    fn margin(&self) -> f64 {
        self.margin.unwrap_or(match self.style {
            DockStyle::Floating => 6.0,
            DockStyle::Taskbar => 0.0,
        })
    }

    fn padding(&self) -> f64 {
        self.padding.unwrap_or(match self.style {
            DockStyle::Floating => 5.0,
            DockStyle::Taskbar => 12.0,
        })
    }

    /// The band (DIPs) the dock reserves: icons plus padding, plus the margin on both sides: from
    /// the screen edge, and toward the windows, which leaves its shadow and glow room (for the
    /// default `floating` dock of 48 px icons, 70).
    pub fn band(&self) -> f64 {
        self.size() + 2.0 * self.padding() + 2.0 * self.margin()
    }

    /// The `taskbar` style's modules; a `floating` dock has none.
    pub fn modules(&self) -> impl Iterator<Item = &ModuleRef> + '_ {
        let taskbar = self.style == DockStyle::Taskbar;
        self.left.iter().chain(&self.right).filter(move |_| taskbar)
    }

    fn fill_defaults(&mut self) {
        let spacing = match self.style {
            DockStyle::Floating => 6.0,
            DockStyle::Taskbar => 4.0,
        };
        self.size = Some(self.size());
        self.margin = Some(self.margin());
        self.padding = Some(self.padding());
        self.spacing.get_or_insert(spacing);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    /// An exe, a `.lnk`, a folder, or `shell:AppsFolder\<AUMID>`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// --- Modules -----------------------------------------------------------------------------------

/// A built-in module (docs/bars.md, "Modules").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Module {
    Start,
    Search,
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
    /// The keyboard layout of the app in front.
    Language,
    /// A bell with the number of notifications.
    Notifications,
}

/// An entry of a module list: a built-in module, `custom:<id>`, or `|` (a break between islands,
/// a separator elsewhere).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleRef {
    Builtin(Module),
    Custom(String),
    Break,
}

impl ModuleRef {
    /// `custom:<id>` ids: 1 to 32 of `a-z 0-9 - _`.
    fn valid_id(id: &str) -> bool {
        (1..=32).contains(&id.len())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    }
}

impl Serialize for ModuleRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Builtin(module) => module.serialize(serializer),
            Self::Custom(id) => serializer.serialize_str(&format!("custom:{id}")),
            Self::Break => serializer.serialize_str("|"),
        }
    }
}

impl<'de> Deserialize<'de> for ModuleRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, IntoDeserializer};
        let text = String::deserialize(deserializer)?;
        if text == "|" {
            return Ok(Self::Break);
        }
        if let Some(id) = text.strip_prefix("custom:") {
            return if Self::valid_id(id) {
                Ok(Self::Custom(id.to_owned()))
            } else {
                Err(D::Error::custom(format!(
                    "custom module ids are 1-32 of a-z, 0-9, - and _, got {id:?}"
                )))
            };
        }
        let module: Result<Module, D::Error> =
            Module::deserialize(text.as_str().into_deserializer());
        module.map(Self::Builtin)
    }
}

/// What a click or a wheel notch does: a built-in action, `{ "exec", "args" }` (a program, run
/// as the user) or `{ "open" }` (a path or URI).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Action {
    Builtin(BuiltinAction),
    Exec { exec: String, args: Vec<String> },
    Open { open: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltinAction {
    /// The module's own behavior (to undo a preset's choice).
    Default,
    None,
    /// The launcher (what the Windows key opens too).
    Start,
    Search,
    /// The notifications flyout.
    Notifications,
    /// The quick settings flyout.
    QuickSettings,
    Calendar,
    Network,
    /// The volume mixer flyout.
    Mixer,
    /// The native Start menu, notification center and quick settings (Windows' own panels).
    NativeStart,
    NativeNotifications,
    NativeQuickSettings,
    TaskManager,
    Settings,
    ShowDesktop,
    MediaToggle,
    MediaNext,
    MediaPrevious,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    WorkspaceNext,
    WorkspacePrevious,
    Lock,
    Explorer,
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Exec {
            exec: String,
            #[serde(default)]
            args: Vec<String>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Open {
            open: String,
        }
        let value = Value::deserialize(deserializer)?;
        let action = match &value {
            Value::String(_) => {
                Self::Builtin(BuiltinAction::deserialize(value).map_err(D::Error::custom)?)
            }
            Value::Object(map) if map.contains_key("exec") => {
                let Exec { exec, args } = Exec::deserialize(value).map_err(D::Error::custom)?;
                Self::Exec { exec, args }
            }
            Value::Object(map) if map.contains_key("open") => {
                let Open { open } = Open::deserialize(value).map_err(D::Error::custom)?;
                Self::Open { open }
            }
            other => {
                return Err(D::Error::custom(format!(
                    "an action is a name, {{ \"exec\": … }} or {{ \"open\": … }}, got {other}"
                )));
            }
        };
        if let Self::Exec { exec: target, .. } | Self::Open { open: target } = &action
            && target.trim().is_empty()
        {
            return Err(D::Error::custom("an action's exec/open target is empty"));
        }
        Ok(action)
    }
}

/// Click and wheel actions; unset ones keep the module's own behavior.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Clicks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle: Option<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wheel_up: Option<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wheel_down: Option<Action>,
}

/// Which input an action is bound to (what the page's `run` request names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

impl Clicks {
    pub fn get(&self, trigger: Trigger) -> Option<&Action> {
        match trigger {
            Trigger::Left => self.left.as_ref(),
            Trigger::Middle => self.middle.as_ref(),
            Trigger::Right => self.right.as_ref(),
            Trigger::WheelUp => self.wheel_up.as_ref(),
            Trigger::WheelDown => self.wheel_down.as_ref(),
        }
    }
}

/// Per-module look overrides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModuleStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<BorderStyle>,
    /// Horizontal padding, DIPs 0..32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<f32>,
    /// DIPs 0..32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// Font size in DIPs, 8..32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// 100..900.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModuleOptions {
    pub click: Clicks,
    pub style: ModuleStyle,
    /// An icon-pack key, or an image file (svg, png, ico; absolute or theme-relative).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Custom modules: the program whose output the module shows (see docs/bars.md).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Seconds between runs, 1..86400. Missing: 60.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<f64>,
}

impl ModuleOptions {
    fn validate(&self, name: &ModuleRef) -> Result<()> {
        let label = serde_json::to_string(name)?;
        let custom = matches!(name, ModuleRef::Custom(_));
        if matches!(name, ModuleRef::Break) {
            bail!("modules: \"|\" isn't a module");
        }
        if custom && self.command.as_deref().is_none_or(|c| c.trim().is_empty()) {
            bail!("modules.{label}: a custom module needs a command");
        }
        if !custom && (self.command.is_some() || !self.args.is_empty() || self.interval.is_some()) {
            bail!("modules.{label}: only custom modules take command, args and interval");
        }
        if self.interval.is_some_and(|i| !(1.0..=86400.0).contains(&i)) {
            bail!("modules.{label}.interval must be between 1 and 86400 seconds");
        }
        let style = &self.style;
        check(
            &format!("modules.{label}.style.padding"),
            style.padding,
            0.0..=32.0,
        )?;
        check(
            &format!("modules.{label}.style.radius"),
            style.radius,
            0.0..=32.0,
        )?;
        check(
            &format!("modules.{label}.style.size"),
            style.size,
            8.0..=32.0,
        )?;
        if style.weight.is_some_and(|w| !(100..=900).contains(&w)) {
            bail!("modules.{label}.style.weight must be between 100 and 900");
        }
        if self.icon.as_deref().is_some_and(|i| i.trim().is_empty()) {
            bail!("modules.{label}.icon is empty");
        }
        Ok(())
    }
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
            Self::Named(names) => names.iter().any(|n| same_device(n, device)),
        }
    }
}

/// `DISPLAY2` and `\\.\DISPLAY2` name the same monitor, in any case.
fn same_device(a: &str, b: &str) -> bool {
    let bare = |name: &str| {
        name.strip_prefix(r"\\.\")
            .unwrap_or(name)
            .to_ascii_uppercase()
    };
    bare(a) == bare(b)
}

impl Serialize for Monitors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str("all"),
            Self::Primary => serializer.serialize_str("primary"),
            Self::Named(names) => names.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Monitors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
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

// --- Parsing -----------------------------------------------------------------------------------

/// Top-level fields an override may not set: they're global.
const GLOBAL_ONLY: &[&str] = &["version", "taskbar", "overrides", "vars", "icons"];

impl BarsConfig {
    pub fn parse(json: &str) -> Result<Self> {
        // The user's own text first, strictly: typos and wrong types come with their line.
        let _: Self = serde_json::from_str(json)?;
        let raw: Map<String, Value> = serde_json::from_str(json)?;
        let config = Self::merged(&raw)?;
        for (key, over) in &config.overrides {
            check_override(key, over)?;
            Self::merged(&with_overrides(&raw, [over]))
                .with_context(|| format!("in overrides.{key}"))?;
        }
        Ok(config)
    }

    /// The user's fields over their preset and each bar's design (`presets::layered`), parsed
    /// and checked.
    fn merged(raw: &Map<String, Value>) -> Result<Self> {
        let mut config: Self = serde_json::from_value(layered(raw)?)?;
        config.validate()?;
        config.raw = raw.clone();
        Ok(config)
    }

    /// Loads `path`, or the defaults (an empty file's: the default preset and designs) when there
    /// is no file.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::parse("{}"),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// This config as one monitor's bars see it: the `overrides` for `secondary` or `primary`,
    /// then for the device itself, merged over the file. Fails only if an override stopped
    /// parsing, which [`parse`](Self::parse) already rules out.
    pub fn for_monitor(&self, device: &str, primary: bool) -> Result<Self> {
        let role = if primary { "primary" } else { "secondary" };
        let mut matching: Vec<(u8, &Value)> = self
            .overrides
            .iter()
            .filter_map(|(key, over)| {
                if key.eq_ignore_ascii_case(role) {
                    Some((0, over))
                } else if same_device(key, device) {
                    Some((1, over))
                } else {
                    None
                }
            })
            .collect();
        if matching.is_empty() {
            return Ok(self.clone());
        }
        matching.sort_by_key(|(rank, _)| *rank);
        let raw = with_overrides(&self.raw, matching.into_iter().map(|(_, over)| over));
        let mut config = Self::merged(&raw)?;
        config.overrides.clear();
        config.dock.pinned.clone_from(&self.dock.pinned);
        config.top.clock.format.clone_from(&self.top.clock.format);
        config.dock.clock.format.clone_from(&self.dock.clock.format);
        config.fill_defaults();
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("version must be 1, got {}", self.version);
        }
        let top = &self.top;
        if !(20.0..=80.0).contains(&top.height) {
            bail!("top.height must be between 20 and 80, got {}", top.height);
        }
        let range = |name: &str, value: Option<f64>, hi: f64| {
            check(name, value.map(|v| v as f32), 0.0..=hi as f32)
        };
        range("top.margin", top.margin, 64.0)?;
        range("top.padding", top.padding, 48.0)?;
        range("top.spacing", top.spacing, 32.0)?;
        let dock = &self.dock;
        if dock.size.is_some_and(|s| !(16.0..=128.0).contains(&s)) {
            bail!("dock.size must be between 16 and 128, got {}", dock.size());
        }
        range("dock.margin", dock.margin, 64.0)?;
        range("dock.padding", dock.padding, 48.0)?;
        range("dock.spacing", dock.spacing, 32.0)?;
        let magnify = dock.magnify;
        if !(1.0..=3.0).contains(&magnify.scale) || !(1.0..=8.0).contains(&magnify.range) {
            bail!("dock.magnify needs scale 1..3 and range 1..8");
        }
        if dock.preview.delay > 3000 {
            bail!("dock.preview.delay must be at most 3000 ms");
        }
        once_each("top", top.modules())?;
        once_each("dock", dock.left.iter().chain(&dock.right))?;
        if dock.extras.iter().collect::<BTreeSet<_>>().len() != dock.extras.len() {
            bail!("dock.extras: an extra is listed twice");
        }
        for (bar, clock) in [("top", &top.clock), ("dock", &dock.clock)] {
            if clock.format.as_ref().is_some_and(|f| f.len() > 64) {
                bail!("{bar}.clock.format is longer than 64 characters");
            }
        }
        if dock.button.is_some_and(|b| !(16.0..=240.0).contains(&b)) {
            bail!("dock.button must be between 16 and 240 DIPs");
        }
        let pinned = dock.pinned.iter().flatten();
        if pinned.clone().any(|p| p.path.trim().is_empty()) {
            bail!("dock.pinned: every item needs a path");
        }
        crate::flyout_config::validate(&self.launcher, &self.calendar)?;
        for (name, options) in &self.modules {
            options.validate(name)?;
        }
        let bad_pack = |pack: &str| {
            !(1..=64).contains(&pack.len())
                || pack.starts_with('.')
                || pack
                    .chars()
                    .any(|c| !(c.is_ascii_alphanumeric() || " _.-".contains(c)))
        };
        if self.icons.as_deref().is_some_and(bad_pack) {
            bail!("icons: a pack name is 1-64 letters, digits, spaces, '.', '_' or '-'");
        }
        if let Some(bad) = self.vars.keys().find(|k| !k.starts_with("--")) {
            bail!("vars: {bad:?} isn't a CSS variable name (--name)");
        }
        self.look.validate()?;
        self.animation.validate()?;
        self.top.look.validate_at("top.look")?;
        self.top.animation.validate_named("top.animation")?;
        self.dock.look.validate_at("dock.look")?;
        self.dock.animation.validate_named("dock.animation")?;
        self.appearance.validate()
    }

    /// Fills the defaults that depend on the machine or the layout: the clock format, layout
    /// geometry and, when `pinned` is missing, `default_pins` (the native taskbar's pins).
    pub fn resolve(&mut self, default_pins: impl FnOnce() -> Vec<Pinned>) {
        for clock in [&mut self.top.clock, &mut self.dock.clock] {
            clock
                .format
                .get_or_insert_with(|| ClockConfig::default_format(clock.seconds).to_owned());
        }
        self.dock.pinned.get_or_insert_with(default_pins);
        self.fill_defaults();
    }

    fn fill_defaults(&mut self) {
        self.top.fill_defaults();
        self.dock.fill_defaults();
    }

    /// Every custom module a bar of this config shows, with its options.
    pub fn custom_modules(&self) -> impl Iterator<Item = (&str, &ModuleOptions)> + '_ {
        self.modules
            .iter()
            .filter_map(|(name, options)| match name {
                ModuleRef::Custom(id) => Some((id.as_str(), options)),
                _ => None,
            })
    }
}

fn once_each<'a>(section: &str, modules: impl Iterator<Item = &'a ModuleRef>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for module in modules.filter(|m| **m != ModuleRef::Break) {
        if !seen.insert(module) {
            bail!(
                "{section}: module {} is listed twice",
                serde_json::to_string(module)?
            );
        }
    }
    Ok(())
}

fn check_override(key: &str, over: &Value) -> Result<()> {
    let Value::Object(map) = over else {
        bail!("overrides.{key} must be an object");
    };
    if key.trim().is_empty() {
        bail!("overrides: a key must name a monitor (DISPLAY2, primary or secondary)");
    }
    if let Some(global) = GLOBAL_ONLY.iter().find(|g| map.contains_key(**g)) {
        bail!("overrides.{key}.{global}: that's global, set it outside overrides");
    }
    if over.pointer("/dock/pinned").is_some() {
        bail!("overrides.{key}.dock.pinned: pins are shared by every dock");
    }
    Ok(())
}

/// `raw` without its `overrides`, with `overs` merged over it in order.
fn with_overrides<'a>(
    raw: &Map<String, Value>,
    overs: impl IntoIterator<Item = &'a Value>,
) -> Map<String, Value> {
    let mut value = Value::Object(raw.clone());
    if let Value::Object(map) = &mut value {
        map.remove("overrides");
    }
    for over in overs {
        merge(&mut value, over);
    }
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
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
    use crate::style::Family;

    #[test]
    fn no_file_and_empty_object_mean_the_defaults() {
        let config = BarsConfig::parse("{}").unwrap();
        assert_eq!(config.preset, Preset::Monochrome);
        assert!(config.top.enabled);
        assert!(!config.dock.enabled);
        assert_eq!(config.taskbar.mode, TaskbarMode::Keep);
        assert_eq!(config.taskbar.alignment, None);
        assert_eq!(config.dock.monitors, Monitors::Primary);
        assert_eq!(config.top.monitors, Monitors::All);
        let defaults = BarsConfig::default();
        assert_eq!(
            (config.top.height, config.dock.size()),
            (defaults.top.height, defaults.dock.size())
        );
    }

    #[test]
    fn parses_the_documented_example() {
        let json = r#"{
          "version": 1,
          "taskbar": { "mode": "hide", "alignment": "left" },
          "top": { "enabled": true, "monitors": ["\\\\.\\DISPLAY2"], "height": 28,
                   "left": ["start"], "center": ["clock"], "right": ["volume", "|", "power"],
                   "clock": { "seconds": true } },
          "dock": { "enabled": true, "position": "left", "size": 56, "autohide": true,
                    "pinned": [ { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" } ] },
          "appearance": { "surface": "solid", "opacity": 0.9 }
        }"#;
        let config = BarsConfig::parse(json).unwrap();
        assert_eq!(config.taskbar.mode, TaskbarMode::Hide);
        assert_eq!(config.taskbar.alignment.map(Alignment::value), Some(0));
        assert!(config.top.monitors.includes(r"\\.\display2", false));
        assert!(config.top.monitors.includes("DISPLAY2", false));
        assert!(!config.top.monitors.includes(r"\\.\DISPLAY1", true));
        assert_eq!(config.top.right[1], ModuleRef::Break);
        assert_eq!(config.dock.position, DockPosition::Left);
        assert_eq!(config.dock.pinned.as_ref().map(Vec::len), Some(1));
        assert!(
            config.dock.magnify.enabled,
            "unset fields keep their defaults"
        );
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
            r#"{ "top": { "left": ["custom:a", "custom:a"] } }"#,
            r#"{ "top": { "left": ["custom:Bad Id"] } }"#,
            r#"{ "top": { "height": 4 } }"#,
            r#"{ "top": { "width": "2%" } }"#,
            r#"{ "top": { "layout": "ribbon" } }"#,
            r#"{ "dock": { "size": 500 } }"#,
            r#"{ "dock": { "style": "ribbon" } }"#,
            r#"{ "dock": { "running": "sometimes" } }"#,
            r#"{ "dock": { "magnify": { "scale": 9 } } }"#,
            r#"{ "dock": { "extras": ["trash", "trash"] } }"#,
            r#"{ "dock": { "pinned": [ { "path": " " } ] } }"#,
            r#"{ "preset": "vaporwave" }"#,
            r#"{ "modules": { "clock": { "command": "x" } } }"#,
            r#"{ "modules": { "custom:x": { "interval": 5 } } }"#,
            r#"{ "modules": { "clock": { "click": { "left": "explode" } } } }"#,
            r#"{ "modules": { "clock": { "click": { "left": { "exec": "" } } } } }"#,
            r#"{ "icons": "../escape" }"#,
            r#"{ "vars": { "glow": 1 } }"#,
            r#"{ "version": 2 }"#,
            r#"{ "appearance": { "opacity": 3 } }"#,
            r#"{ "overrides": { "DISPLAY2": { "taskbar": { "mode": "hide" } } } }"#,
            r#"{ "overrides": { "DISPLAY2": { "dock": { "pinned": [] } } } }"#,
            r#"{ "overrides": { "DISPLAY2": { "top": { "height": 400 } } } }"#,
        ] {
            assert!(BarsConfig::parse(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn the_preset_sits_under_the_users_fields() {
        let config = BarsConfig::parse(r#"{ "preset": "windows11" }"#).unwrap();
        assert_eq!(config.dock.style, DockStyle::Taskbar);
        assert!(!config.top.enabled, "windows11 is one taskbar");
        let config =
            BarsConfig::parse(r#"{ "preset": "windows11", "top": { "enabled": true } }"#).unwrap();
        assert!(config.top.enabled, "the user wins");
        assert_eq!(config.dock.style, DockStyle::Taskbar, "the rest stays");
    }

    /// A file written before designs existed (the shape Studio and the tray wrote) keeps its
    /// look: its preset now names the designs it was made of.
    #[test]
    fn files_from_before_designs_still_resolve_the_same() {
        let config = BarsConfig::parse(
            r#"{ "version": 1, "preset": "islands", "taskbar": { "mode": "hide" },
                 "dock": { "enabled": true, "monitors": "all", "position": "bottom" },
                 "icons": "mono", "tray": { "host": true } }"#,
        )
        .unwrap();
        assert_eq!((config.top.design, config.dock.design), (TopDesign::Islands, DockDesign::Islands));
        assert_eq!(config.top.layout, TopLayout::Islands);
        assert!(config.top.right.contains(&ModuleRef::Break));
        assert_eq!(config.dock.indicator, IndicatorStyle::Bar);
        assert!(config.dock.enabled && config.dock.monitors == Monitors::All);
        assert!(matches!(config.top.look.background, Some(crate::style::Fill::Solid { .. })));
        assert_eq!(config.taskbar.mode, TaskbarMode::Hide);

        let win = BarsConfig::parse(r#"{ "preset": "windows11", "taskbar": { "mode": "hide" } }"#).unwrap();
        assert!(!win.top.enabled);
        assert_eq!(win.dock.design, DockDesign::Windows11);
        assert_eq!(win.dock.style, DockStyle::Taskbar);
        assert_eq!(win.dock.band(), 48.0, "Windows 11's taskbar is 48 DIPs");
    }

    #[test]
    fn each_bar_takes_its_own_design() {
        let config = BarsConfig::parse(
            r#"{ "top": { "design": "macos" }, "dock": { "design": "windows11", "enabled": true } }"#,
        )
        .unwrap();
        assert_eq!(config.preset, Preset::Monochrome, "the preset only fills what's unnamed");
        assert_eq!(config.top.height, 24.0);
        assert_eq!(config.top.look.family, Some(Family::Macos));
        assert_eq!(config.dock.style, DockStyle::Taskbar);
        assert_eq!(config.dock.look.family, Some(Family::Fluent));
        assert_eq!(config.dock.clock.format.as_deref(), Some("HH:mm\ndd/MM/yyyy"));

        // A bar's design beats the preset's choice for it; the other bar keeps the preset's.
        let config =
            BarsConfig::parse(r#"{ "preset": "macos", "dock": { "design": "chromeos" } }"#).unwrap();
        assert_eq!((config.top.design, config.dock.design), (TopDesign::Macos, DockDesign::Chromeos));
        assert!(BarsConfig::parse(r#"{ "top": { "design": "windows11" } }"#).is_err(), "a dock design");
        assert!(BarsConfig::parse(r#"{ "dock": { "design": "capsule" } }"#).is_err(), "a top design");
    }

    #[test]
    fn looks_layer_design_then_shared_then_the_bars_own() {
        let config = BarsConfig::parse(
            r#"{ "top": { "design": "macos", "look": { "tint": 0.9 } },
                 "dock": { "design": "macos" },
                 "look": { "tint": 0.2, "noise": 0.3 },
                 "animation": { "duration": 90 } }"#,
        )
        .unwrap();
        assert_eq!(config.top.look.tint, Some(0.9), "the bar's own wins");
        assert_eq!(config.dock.look.tint, Some(0.2), "then the shared look");
        assert_eq!(config.top.look.noise, Some(0.3));
        assert_eq!(config.dock.look.saturation, Some(1.85), "the design's fills the rest");
        assert_eq!(config.top.animation.duration, Some(90.0));
        assert_eq!(config.dock.animation.duration, Some(90.0));

        // Another background type replaces the design's whole background, in one bar only.
        let config = BarsConfig::parse(
            r##"{ "top": { "look": { "background": { "type": "solid", "color": "#101010" } } } }"##,
        )
        .unwrap();
        assert!(matches!(config.top.look.background, Some(crate::style::Fill::Solid { .. })));
        assert!(config.dock.look.wallpaper().is_some(), "the dock keeps its design's glass");
    }

    #[test]
    fn bar_looks_are_checked_with_their_paths() {
        let error = BarsConfig::parse(r#"{ "dock": { "look": { "tint": 4 } } }"#).unwrap_err();
        assert!(format!("{error:#}").contains("dock.look.tint"), "{error:#}");
        let error = BarsConfig::parse(r#"{ "top": { "animation": { "magnify": {} } } }"#).unwrap_err();
        assert!(format!("{error:#}").contains("top.animation.magnify"), "{error:#}");
        assert!(BarsConfig::parse(r#"{ "dock": { "button": 4 } }"#).is_err());
    }

    #[test]
    fn a_monitor_can_have_its_own_designs() {
        let config = BarsConfig::parse(
            r#"{ "top": { "design": "macos" },
                 "overrides": { "secondary": { "top": { "design": "minimal" } } } }"#,
        )
        .unwrap();
        assert_eq!(config.for_monitor("DISPLAY1", true).unwrap().top.design, TopDesign::Macos);
        let second = config.for_monitor("DISPLAY2", false).unwrap();
        assert_eq!(second.top.design, TopDesign::Minimal);
        assert!(matches!(second.top.look.background, Some(crate::style::Fill::None)));
    }

    #[test]
    fn no_file_is_an_empty_file() {
        let dir = crate::files::temp_dir("config-none");
        let missing = BarsConfig::load(&dir.join(FILE)).unwrap();
        assert_eq!(missing, BarsConfig::parse("{}").unwrap());
        assert!(missing.top.look.wallpaper().is_some(), "the default design's look");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn overrides_apply_per_monitor() {
        let config = BarsConfig::parse(
            r#"{ "dock": { "enabled": true, "pinned": [ { "path": "a.exe" } ] },
                 "overrides": {
                   "secondary": { "dock": { "position": "left" } },
                   "DISPLAY3": { "preset": "islands", "dock": { "position": "right", "size": 40 } }
                 } }"#,
        )
        .unwrap();
        let primary = config.for_monitor(r"\\.\DISPLAY1", true).unwrap();
        assert_eq!(primary.dock.position, DockPosition::Bottom);
        let second = config.for_monitor(r"\\.\DISPLAY2", false).unwrap();
        assert_eq!(second.dock.position, DockPosition::Left);
        let third = config.for_monitor(r"\\.\display3", false).unwrap();
        assert_eq!(
            third.dock.position,
            DockPosition::Right,
            "the device beats its role"
        );
        assert_eq!(third.dock.size(), 40.0);
        assert_eq!(third.preset, Preset::Islands);
        assert_eq!(third.top.layout, TopLayout::Islands);
        assert_eq!(
            third.dock.pinned.as_ref().map(Vec::len),
            Some(1),
            "shared pins"
        );
        assert!(third.overrides.is_empty());
    }

    #[test]
    fn modules_take_actions_styles_and_custom_commands() {
        let config = BarsConfig::parse(
            r##"{ "top": { "center": [], "right": ["custom:weather", "clock"] },
                 "modules": {
                   "clock": { "click": { "left": "notifications", "middle": { "exec": "notepad.exe", "args": ["a"] },
                              "wheelUp": "volume-up", "right": { "open": "ms-settings:dateandtime" } },
                              "style": { "color": "accent", "padding": 10, "weight": 600 } },
                   "custom:weather": { "command": "curl", "args": ["-s", "wttr.in?format=1"], "interval": 900 }
                 } }"##,
        )
        .unwrap();
        let clock = &config.modules[&ModuleRef::Builtin(Module::Clock)];
        assert_eq!(
            clock.click.get(Trigger::Left),
            Some(&Action::Builtin(BuiltinAction::Notifications))
        );
        assert!(
            matches!(clock.click.get(Trigger::Middle), Some(Action::Exec { args, .. }) if args == &["a"])
        );
        assert!(matches!(
            clock.click.get(Trigger::Right),
            Some(Action::Open { .. })
        ));
        let custom: Vec<&str> = config.custom_modules().map(|(id, _)| id).collect();
        assert_eq!(custom, ["weather"]);
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["modules"]["custom:weather"]["interval"], 900.0);
        assert_eq!(json["modules"]["clock"]["click"]["wheelUp"], "volume-up");
    }

    #[test]
    fn resolve_fills_the_clock_format_layout_and_missing_pins_only() {
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
        assert_eq!(config.top.margin, Some(0.0));
        assert_eq!(config.dock.size, Some(48.0));

        let mut config = BarsConfig::parse(
            r#"{ "top": { "clock": { "seconds": true }, "layout": "floating" }, "dock": { "pinned": [], "style": "taskbar" } }"#,
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
        assert_eq!(config.top.margin, Some(8.0));
        assert_eq!(config.top.band(), config.top.height + 16.0);
        assert_eq!(config.dock.band(), 24.0 + 24.0, "a 48 DIP taskbar");
    }

    #[test]
    fn serializes_for_the_page_without_the_user_appearance() {
        let mut config = BarsConfig::default();
        config.resolve(Vec::new);
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["top"]["monitors"], "all");
        assert_eq!(json["top"]["width"], "full");
        assert_eq!(json["dock"]["position"], "bottom");
        assert_eq!(json["dock"]["style"], "floating");
        assert_eq!(json["dock"]["running"], true);
        assert_eq!(json["dock"]["magnify"]["scale"], 1.5);
        assert_eq!(
            config.dock.band(),
            70.0,
            "icons plus pill padding plus the margin on both sides"
        );
        assert_eq!(json["top"]["right"][0], "media");
        assert!(json.get("appearance").is_none());
        assert!(json.get("overrides").is_none());
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
        assert_eq!(config.dock.size(), 40.0);
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

    /// The page runs the built-in actions (`bars/shell/src/actions.ts`): both sides must name the
    /// same ones, or a valid `bars.json` would bind an action the page can't run (or the reverse).
    #[test]
    fn builtin_actions_are_the_pages() {
        use BuiltinAction::*;
        let host = [
            Default,
            None,
            Start,
            Search,
            Notifications,
            QuickSettings,
            Calendar,
            Network,
            Mixer,
            NativeStart,
            NativeNotifications,
            NativeQuickSettings,
            TaskManager,
            Settings,
            ShowDesktop,
            MediaToggle,
            MediaNext,
            MediaPrevious,
            VolumeUp,
            VolumeDown,
            VolumeMute,
            WorkspaceNext,
            WorkspacePrevious,
            Lock,
            Explorer,
        ]
        .map(|action| serde_json::to_value(action).unwrap());

        let protocol = include_str!("../../../bars/shell/src/protocol.ts");
        let list = protocol
            .split("BUILTIN_ACTIONS = [")
            .nth(1)
            .and_then(|rest| rest.split("] as const").next())
            .expect("protocol.ts lists BUILTIN_ACTIONS");
        let page: Vec<Value> = list
            .split(',')
            .map(|name| name.trim().trim_matches('"'))
            .filter(|name| !name.is_empty())
            .map(Value::from)
            .collect();
        assert_eq!(page, host);
    }

    #[test]
    fn native_actions_are_bindable() {
        let config = BarsConfig::parse(
            r#"{ "modules": { "start": { "click": { "left": "native-start", "right": "start" } },
                              "quick": { "click": { "left": "native-quick-settings" } },
                              "notifications": { "click": { "left": "native-notifications" } } } }"#,
        )
        .unwrap();
        let click = |module| &config.modules[&ModuleRef::Builtin(module)].click;
        assert_eq!(
            click(Module::Start).get(Trigger::Left),
            Some(&Action::Builtin(BuiltinAction::NativeStart))
        );
        assert_eq!(
            click(Module::Start).get(Trigger::Right),
            Some(&Action::Builtin(BuiltinAction::Start))
        );
        assert_eq!(
            click(Module::Quick).get(Trigger::Left),
            Some(&Action::Builtin(BuiltinAction::NativeQuickSettings))
        );
        assert_eq!(
            click(Module::Notifications).get(Trigger::Left),
            Some(&Action::Builtin(BuiltinAction::NativeNotifications))
        );
    }
}
