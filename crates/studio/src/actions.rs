//! What Studio's controls do: the pages, the places inside them, and every action a hit can
//! carry. The app runs them in `App::run`.

use glaze_ui::controls::Glyph;
use std::sync::Arc;

use glaze_config::look::Style;
use serde_json::Value;

use crate::docs::DocId;

/// The six places plus Settings (Ctrl 1–6, Ctrl ,).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Wallpapers,
    Look,
    Widgets,
    Bars,
    Integrations,
    Settings,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Home,
        Page::Wallpapers,
        Page::Look,
        Page::Widgets,
        Page::Bars,
        Page::Integrations,
        Page::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Wallpapers => "Wallpapers",
            Page::Look => "Look",
            Page::Widgets => "Widgets",
            Page::Bars => "Bars",
            Page::Integrations => "Integrations",
            Page::Settings => "Settings",
        }
    }

    pub fn glyph(self) -> Glyph {
        use crate::theme::glyph;
        match self {
            Page::Home => glyph::HOME,
            Page::Wallpapers => glyph::WALLPAPERS,
            Page::Look => glyph::LOOK,
            Page::Widgets => glyph::WIDGETS,
            Page::Bars => glyph::BARS,
            Page::Integrations => glyph::INTEGRATIONS,
            Page::Settings => glyph::SETTINGS,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Wallpapers => "wallpapers",
            Page::Look => "look",
            Page::Widgets => "widgets",
            Page::Bars => "bars",
            Page::Integrations => "integrations",
            Page::Settings => "settings",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// Settings' sub-list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    General,
    Performance,
    Desktop,
    Keyboard,
    Troubleshoot,
    About,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::General,
        Section::Performance,
        Section::Desktop,
        Section::Keyboard,
        Section::Troubleshoot,
        Section::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Performance => "Performance and power",
            Section::Desktop => "Desktop",
            Section::Keyboard => "Keyboard",
            Section::Troubleshoot => "Troubleshoot",
            Section::About => "About",
        }
    }

    pub fn glyph(self) -> Glyph {
        use crate::theme::glyph;
        match self {
            Section::General => glyph::SLIDERS,
            Section::Performance => glyph::GAUGE,
            Section::Desktop => glyph::DESKTOP,
            Section::Keyboard => glyph::KEYBOARD,
            Section::Troubleshoot => glyph::WRENCH,
            Section::About => glyph::INFO,
        }
    }
}

/// A pinned-apps list: the dock's, or Start's own (which otherwise follows the dock's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pins {
    Dock,
    Start,
}

impl Pins {
    /// Where the list is in `bars.json`.
    pub fn path(self) -> &'static str {
        match self {
            Pins::Dock => "/dock/pinned",
            Pins::Start => "/launcher/pinned",
        }
    }
}

/// Which bar the Bars inspector shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bar {
    Top,
    Dock,
}

impl Bar {
    pub fn key(self) -> &'static str {
        match self {
            Bar::Top => "top",
            Bar::Dock => "dock",
        }
    }
}

/// A module's place: a bar and one of its lists.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slot {
    pub bar: Bar,
    /// `left`, `center`, `right` (top) or `left`, `right` (dock).
    pub list: &'static str,
    pub index: usize,
}

/// A Bars panel (nothing selected › Panels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Start,
    Calendar,
    Sound,
    Tray,
    Notifications,
}

/// Where a wallpaper comes from (Add wallpaper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    File,
    Folder,
    Web,
    App,
}

/// One edit of a document value: the doc, the JSON pointer and what to call the change.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub doc: DocId,
    pub path: Arc<str>,
    pub label: Arc<str>,
}

impl Edit {
    pub fn new(doc: DocId, path: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            doc,
            path: path.into(),
            label: label.into(),
        }
    }
}

/// A select's options: values and their labels, opened as a native menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Choices {
    pub edit: Edit,
    pub options: Vec<(Value, String)>,
    pub current: Option<Value>,
}

/// A row of an action menu ("…" buttons, Change, dropdowns that run commands), opened as the
/// kit's native menu anchored under its control. Choosing a row runs its action.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    Item {
        label: String,
        /// A Segoe Fluent Icons glyph for the leading column.
        icon: Option<char>,
        /// `Some(true)` marks the current choice of a group.
        checked: Option<bool>,
        enabled: bool,
        action: Action,
    },
    Submenu { label: String, items: Vec<MenuItem> },
    Separator,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, action: Action) -> Self {
        Self::Item { label: label.into(), icon: None, checked: None, enabled: true, action }
    }
    /// One choice of a group: checked while it is the current value.
    pub fn choice(label: impl Into<String>, on: bool, action: Action) -> Self {
        Self::Item { label: label.into(), icon: None, checked: Some(on), enabled: true, action }
    }
    pub fn icon(mut self, glyph: char) -> Self {
        if let Self::Item { icon, .. } = &mut self {
            *icon = Some(glyph);
        }
        self
    }
    pub fn enabled(mut self, on: bool) -> Self {
        if let Self::Item { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }
}

