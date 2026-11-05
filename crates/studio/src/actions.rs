//! What Studio's controls do: the pages, the places inside them, and every action a hit can
//! carry. The app runs them in `App::run`.

use std::sync::Arc;

use mypaper_config::look::Style;
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

    pub fn glyph(self) -> char {
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

    pub fn glyph(self) -> char {
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

/// Text fields Studio keeps (windowless RichEdit), by what they edit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldId {
    /// The command palette's input.
    Palette,
    WallpaperSearch,
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
    StartMypaper,
    StopMypaper,
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
    PinAdd,
    PinRemove(usize),
    PinMove(usize, i32),
    PinsFromWindows,
    // Settings
    ToggleAutostart(bool),
    MenuReset,
    MenuAdd(String),
    MenuRemove(usize),
    MenuMove(usize, i32),
    StartOver(DocId),
    ShowFile(DocId),
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
