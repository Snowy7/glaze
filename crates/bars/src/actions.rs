//! Typed inputs and subscriptions for the native shell's Windows services.
use crate::config::Extra;
use crate::{BarsConfig, Module, ModuleRef};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarKind {
    Top,
    Dock,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Notifications,
    Quick,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    StartMenu,
    ActionCenter(Panel),
    Settings(&'static str),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Toggle,
    Next,
    Previous,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Double,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    Sleep,
    Signout,
    Restart,
    Shutdown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
}
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
        Module::Start
        | Module::Search
        | Module::Quick
        | Module::Power
        | Module::T3
        | Module::Obs
        | Module::Docker => None,
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