/// Segoe Fluent Icons code points for menu rows.
pub mod menu_icon {
    pub const COPY: char = '\u{E8C8}';
    pub const DELETE: char = '\u{E74D}';
    pub const UP: char = '\u{E74A}';
    pub const DOWN: char = '\u{E74B}';
    pub const UNDO: char = '\u{E7A7}';
    pub const CODE: char = '\u{E943}';
    pub const PHOTO: char = '\u{E91B}';
    pub const VIDEO: char = '\u{E714}';
    pub const COLOR: char = '\u{E790}';
    pub const SHADER: char = '\u{E943}';
    pub const SCENE: char = '\u{E81E}';
    pub const INFO: char = '\u{E946}';
}

/// Text fields Studio keeps (windowless RichEdit), by what they edit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldId {
    ShaderCode,
    /// The command palette's input.
    Palette,
    WallpaperSearch,
    /// The effect gallery's search.
    EffectSearch,
    AddUrl,
    AddName,
    AddArgs,
    WidgetSearch,
    IntegrationSearch,
    PlaceSearch,
    /// A hex colour for this setting path.
    Hex(Arc<str>),
    /// A text setting, by document and path.
    Text(DocId, Arc<str>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Panel(crate::panel::Command),
    /// A hit with nothing to do (a scrim edge, a scroll thumb).
    None,
    Minimize,
    Maximize,
    Close,
    Nav(Page),
    Section(Section),
    /// The inspector's back link, or Esc: up one level.
    Back,
    Palette,
    Undo,
    Redo,
    Peek,
    Pause,
    StartGlaze,
    StopGlaze,
    Restart(&'static str),
    RestoreDesktop,
    CopyDiagnostics,
    OpenLogs,
    /// Opens or closes a More disclosure.
    More(u64),
    /// Sets the value (segments, tiles, swatches).
    Set(Edit, Value),
    /// Flips a switch whose value is the given one now.
    Toggle(Edit, bool),
    /// A slider (the drag supplies the value); `factor` scales stored to shown values.
    Slider(Edit),
    /// Removes the value: back to inherited (↺).
    Unset(Edit),
    /// Opens a select's menu.
    Choose(Choices),
    /// Opens an action menu under the control that ran it.
    Menu(Vec<MenuItem>),
    Field(FieldId),
    // Wallpapers
    SelectWallpaper(usize),
    UseWallpaper(usize),
    WallpaperFilter(u8),
    AddWallpaper,
    AddSource(Source),
    AddChoose(Source),
    AddCommit(bool),
    AddBack,
    CopyWallpaper(usize),
    RemoveWallpaper(usize),
    Reveal(std::path::PathBuf),
    /// Home's strip "All wallpapers ›", or Performance's "have their own ›".
    WallpapersOwn,
    // Look
    LookTab(bool),
    LookScope(bool),
    /// Make a new, empty icon pack of your own and use it.
    IconPackNew,
    /// Give an app its own icon in the current pack (asks for the app, then the picture).
    IconPackAdd,
    IconPackRemove(usize),
    SetStyle(Option<Style>),
    WindowsLook(usize),
    LookReset,
    /// Specimen (false) or the display's own layout (true) under the Look stage.
    StageReal(bool),
    // Widgets and stages
    Display(usize),
    SelectWidget(Option<String>),
    AddWidgetSheet,
    AddWidget(String),
    DuplicateWidget,
    RemoveWidget,
    ResizeWidget(i32, i32),
    ArrangeOnDesktop,
    /// A widget setting's list: add a row, remove a row, move one.
    ListAdd(String),
    ListRemove(String, usize),
    PlacePick(usize),
    PlaceAuto,
    PickFile(String, String),
    // Bars
    SelectBar(Option<Bar>),
    SelectModule(Option<Slot>),
    SelectPanel(Option<Panel>),
    Preset(&'static str),
    AddModule(Bar),
    InsertModule(Bar, &'static str, String),
    RemoveModule(Slot),
    MoveModule(Slot, i32),
    DeleteCustom(String),
    ResetBar(Bar),
    StartBars,
    PinAdd(Pins),
    PinRemove(Pins, usize),
    PinMove(Pins, usize, i32),
    /// Start an own list from what the list follows now.
    PinsOwn(Pins),
    // Settings
    ToggleAutostart(bool),
    MenuReset,
    MenuAdd(String),
    MenuRemove(usize),
    MenuMove(usize, i32),
    StartOver(DocId),
    ShowFile(DocId),
    /// Quit and start Studio again (a file was saved by a newer Glaze).
    RestartStudio,
    // Integrations
    SelectIntegration(usize),
    // Overlays
    Dismiss,
    PaletteRun(usize),
    ToastButton,
    FirstRunPick(usize),
    FirstRunTaskbar(usize),
    FirstRunNext,
    FirstRunSkip,
    /// Opens a URL or file with the shell.
    Open(String),
}
