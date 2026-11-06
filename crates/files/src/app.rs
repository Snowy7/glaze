//! Browser controller. Only visible rows enter the kit display list; services return immutable
//! snapshots tagged with navigation generations so stale work never replaces a new folder.
use crate::{
    backend::{Backend, Reply},
    model::{Column, Entry as FileEntry, History, Preferences, Selection, Sort, View},
    native,
    shell::{self, Decoration, Location, Operation},
    transfer::{self, DropTarget},
};
use anyhow::Result;
use mypaper_ui::{icons::Icon, *};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

pub const BACK: u32 = 1;
pub const FORWARD: u32 = 2;
pub const UP: u32 = 3;
pub const ADDRESS: u32 = 4;
pub const SEARCH: u32 = 5;
pub const NEW: u32 = 6;
pub const CUT: u32 = 7;
pub const COPY: u32 = 8;
pub const PASTE: u32 = 9;
pub const RENAME: u32 = 10;
pub const DELETE: u32 = 11;
pub const DETAILS: u32 = 12;
pub const ICONS: u32 = 13;
pub const REFRESH: u32 = 14;
pub const MORE: u32 = 15;
pub const SUBMIT: u32 = 16;
pub const CANCEL: u32 = 17;
pub const MINIMIZE: u32 = 18;
pub const MAXIMIZE: u32 = 19;
pub const CLOSE: u32 = 20;
pub const EDIT_FIELD: u32 = 21;
pub const NEW_TAB: u32 = 22;
pub const PREVIEW: u32 = 23;
pub const LOOK_CLOSE: u32 = 24;
pub const LOOK_PREVIOUS: u32 = 25;
pub const LOOK_NEXT: u32 = 26;
pub const PREVIEW_CLOSE: u32 = 27;
pub const HEADER: u32 = 100;
pub const SIDEBAR: u32 = 200;
pub const CRUMB: u32 = 500;
pub const TAB: u32 = 1000;
pub const TAB_CLOSE: u32 = 2000;
pub const ITEM: u32 = 10_000;

#[derive(Clone, Copy)]
pub enum Command {
    Open,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    NewFolder,
    Pin,
    Refresh,
    Details,
    Icons,
    Shell,
    Terminal,
    CopyPath,
    Preview,
}

#[derive(Clone, PartialEq)]
pub enum Field {
    None,
    Address,
    Search,
    Rename(String),
    NewFolder,
}

pub enum Drag {
    Tab {
        index: usize,
        start: Point,
    },
    Item {
        start: Point,
    },
    Column {
        index: usize,
        start: f32,
        width: f32,
    },
    Rubber {
        start: Point,
        now: Point,
        initial: BTreeSet<usize>,
    },
}

#[derive(Default)]
pub struct Geometry {
    pub tabs: Rect,
    pub preview: Rect,
    pub viewport: Rect,
    pub address: Rect,
    pub search: Rect,
    pub edit: Rect,
    pub columns: Vec<f32>,
    pub rows: Vec<(usize, Rect)>,
    pub crumbs: Vec<String>,
    pub row: f32,
    pub grid_columns: usize,
}

pub struct App {
    pub quick_look: bool,
    pub preview_content: Option<crate::preview::Content>,
    pub preview_path: String,
    pub preview_scroll: Scroll,
    preview_service: Option<crate::preview::Service>,
    preview_ticket: u64,
    preview_handler: Option<crate::preview::ShellHandler>,
    handler_attempted: String,
    pub tabs: Vec<crate::tabs::Tab>,
    pub active_tab: usize,
    cache: crate::tabs::FolderCache,
    pending_paint: Option<(Instant, bool, String)>,
    pub layered: mypaper_ui::layered_app::LayeredApp,
    pub surface: Surface,
    pub backdrop: std::rc::Rc<crate::backdrop::Client>,
    pub glass_active: bool,
    first_frame: bool,
    pub theme: Theme,
    pub scheme_override: Option<Scheme>,
    pub interaction: Interaction,
    pub location: Option<Location>,
    pub loading: bool,
    pub elapsed_ms: u128,
    pub status: String,
    pub error: Option<String>,
    pub busy: bool,
    pub entries: Arc<Vec<FileEntry>>,
    pub shown: Arc<Vec<FileEntry>>,
    pub history: History,
    pub selection: Selection,
    pub sort: Sort,
    pub preferences: Preferences,
    pub scroll: Scroll,
    pub sidebar_scroll: Scroll,
    pub sidebar: Vec<(String, String, Icon)>,
    pub field: Field,
    pub address: TextField,
    pub search: TextField,
    pub editor: TextField,
    pub query_text: String,
    pub indexed: bool,
    pub geometry: Geometry,
    pub drag: Option<Drag>,
    pub sizes: HashMap<String, crate::model::Size>,
    pub decorations: HashMap<(String, i32), Option<Decoration>>,
    pending_images: HashSet<(String, i32)>,
    image_order: VecDeque<(String, i32)>,
    pub backend: Backend,
    watch: Option<(String, crate::backend::FolderWatch)>,
    directory_dirty: bool,
    refreshing: bool,
    drop_target: DropTarget,
    menu: Menu<Command>,
    menu_at: (i32, i32),
    epoch: u64,
    query: u64,
    pending_history: bool,
    typeahead: String,
    typed_at: Instant,
    pub closing: bool,
}

pub fn control() -> bool {
    // SAFETY: GetKeyState is a read-only query on this UI thread.
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}
pub fn shift() -> bool {
    // SAFETY: GetKeyState is a read-only query on this UI thread.
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

pub fn clipboard_has_files() -> bool {
    // SAFETY: this read-only format query neither opens the clipboard nor borrows its data.
    unsafe { windows::Win32::System::DataExchange::IsClipboardFormatAvailable(15) }.is_ok()
}

impl App {
    pub fn new(path: String, scheme_override: Option<Scheme>) -> Result<Self> {
        let surface = Surface::new(Kind::App { min: (900, 560) }, "mypaper Files", Role::Window)?;
        native::install(surface.hwnd())?;
        let layered = mypaper_ui::layered_app::LayeredApp::new(&surface)?;
        let backdrop = crate::backdrop::Client::install(surface.hwnd());
        let theme = crate::look::load(scheme_override);
        let s = surface.scale();
        surface.set_background(theme.color.surface);
        surface.set_frame(theme.scheme == Scheme::Dark, theme.color.surface);
        let field = || {
            TextField::new(
                surface.hwnd(),
                Rect::new(0.0, 0.0, 200.0, 24.0),
                false,
                theme.font.font(TextRole::Body).size * s,
                theme.color.label,
            )
        };
        let (address, search, editor) = (field()?, field()?, field()?);
        let backend = Backend::new(surface.hwnd())?;
        let drop_target = DropTarget::new(surface.hwnd())?;
        let preferences = Preferences::load();
        let (paths, active_tab) = if path.is_empty() {
            preferences.session()
        } else {
            (vec![path], 0)
        };
        let path = paths[active_tab].clone();
        let mut app = Self {
            quick_look: false,
            preview_content: None,
            preview_path: String::new(),
            preview_scroll: Scroll::new(&theme.motion),
            preview_service: None,
            preview_ticket: 0,
            preview_handler: None,
            handler_attempted: String::new(),
            tabs: paths.into_iter().map(crate::tabs::Tab::new).collect(),
            active_tab,
            cache: crate::tabs::FolderCache::default(),
            pending_paint: None,
            scroll: Scroll::new(&theme.motion),
            sidebar_scroll: Scroll::new(&theme.motion),
            surface,
            layered,
            backdrop,
            glass_active: false,
            first_frame: true,
            theme,
            scheme_override,
            interaction: Interaction::default(),
            location: None,
            loading: true,
            elapsed_ms: 0,
            status: String::new(),
            error: None,
            busy: false,
            entries: Arc::default(),
            shown: Arc::default(),
            history: History::default(),
            selection: Selection::default(),
            sort: Sort::default(),
            preferences,
            sidebar: Vec::new(),
            field: Field::None,
            address,
            search,
            editor,
            query_text: String::new(),
            indexed: false,
            geometry: Geometry::default(),
            drag: None,
            sizes: HashMap::new(),
            decorations: HashMap::new(),
            pending_images: HashSet::new(),
            image_order: VecDeque::new(),
            backend,
            watch: None,
            directory_dirty: false,
            refreshing: false,
            drop_target,
            menu: Menu::new("Files actions")?,
            menu_at: (0, 0),
            epoch: 0,
            query: 0,
            pending_history: true,
            typeahead: String::new(),
            typed_at: Instant::now(),
            closing: false,
        };
        app.rebuild_sidebar();
        app.navigate(path, true)?;
        Ok(app)
    }

    pub fn rebuild_sidebar(&mut self) {
        use windows::Win32::UI::Shell::*;
        let mut sidebar = vec![(
            "Home".into(),
            shell::known_folder(&FOLDERID_Profile).unwrap_or_else(|| "shell:profile".into()),
            Icon::Home,
        )];
        for (name, id, icon) in [
            ("Desktop", FOLDERID_Desktop, Icon::Desktop),
            ("Downloads", FOLDERID_Downloads, Icon::Download),
            ("Documents", FOLDERID_Documents, Icon::File),
            ("Pictures", FOLDERID_Pictures, Icon::Photo),
        ] {
            if let Some(path) = shell::known_folder(&id) {
                sidebar.push((name.into(), path, icon));
            }
        }
        for path in &self.preferences.pins {
            let name = Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            sidebar.push((name, path.clone(), Icon::Pin));
        }
        sidebar.push((
            "This PC".into(),
            "shell:MyComputerFolder".into(),
            Icon::Window,
        ));
        for path in shell::drives() {
            sidebar.push((
                format!("Local disk ({})", path.trim_end_matches('\\')),
                path,
                Icon::Layers,
            ));
        }
        self.sidebar = sidebar;
    }

    pub fn prepare_backdrop(&mut self, bounds: Rect) -> bool {
        use mypaper_ui::glass::{Backdrop, Shape};
        let hwnd = self.surface.hwnd();
        let material = self.theme.glass.filter(|_| {
            // SAFETY: the surface owns this HWND; IsIconic only queries its state.
            !unsafe { IsIconic(hwnd) }.as_bool()
                && (self.first_frame || self.surface.is_visible())
                && !bounds.is_empty()
                && self.backdrop.available()
        });
        let enabled = if let Some(mut material) = material {
            let origin = native::origin(hwnd);
            let shape = Shape {
                x: origin.0,
                y: origin.1,
                width: bounds.width() as i32,
                height: bounds.height() as i32,
            };
            let radius = (self.theme.radius.surface * self.surface.scale()).round();
            material.blur *= self.surface.scale();
            if self.backdrop.contains(hwnd) {
                self.backdrop.remember_overlay(hwnd);
                self.backdrop.place(hwnd, shape, radius);
                self.backdrop.material(hwnd, &material);
                true
            } else {
                self.backdrop.open(hwnd, shape, radius, &material)
            }
        } else {
            self.backdrop.close(hwnd);
            false
        };
        if self.glass_active != enabled {
            self.layered.set_enabled(enabled);
            self.glass_active = enabled;
            if !enabled {
                self.surface.release();
            }
        }
        self.first_frame = false;
        enabled
    }

    fn request_close(&mut self) {
        if self.busy {
            self.status =
                "Finish or cancel the current file operation before closing Files.".into();
        } else {
            if let Err(error) = self.save_session() {
                log::warn!("Saving Files session: {error:#}");
            }
            self.closing = true;
        }
    }

    pub fn navigate(&mut self, path: String, record: bool) -> Result<()> {
        self.quick_look = false;
        self.remember_folder();
        let started = Instant::now();
        let cached = self.cache.get(&path, self.sort);
        self.pending_paint = Some((started, cached.is_some(), path.clone()));
        self.tabs[self.active_tab].path = path.clone();
        self.drop_target
            .enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.watch = None;
        self.directory_dirty = false;
        self.refreshing = false;
        self.focus(Field::None)?;
        self.menu.close();
        self.search.set_text("")?;
        self.query_text.clear();
        self.query = 0;
        self.epoch = self.backend.folder(
            path,
            self.location.as_ref().map(|l| l.path.clone()),
            self.sort,
        );
        self.pending_history = record;
        self.loading = true;
        self.error = None;
        self.status.clear();
        self.selection.clear();
        self.scroll.reset();
        self.entries = Arc::default();
        self.replace_shown(Arc::default())?;
        self.sizes.clear();
        self.pending_images.clear();
        self.drop_target
            .destination
            .lock()
            .map(|mut d| *d = None)
            .ok();
        if let Some(folder) = cached {
            self.entries = folder.entries.clone();
            self.replace_shown(folder.entries)?;
            self.sizes = folder.sizes;
            if record {
                self.history.visit(folder.location.path.clone());
            }
            self.pending_history = false;
            self.location = Some(folder.location);
            self.refreshing = true;
        }
        Ok(())
    }

    fn remember_folder(&mut self) {
        if !self.loading
            && self.error.is_none()
            && let Some(location) = self.location.clone()
        {
            self.cache.insert(crate::tabs::Folder {
                location,
                entries: self.entries.clone(),
                sort: self.sort,
                sizes: self.sizes.clone(),
            });
        }
    }

    fn snapshot_tab(&mut self) {
        use crate::tabs::Tab;
        self.tabs[self.active_tab] = Tab {
            path: self.tabs[self.active_tab].path.clone(),
            location: self.location.clone(),
            entries: self.entries.clone(),
            shown: self.shown.clone(),
            history: self.history.clone(),
            selection: self.selection.clone(),
            sort: self.sort,
            scroll: self.scroll.clone(),
            query_text: self.query_text.clone(),
            indexed: self.indexed,
            sizes: self.sizes.clone(),
            loaded: !self.loading && self.error.is_none(),
        };
    }

    pub fn save_session(&mut self) -> Result<()> {
        self.preferences.tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.preferences.active_tab = self.active_tab;
        self.preferences.save()
    }

    pub fn switch_tab(&mut self, index: usize) -> Result<()> {
        if index == self.active_tab || index >= self.tabs.len() {
            return Ok(());
        }
        self.snapshot_tab();
        self.remember_folder();
        let tab = self.tabs[index].clone();
        self.active_tab = index;
        self.loading = true; // the previous snapshot was already cached with its own sort.
        self.sort = tab.sort;
        // A tab snapshot can be shown immediately; enumeration reconciles inactive changes.
        if tab.loaded
            && let Some(location) = tab.location.clone()
        {
            self.cache.insert(crate::tabs::Folder {
                location,
                entries: tab.entries.clone(),
                sort: tab.sort,
                sizes: tab.sizes.clone(),
            });
        }
        self.navigate(tab.path, false)?;
        self.history = tab.history;
        if tab.loaded {
            self.replace_shown(tab.shown)?;
            self.selection = tab.selection;
            self.scroll = tab.scroll;
            self.query_text = tab.query_text;
            self.search.set_text(&self.query_text)?;
            self.indexed = tab.indexed;
        } else {
            self.pending_history = true;
        }
        self.save_session()
    }

    pub fn new_tab(&mut self, path: String) -> Result<()> {
        if self.tabs.len() >= 32 {
            self.status = "The limit is 32 tabs per window".into();
            return Ok(());
        }
        let index = self.tabs.len();
        self.tabs.push(crate::tabs::Tab::new(path));
        self.switch_tab(index)
    }

    pub fn close_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Ok(());
        }
        if self.tabs.len() == 1 {
            self.request_close();
            return Ok(());
        }
        if index == self.active_tab {
            self.switch_tab(if index == 0 { 1 } else { index - 1 })?;
        }
        self.tabs.remove(index);
        if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.save_session()
    }

    fn detach_tab(&mut self, index: usize) -> Result<()> {
        use std::os::windows::process::CommandExt;
        if index >= self.tabs.len() || self.tabs.len() == 1 {
            return Ok(());
        }
        std::process::Command::new(std::env::current_exe()?)
            .args(["--", &self.tabs[index].path])
            .creation_flags(0x08000000)
            .spawn()?;
        self.close_tab(index)
    }

    /// Timestamp after the first nonempty frame is actually presented, not worker completion.
    pub fn painted(&mut self) {
        if (!self.shown.is_empty() || !self.loading)
            && let Some((started, cached, path)) = self.pending_paint.take()
        {
            log::info!(
                "first-paint: path={path:?} cached={cached} rows={} elapsed_us={}",
                self.geometry.rows.len(),
                started.elapsed().as_micros()
            );
        }
    }

    pub fn update_preview(&mut self) -> Result<()> {
        let path = (self.preferences.preview || self.quick_look)
            .then(|| {
                self.selection
                    .focus
                    .and_then(|i| self.shown.get(i))
                    .filter(|e| !e.directory)
                    .map(|e| e.path.clone())
            })
            .flatten()
            .unwrap_or_default();
        if path == self.preview_path {
            return Ok(());
        }
        self.preview_path = path.clone();
        self.preview_handler = None;
        self.handler_attempted.clear();
        self.preview_content = None;
        self.preview_scroll.reset();
        if path.is_empty() {
            self.preview_service = None;
        } else {
            if self.preview_service.is_none() {
                self.preview_service = Some(crate::preview::Service::new(self.surface.hwnd())?);
            }
            self.preview_ticket = self.preview_service.as_ref().unwrap().request(path);
        }
        Ok(())
    }

    pub fn place_preview_handler(&mut self) {
        if self.geometry.preview.is_empty() {
            self.preview_handler = None;
            return;
        }
        if !matches!(self.preview_content, Some(crate::preview::Content::Other)) {
            return;
        }
        let area = self.geometry.preview;
        let rect = windows::Win32::Foundation::RECT {
            left: area.left as i32,
            top: area.top as i32,
            right: area.right as i32,
            bottom: area.bottom as i32,
        };
        if let Some(handler) = &self.preview_handler {
            handler.place(rect);
        } else if self.handler_attempted != self.preview_path {
            self.handler_attempted = self.preview_path.clone();
            match crate::preview::ShellHandler::open(&self.preview_path, self.surface.hwnd(), rect)
            {
                Ok(handler) => self.preview_handler = Some(handler),
                Err(error) => log::debug!("Shell preview unavailable: {error:#}"),
            }
        }
    }

    fn quick_step(&mut self, forward: bool) {
        let current = self.selection.focus.unwrap_or(0);
        let next = if forward {
            (current + 1..self.shown.len()).find(|i| !self.shown[*i].directory)
        } else {
            (0..current).rev().find(|i| !self.shown[*i].directory)
        };
        if let Some(next) = next {
            self.selection.choose(next, false, false);
        }
    }

    pub fn focus(&mut self, field: Field) -> Result<()> {
        self.address.blur();
        self.search.blur();
        self.editor.blur();
        self.surface.set_text_field(None);
        native::field(field != Field::None);
        match &field {
            Field::Address => {
                self.address
                    .set_text(self.location.as_ref().map_or("", |l| &l.path))?;
                self.address.set_rect(self.geometry.address);
                self.surface.set_text_field(Some(self.address.clone()));
                self.address.request_redraw();
                self.address.focus();
                self.address.select_all();
            }
            Field::Search => {
                self.search.set_rect(self.geometry.search);
                self.surface.set_text_field(Some(self.search.clone()));
                self.search.request_redraw();
                self.search.focus();
            }
            Field::Rename(path) => {
                self.error = None;
                let name = Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.editor.set_text(&name)?;
                if !self.geometry.edit.is_empty() {
                    self.editor.set_rect(self.geometry.edit);
                }
                self.surface.set_text_field(Some(self.editor.clone()));
                self.editor.request_redraw();
                self.editor.focus();
                let directory = self
                    .selection
                    .items
                    .first()
                    .and_then(|index| self.shown.get(*index))
                    .is_some_and(|entry| entry.directory);
                let selected = if directory {
                    name.as_str()
                } else {
                    Path::new(&name)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(&name)
                };
                self.editor
                    .select_range(0..selected.encode_utf16().count() as u32);
            }
            Field::NewFolder => {
                self.error = None;
                self.editor.set_text("New folder")?;
                self.surface.set_text_field(Some(self.editor.clone()));
                self.editor.request_redraw();
                self.editor.focus();
                self.editor.select_all();
            }
            Field::None => {}
        }
        self.field = field;
        Ok(())
    }

    pub fn selected_paths(&self) -> Vec<String> {
        self.selection
            .items
            .iter()
            .filter_map(|index| self.shown.get(*index).map(|entry| entry.path.clone()))
            .collect()
    }

    /// Every row replacement reconciles all selection indexes and cancels gestures/menu hits
    /// made against the previous layout. Rename and image work already carry paths, not indexes.
    fn replace_shown(&mut self, entries: Arc<Vec<FileEntry>>) -> Result<()> {
        self.selection.remap(&self.shown, &entries);
        self.shown = entries;
        if matches!(self.drag, Some(Drag::Item { .. } | Drag::Rubber { .. })) {
            self.drag = None;
        }
        self.interaction.pressed = None;
        self.interaction.hover = None;
        self.menu.close();
        if let Field::Rename(path) = &self.field
            && !self.shown.iter().any(|entry| &entry.path == path)
        {
            self.focus(Field::None)?;
        }
        Ok(())
    }

    fn open_selection(&mut self) -> Result<()> {
        let selected = self
            .selection
            .focus
            .and_then(|index| self.shown.get(index))
            .cloned();
        if let Some(entry) = selected {
            if entry.directory {
                self.navigate(entry.path, true)?;
            } else {
                shell::open(&entry.path)?;
            }
        }
        Ok(())
    }

    pub fn refresh(&mut self) -> Result<()> {
        if let Some(location) = &self.location {
            self.epoch = self.backend.folder(location.path.clone(), None, self.sort);
            self.refreshing = true;
            self.loading = true;
            self.directory_dirty = false;
            self.error = None;
            self.pending_images.clear();
        }
        Ok(())
    }

    pub fn command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Open => self.open_selection()?,
            Command::Copy | Command::Cut => {
                transfer::copy(&self.selected_paths(), matches!(command, Command::Cut))?;
                self.status = if matches!(command, Command::Cut) {
                    "Ready to move"
                } else {
                    "Copied to clipboard"
                }
                .into();
            }
            Command::Paste => {
                if let Some(location) = self.location.as_ref().filter(|l| l.filesystem) {
                    self.operation(transfer::paste(location.path.clone())?)?;
                }
            }
            Command::Delete => {
                let paths = self.selected_paths();
                if !paths.is_empty() {
                    self.operation(Operation::Delete {
                        paths,
                        permanent: false,
                    })?;
                }
            }
            Command::Rename => {
                if self.selection.items.len() == 1
                    && let Some(path) = self.selected_paths().first()
                {
                    self.focus(Field::Rename(path.clone()))?;
                }
            }
            Command::NewFolder => {
                if self.location.as_ref().is_some_and(|l| l.filesystem) {
                    self.focus(Field::NewFolder)?;
                }
            }
            Command::Pin => {
                if let Some(location) = &self.location {
                    if self.preferences.pins.contains(&location.path) {
                        self.preferences.pins.retain(|p| p != &location.path);
                    } else {
                        self.preferences.pins.push(location.path.clone());
                    }
                    self.preferences.save()?;
                    self.rebuild_sidebar();
                }
            }
            Command::Refresh => self.refresh()?,
            Command::Terminal => {
                let folder = (self.selection.items.len() == 1)
                    .then(|| {
                        self.selection
                            .items
                            .first()
                            .and_then(|i| self.shown.get(*i))
                    })
                    .flatten()
                    .filter(|entry| entry.directory)
                    .map(|entry| entry.path.as_str())
                    .or_else(|| {
                        self.location
                            .as_ref()
                            .filter(|l| l.filesystem)
                            .map(|l| l.path.as_str())
                    });
                if let Some(folder) = folder {
                    shell::terminal(folder)?;
                }
            }
            Command::CopyPath => {
                let mut paths = self.selected_paths();
                if paths.is_empty()
                    && let Some(location) = &self.location
                {
                    paths.push(location.path.clone());
                }
                native::copy_text(self.surface.hwnd(), &paths.join("\r\n"))?;
                self.status = "Path copied".into();
            }
            Command::Preview => {
                self.preferences.preview = !self.preferences.preview;
                self.preferences.save()?;
            }
            Command::Details | Command::Icons => {
                self.preferences.view = if matches!(command, Command::Details) {
                    View::Details
                } else {
                    View::Icons
                };
                self.scroll.reset();
                self.preferences.save()?;
            }
            Command::Shell => {
                self.focus(Field::None)?;
                native::context_menu(
                    self.surface.hwnd(),
                    &self.selected_paths(),
                    self.location.as_ref().map(|l| l.path.as_str()),
                    self.menu_at,
                )?;
                self.refresh()?;
            }
        }
        Ok(())
    }

    fn operation(&mut self, operation: Operation) -> Result<()> {
        if self.busy {
            self.status = "An operation is already running".into();
            return Ok(());
        }
        self.backend.operation(operation)?;
        self.busy = true;
        self.drop_target
            .enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.drop_target
            .destination
            .lock()
            .map(|mut d| *d = None)
            .ok();
        self.status = "Working…".into();
        Ok(())
    }

    fn complete_operation(&mut self, result: std::result::Result<(), String>) -> Result<()> {
        self.busy = false;
        self.drop_target.enabled.store(
            !self.loading || self.refreshing,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.drop_target
            .destination
            .lock()
            .map(|mut d| {
                *d = self
                    .location
                    .as_ref()
                    .filter(|l| l.filesystem)
                    .map(|l| l.path.clone())
            })
            .ok();
        // A navigation in flight still owns the previous location until its first reply.
        // Defer refresh until that navigation finishes, rather than cancelling its epoch.
        if self.loading {
            self.directory_dirty = true;
        } else {
            self.refresh()?;
        }
        match result {
            Ok(()) => self.status = "Operation complete".into(),
            Err(error) => self.status = error,
        }
        Ok(())
    }

    pub fn submit(&mut self) -> Result<()> {
        match self.field.clone() {
            Field::Address => self.navigate(self.address.text()?, true)?,
            Field::Search => {
                self.run_search();
            }
            Field::Rename(path) => {
                let name = self.editor.text()?;
                shell::validate_name(&name)?;
                self.focus(Field::None)?;
                self.operation(Operation::Rename { path, name })?;
            }
            Field::NewFolder => {
                if let Some(location) = &self.location {
                    let destination = location.path.clone();
                    let name = self.editor.text()?;
                    shell::validate_name(&name)?;
                    self.focus(Field::None)?;
                    self.operation(Operation::NewFolder { destination, name })?;
                }
            }
            Field::None => self.open_selection()?,
        }
        Ok(())
    }

    fn run_search(&mut self) {
        let text = self.search.text().unwrap_or_default();
        self.query_text = text.clone();
        self.query = self
            .backend
            .search(text, self.entries.clone(), self.sort, self.sizes.clone());
        self.selection.clear();
        self.scroll.reset();
    }

    pub fn context(&mut self, ui: &mut Ui, at: (i32, i32)) -> Result<()> {
        self.focus(Field::None)?;
        self.menu_at = at;
        let mut items = Vec::new();
        if !self.selection.items.is_empty() {
            items.extend([
                Entry::action("Open", Command::Open),
                Entry::Separator,
                Entry::action("Cut", Command::Cut),
                Entry::action("Copy", Command::Copy),
                Entry::Action {
                    label: "Rename…".into(),
                    command: Command::Rename,
                    checked: None,
                    enabled: self.selection.items.len() == 1,
                },
                Entry::action("Move to Recycle Bin", Command::Delete),
                Entry::Separator,
            ]);
        }
        items.extend([
            Entry::action("Copy path", Command::CopyPath),
            Entry::Action {
                label: "Open in Terminal".into(),
                command: Command::Terminal,
                checked: None,
                enabled: self.location.as_ref().is_some_and(|l| l.filesystem),
            },
            Entry::Separator,
            Entry::Action {
                label: "Paste".into(),
                command: Command::Paste,
                checked: None,
                enabled: self.location.as_ref().is_some_and(|l| l.filesystem)
                    && clipboard_has_files()
                    && !self.busy,
            },
            Entry::Action {
                label: "New folder…".into(),
                command: Command::NewFolder,
                checked: None,
                enabled: self.location.as_ref().is_some_and(|l| l.filesystem) && !self.busy,
            },
            Entry::action(
                if self
                    .location
                    .as_ref()
                    .is_some_and(|l| self.preferences.pins.contains(&l.path))
                {
                    "Unpin this folder"
                } else {
                    "Pin this folder"
                },
                Command::Pin,
            ),
            Entry::Separator,
            Entry::radio(
                "Details",
                Command::Details,
                self.preferences.view == View::Details,
            ),
            Entry::radio(
                "Icons",
                Command::Icons,
                self.preferences.view == View::Icons,
            ),
            Entry::action("Refresh", Command::Refresh),
            Entry::radio(
                "Preview pane (Alt+P)",
                Command::Preview,
                self.preferences.preview,
            ),
            Entry::Separator,
            Entry::action("Show more options", Command::Shell),
        ]);
        self.menu
            .open(ui, items, at, screen_at(at.0, at.1), &self.theme.tokens())?;
        Ok(())
    }

    fn activate(&mut self, ui: &mut Ui, id: u32) -> Result<()> {
        match id {
            LOOK_CLOSE => self.quick_look = false,
            LOOK_PREVIOUS | LOOK_NEXT => self.quick_step(id == LOOK_NEXT),
            NEW_TAB => self.new_tab(
                self.location
                    .as_ref()
                    .map_or_else(|| "shell:profile".into(), |l| l.path.clone()),
            )?,
            PREVIEW | PREVIEW_CLOSE => self.command(Command::Preview)?,
            id if (TAB..TAB + 32).contains(&id) => self.switch_tab((id - TAB) as usize)?,
            id if (TAB_CLOSE..TAB_CLOSE + 32).contains(&id) => {
                self.close_tab((id - TAB_CLOSE) as usize)?
            }
            BACK | FORWARD => {
                if let Some(path) = self.history.go(id == FORWARD) {
                    self.navigate(path, false)?;
                }
            }
            UP => {
                if let Some(parent) = self.location.as_ref().and_then(|l| l.parent.clone()) {
                    self.navigate(parent, true)?;
                }
            }
            ADDRESS => self.focus(Field::Address)?,
            SEARCH => self.focus(Field::Search)?,
            EDIT_FIELD => self.editor.focus(),
            NEW => self.command(Command::NewFolder)?,
            CUT => self.command(Command::Cut)?,
            COPY => self.command(Command::Copy)?,
            PASTE => self.command(Command::Paste)?,
            RENAME => self.command(Command::Rename)?,
            DELETE => self.command(Command::Delete)?,
            DETAILS => self.command(Command::Details)?,
            ICONS => self.command(Command::Icons)?,
            REFRESH => self.refresh()?,
            MORE => {
                let rect = self
                    .surface
                    .shown()
                    .hits
                    .iter()
                    .find(|h| h.id == MORE)
                    .map(|h| h.rect)
                    .unwrap_or_default();
                let origin = native::origin(self.surface.hwnd());
                self.context(
                    ui,
                    (origin.0 + rect.left as i32, origin.1 + rect.bottom as i32),
                )?;
            }
            SUBMIT => self.submit()?,
            CANCEL => self.focus(Field::None)?,
            MINIMIZE => self.surface.minimize(),
            MAXIMIZE => self.surface.toggle_maximize(),
            CLOSE => self.request_close(),
            id if (HEADER..HEADER + 4).contains(&id) => {
                let column = [Column::Name, Column::Modified, Column::Type, Column::Size]
                    [(id - HEADER) as usize];
                self.sort = Sort {
                    column,
                    descending: self.sort.column == column && !self.sort.descending,
                };
                if self.query_text.is_empty() {
                    self.backend
                        .sort(self.entries.clone(), self.sort, self.sizes.clone());
                    self.query = 0;
                } else {
                    self.run_search();
                }
            }
            id if (SIDEBAR..CRUMB).contains(&id) => {
                if let Some((_, path, _)) = self.sidebar.get((id - SIDEBAR) as usize) {
                    self.navigate(path.clone(), true)?;
                }
            }
            id if (CRUMB..TAB).contains(&id) => {
                if let Some(path) = self.geometry.crumbs.get((id - CRUMB) as usize) {
                    self.navigate(path.clone(), true)?;
                }
            }
            id if id >= ITEM => {
                self.selection.choose((id - ITEM) as usize, false, false);
                self.open_selection()?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn key(&mut self, ui: &mut Ui, key: u16, modifiers: native::Modifiers) -> Result<()> {
        if self.quick_look {
            match key {
                0x1b | 0x20 => self.quick_look = false,
                0x25 | 0x26 => self.quick_step(false),
                0x27 | 0x28 => self.quick_step(true),
                _ => {}
            }
            return Ok(());
        }
        if modifiers.alt {
            match key {
                0x50 => self.command(Command::Preview)?,
                0x25 => self.activate(ui, BACK)?,
                0x27 => self.activate(ui, FORWARD)?,
                0x26 => self.activate(ui, UP)?,
                _ => {}
            }
            return Ok(());
        }
        if modifiers.control && !matches!(key, 0x21..=0x28) {
            match key {
                0x54 => self.activate(ui, NEW_TAB)?,
                0x57 => self.close_tab(self.active_tab)?,
                0x09 => {
                    let next = if modifiers.shift {
                        (self.active_tab + self.tabs.len() - 1) % self.tabs.len()
                    } else {
                        (self.active_tab + 1) % self.tabs.len()
                    };
                    self.switch_tab(next)?;
                }
                0x4c => self.focus(Field::Address)?,
                0x45 | 0x46 => {
                    self.focus(Field::Search)?;
                    self.search.select_all();
                }
                0x41 if self.field == Field::None => {
                    self.selection.items = (0..self.shown.len()).collect();
                }
                0x43 if self.field == Field::None => self.command(Command::Copy)?,
                0x58 if self.field == Field::None => self.command(Command::Cut)?,
                0x56 if self.field == Field::None => self.command(Command::Paste)?,
                0x4e if modifiers.shift => self.command(Command::NewFolder)?,
                _ => {}
            }
            return Ok(());
        }
        match key {
            0x1b => {
                self.menu.close();
                self.focus(Field::None)?;
            }
            0x0d => self.submit()?,
            0x08 if self.field == Field::None => self.activate(ui, UP)?,
            0x71 if self.field == Field::None => self.command(Command::Rename)?,
            0x74 => self.refresh()?,
            0x2e if self.field == Field::None => {
                let paths = self.selected_paths();
                if !paths.is_empty() {
                    self.operation(Operation::Delete {
                        paths,
                        permanent: modifiers.shift,
                    })?;
                }
            }
            0x5d => {
                let origin = native::origin(self.surface.hwnd());
                self.context(
                    ui,
                    (
                        origin.0 + self.geometry.viewport.left as i32 + 60,
                        origin.1 + self.geometry.viewport.top as i32 + 40,
                    ),
                )?;
            }
            0x09 => {
                self.focus(match self.field {
                    Field::None => Field::Address,
                    Field::Address => Field::Search,
                    _ => Field::None,
                })?;
            }
            0x20 if self.field == Field::None => {
                self.quick_look = self
                    .selection
                    .focus
                    .and_then(|i| self.shown.get(i))
                    .is_some_and(|e| !e.directory);
            }
            0x21..=0x28
                if !self.shown.is_empty() && matches!(self.field, Field::None | Field::Search) =>
            {
                let columns = self.geometry.grid_columns.max(1);
                let current = self.selection.focus.unwrap_or(0);
                let page = (self.geometry.viewport.height() / self.geometry.row.max(1.0)) as usize
                    * columns;
                let next = match key {
                    0x24 => 0,
                    0x23 => self.shown.len() - 1,
                    0x21 => current.saturating_sub(page),
                    0x22 => current.saturating_add(page),
                    0x25 => current.saturating_sub(1),
                    0x27 => current + 1,
                    0x26 => current.saturating_sub(columns),
                    _ => {
                        if self.selection.focus.is_none() {
                            0
                        } else {
                            current + columns
                        }
                    }
                }
                .min(self.shown.len() - 1);
                if modifiers.control {
                    self.selection.focus = Some(next);
                } else {
                    self.selection.choose(next, false, modifiers.shift);
                }
                self.interaction.focus = Some(ITEM + next as u32);
                self.interaction.focus_visible = true;
                let top = (next / columns) as f32 * self.geometry.row;
                self.scroll
                    .reveal(top, top + self.geometry.row, Instant::now());
            }
            _ => {}
        }
        Ok(())
    }

    pub fn type_select(&mut self, character: char) {
        if self.field != Field::None || self.shown.is_empty() || character == ' ' || self.quick_look
        {
            return;
        }
        if self.typed_at.elapsed() > Duration::from_millis(900) {
            self.typeahead.clear();
        }
        self.typed_at = Instant::now();
        self.typeahead.extend(character.to_lowercase());
        let cycle = self
            .typeahead
            .chars()
            .all(|c| c == character.to_lowercase().next().unwrap_or(character));
        let prefix = if cycle {
            character.to_lowercase().to_string()
        } else {
            self.typeahead.clone()
        };
        let start = if cycle {
            self.selection.focus.map_or(0, |n| n + 1)
        } else {
            0
        };
        if let Some(index) = (0..self.shown.len())
            .map(|n| (start + n) % self.shown.len())
            .find(|i| self.shown[*i].name.to_lowercase().starts_with(&prefix))
        {
            self.selection.choose(index, false, false);
            self.interaction.focus = Some(ITEM + index as u32);
            self.interaction.focus_visible = true;
            let row = (index / self.geometry.grid_columns.max(1)) as f32 * self.geometry.row;
            self.scroll
                .reveal(row, row + self.geometry.row, Instant::now());
        }
    }

    pub fn event(&mut self, ui: &mut Ui, id: SurfaceId, event: Event) -> Result<bool> {
        if self.menu.owns(id) {
            if let Some(Outcome::Chosen(command)) = self.menu.handle(ui, id, &event) {
                self.command(command)?;
            }
            return Ok(true);
        }
        if id != self.surface.id() {
            return Ok(false);
        }
        let hit = |app: &Self, at: Point| app.surface.shown().hit_at(at).map(|h| h.id);
        match event {
            Event::Close => self.request_close(),
            Event::Activated => {
                self.backdrop.restack(self.surface.hwnd());
            }
            Event::SettingChanged | Event::DpiChanged(_) => self.reload_look()?,
            Event::Key(key) => self.key(ui, key, native::Modifiers::read())?,
            Event::TextSubmit => self.submit()?,
            Event::TextChanged => {
                if self.field == Field::Search {
                    let text = self.search.text()?;
                    if text != self.query_text {
                        self.run_search();
                    }
                }
            }
            Event::Invoke(id) => self.activate(ui, id)?,
            Event::Focus(id) => {
                if id >= ITEM && ((id - ITEM) as usize) < self.shown.len() {
                    let index = (id - ITEM) as usize;
                    self.selection.focus = Some(index);
                    self.interaction.focus = Some(id);
                    self.interaction.focus_visible = true;
                    let top =
                        (index / self.geometry.grid_columns.max(1)) as f32 * self.geometry.row;
                    self.scroll
                        .reveal(top, top + self.geometry.row, Instant::now());
                } else {
                    self.activate(ui, id)?;
                }
            }
            Event::Wheel(pointer, delta) => {
                if self.quick_look || self.geometry.preview.contains(pointer.at) {
                    self.preview_scroll
                        .wheel(delta, self.surface.scale(), Instant::now());
                } else if pointer.at.x < self.geometry.viewport.left {
                    self.sidebar_scroll
                        .wheel(delta, self.surface.scale(), Instant::now());
                } else {
                    self.scroll
                        .wheel(delta, self.surface.scale(), Instant::now());
                }
            }
            Event::Press(pointer, Button::Left) => {
                self.menu.close();
                let target = hit(self, pointer.at);
                self.interaction.pressed = target;
                self.interaction.focus_visible = false;
                if let Some(index) = self
                    .geometry
                    .columns
                    .iter()
                    .position(|x| (pointer.at.x - x).abs() < 5.0 * self.surface.scale())
                    && pointer.at.y >= self.geometry.viewport.top - 40.0 * self.surface.scale()
                    && pointer.at.y < self.geometry.viewport.top
                    && self.preferences.view == View::Details
                {
                    let widths = crate::view::column_widths(self, self.geometry.viewport.width());
                    self.drag = Some(Drag::Column {
                        index,
                        start: pointer.at.x,
                        width: widths[index] / self.surface.scale(),
                    });
                } else if let Some(id) = target.filter(|id| (TAB..TAB + 32).contains(id)) {
                    // Keep the release when a tab is dragged outside this window.
                    // SAFETY: this UI thread captures the pointer for its own live surface.
                    unsafe {
                        SetCapture(self.surface.hwnd());
                    }
                    self.drag = Some(Drag::Tab {
                        index: (id - TAB) as usize,
                        start: pointer.at,
                    });
                } else if let Some(id) = target.filter(|id| *id >= ITEM) {
                    self.focus(Field::None)?;
                    let index = (id - ITEM) as usize;
                    if !self.selection.items.contains(&index) || control() || shift() {
                        self.selection.choose(index, control(), shift());
                    } else {
                        self.selection.focus = Some(index);
                    }
                    self.drag = Some(Drag::Item { start: pointer.at });
                } else if self.geometry.viewport.contains(pointer.at) {
                    self.focus(Field::None)?;
                    let initial = if control() {
                        self.selection.items.clone()
                    } else {
                        BTreeSet::new()
                    };
                    self.selection.items = initial.clone();
                    self.drag = Some(Drag::Rubber {
                        start: pointer.at,
                        now: pointer.at,
                        initial,
                    });
                } else if target != Some(SEARCH)
                    && target != Some(ADDRESS)
                    && !matches!(self.field, Field::Rename(_) | Field::NewFolder)
                {
                    self.focus(Field::None)?;
                }
            }
            Event::Move(pointer) => {
                let hover = hit(self, pointer.at);
                let changed = self.interaction.hover != hover;
                self.interaction.hover = hover;
                match self.drag.as_mut() {
                    Some(Drag::Column { .. }) => {
                        if self.preferences.widths.len() != 4 {
                            self.preferences.widths =
                                crate::view::column_widths(self, self.geometry.viewport.width())
                                    .iter()
                                    .map(|v| v / self.surface.scale())
                                    .collect();
                        }
                        if let Some(Drag::Column {
                            index,
                            start,
                            width,
                        }) = &self.drag
                        {
                            self.preferences.widths[*index] = (*width
                                + (pointer.at.x - *start) / self.surface.scale())
                            .clamp(80.0, 700.0);
                        }
                    }
                    Some(Drag::Rubber {
                        start,
                        now,
                        initial,
                    }) => {
                        *now = pointer.at;
                        let rect = Rect {
                            left: start.x.min(now.x),
                            right: start.x.max(now.x),
                            top: start.y.min(now.y),
                            bottom: start.y.max(now.y),
                        };
                        self.selection.items = initial.clone();
                        for (index, row) in &self.geometry.rows {
                            if row.intersects(&rect) {
                                self.selection.items.insert(*index);
                            }
                        }
                    }
                    Some(Drag::Item { start })
                        if (pointer.at.x - start.x).abs() + (pointer.at.y - start.y).abs()
                            > 6.0 * self.surface.scale() =>
                    {
                        self.drag = None;
                        self.interaction.pressed = None;
                        // SAFETY: the current UI thread releases the mouse capture taken by its own surface.
                        let _ = unsafe { ReleaseCapture() };
                        transfer::drag(&self.selected_paths())?;
                        self.refresh()?;
                    }
                    _ => {
                        if !changed {
                            return Ok(false);
                        }
                    }
                }
            }
            Event::Release(pointer, Button::Left) => {
                let target = hit(self, pointer.at);
                let drag = self.drag.take();
                if let Some(Drag::Tab { index, start }) = drag {
                    // SAFETY: balances this tab's pointer capture before opening another window.
                    let _ = unsafe { ReleaseCapture() };
                    if !self
                        .geometry
                        .tabs
                        .inflate(24.0 * self.surface.scale())
                        .contains(pointer.at)
                        && (pointer.at.x - start.x).abs() + (pointer.at.y - start.y).abs()
                            > 42.0 * self.surface.scale()
                    {
                        self.detach_tab(index)?;
                    } else {
                        self.switch_tab(index)?;
                    }
                } else if matches!(drag, Some(Drag::Column { .. })) {
                    self.preferences.save()?;
                } else if matches!(drag, Some(Drag::Item { .. })) && !control() && !shift() {
                    if let Some(id) = target.filter(|id| *id >= ITEM) {
                        self.selection.choose((id - ITEM) as usize, false, false);
                    }
                } else if drag.is_none()
                    && target == self.interaction.pressed
                    && let Some(id) = target
                {
                    self.activate(ui, id)?;
                }
                self.interaction.pressed = None;
            }
            Event::Release(pointer, Button::Right) => {
                if let Some(id) = hit(self, pointer.at).filter(|id| *id >= ITEM) {
                    let index = (id - ITEM) as usize;
                    if !self.selection.items.contains(&index) {
                        self.selection.choose(index, false, false);
                    }
                } else if self.geometry.viewport.contains(pointer.at) {
                    self.selection.clear();
                }
                let origin = native::origin(self.surface.hwnd());
                self.context(
                    ui,
                    (
                        origin.0 + pointer.at.x as i32,
                        origin.1 + pointer.at.y as i32,
                    ),
                )?;
            }
            Event::Release(pointer, Button::Middle) => {
                if let Some(id) = hit(self, pointer.at) {
                    if (TAB..TAB + 32).contains(&id) {
                        self.close_tab((id - TAB) as usize)?;
                    } else if id >= ITEM
                        && let Some(entry) =
                            self.shown.get((id - ITEM) as usize).filter(|e| e.directory)
                    {
                        self.new_tab(entry.path.clone())?;
                    }
                }
            }
            Event::DoubleClick(pointer, Button::Left) => {
                if let Some(id) = hit(self, pointer.at).filter(|id| *id >= ITEM) {
                    self.selection.choose((id - ITEM) as usize, false, false);
                    self.open_selection()?;
                }
            }
            Event::Leave => {
                self.interaction.hover = None;
            }
            Event::CaptureLost => {
                self.drag = None;
                self.interaction.pressed = None;
            }
            _ => {}
        }
        Ok(true)
    }

    pub fn reload_look(&mut self) -> Result<()> {
        self.theme = crate::look::load(self.scheme_override);
        self.surface.set_background(self.theme.color.surface);
        self.surface
            .set_frame(self.theme.scheme == Scheme::Dark, self.theme.color.surface);
        let field = self.field.clone();
        let text = self.search.text().unwrap_or_default();
        self.focus(Field::None)?;
        let create = || {
            TextField::new(
                self.surface.hwnd(),
                Rect::new(0.0, 0.0, 200.0, 24.0),
                false,
                self.theme.font.font(TextRole::Body).size * self.surface.scale(),
                self.theme.color.label,
            )
        };
        self.address = create()?;
        self.search = create()?;
        self.editor = create()?;
        self.search.set_text(&text)?;
        self.focus(field)?;
        self.rebuild_sidebar();
        Ok(())
    }

    pub fn replies(&mut self) -> Result<bool> {
        let mut changed = false;
        if let Some(service) = &self.preview_service {
            while let Ok((ticket, content)) = service.replies.try_recv() {
                if ticket == self.preview_ticket {
                    self.preview_content = Some(content);
                    changed = true;
                }
            }
        }
        while let Ok(reply) = self.backend.replies.try_recv() {
            changed = true;
            match reply {
                Reply::Changed { path } => {
                    self.cache.invalidate(&path);
                    if self
                        .location
                        .as_ref()
                        .is_some_and(|location| location.path.eq_ignore_ascii_case(&path))
                    {
                        self.directory_dirty = true;
                    }
                }
                Reply::Folder {
                    epoch,
                    location,
                    entries,
                    complete,
                    elapsed_ms,
                } if epoch == self.epoch => {
                    if self.refreshing && !complete {
                        continue;
                    }
                    if location.filesystem
                        && self
                            .watch
                            .as_ref()
                            .is_none_or(|(path, _)| path != &location.path)
                    {
                        self.watch = Some((
                            location.path.clone(),
                            self.backend.watch(location.path.clone())?,
                        ));
                    }
                    if self.pending_history {
                        self.history.visit(location.path.clone());
                        self.pending_history = false;
                    }
                    // SAFETY: our live HWND and a terminated HSTRING remain valid through this synchronous call.
                    let _ = unsafe {
                        SetWindowTextW(
                            self.surface.hwnd(),
                            &windows::core::HSTRING::from(format!(
                                "{} — mypaper Files",
                                location.name
                            )),
                        )
                    };
                    self.drop_target
                        .destination
                        .lock()
                        .map(|mut d| *d = location.filesystem.then(|| location.path.clone()))
                        .ok();
                    self.drop_target
                        .enabled
                        .store(!self.busy, std::sync::atomic::Ordering::Relaxed);
                    self.location = Some(location);
                    if let Some(location) = &self.location {
                        self.tabs[self.active_tab].path = location.path.clone();
                        self.tabs[self.active_tab].location = Some(location.clone());
                    }
                    self.loading = !complete;
                    if complete {
                        self.save_session()?;
                        self.refreshing = false;
                    }
                    self.elapsed_ms = elapsed_ms;
                    self.entries = entries.clone();
                    if complete {
                        self.remember_folder();
                    }
                    if self.query_text.is_empty() {
                        self.replace_shown(entries)?;
                    } else {
                        self.query = self.backend.search(
                            self.query_text.clone(),
                            self.entries.clone(),
                            self.sort,
                            self.sizes.clone(),
                        );
                    }
                    if complete {
                        log::info!("folder: {} entries in {elapsed_ms} ms", self.entries.len());
                    } else {
                        log::info!(
                            "folder first page: {} entries in {elapsed_ms} ms",
                            self.entries.len()
                        );
                    }
                }
                Reply::Search {
                    epoch,
                    query,
                    entries,
                    indexed,
                } if epoch == self.epoch && query == self.query => {
                    self.replace_shown(entries)?;
                    self.indexed = indexed;
                    if self.query_text.is_empty() {
                        self.entries = self.shown.clone();
                    }
                }
                Reply::Sizes { epoch, sizes } if epoch == self.epoch => {
                    self.sizes = sizes;
                    if self.sort.column == Column::Size {
                        if self.query_text.is_empty() {
                            self.backend
                                .sort(self.entries.clone(), self.sort, self.sizes.clone());
                        } else {
                            self.query = self.backend.search(
                                self.query_text.clone(),
                                self.entries.clone(),
                                self.sort,
                                self.sizes.clone(),
                            );
                        }
                    }
                }
                Reply::Image {
                    epoch,
                    key,
                    decoration,
                } if epoch == self.epoch => {
                    self.pending_images.remove(&key);
                    self.decorations.insert(key.clone(), decoration);
                    self.image_order.push_back(key);
                    while self.image_order.len() > 384
                        || self
                            .decorations
                            .values()
                            .filter_map(|d| d.as_ref())
                            .filter_map(|d| d.image.as_ref())
                            .map(|image| image.pixels.len())
                            .sum::<usize>()
                            > 16 * 1024 * 1024
                    {
                        if let Some(key) = self.image_order.pop_front() {
                            self.decorations.remove(&key);
                        }
                    }
                }
                Reply::Operation(result) => {
                    self.complete_operation(result)?;
                }
                Reply::Error { epoch, message } if epoch == self.epoch => {
                    self.loading = false;
                    self.error = Some(message);
                }
                _ => {}
            }
        }
        while let Ok(operation) = self.drop_target.requests.try_recv() {
            self.operation(operation)?;
            changed = true;
        }
        if self.directory_dirty && !self.loading && !self.busy {
            self.refresh()?;
            changed = true;
        }
        Ok(changed)
    }

    pub fn request_images(&mut self) {
        let origin = native::origin(self.surface.hwnd());
        let region = |rect: Rect, path: String| transfer::DropRegion {
            left: origin.0 + rect.left as i32,
            top: origin.1 + rect.top as i32,
            right: origin.0 + rect.right as i32,
            bottom: origin.1 + rect.bottom as i32,
            path,
        };
        let mut regions = self
            .geometry
            .rows
            .iter()
            .filter_map(|(index, rect)| {
                self.shown
                    .get(*index)
                    .filter(|e| e.directory && Path::new(&e.path).is_absolute())
                    .map(|e| region(*rect, e.path.clone()))
            })
            .collect::<Vec<_>>();
        for hit in &self.surface.shown().hits {
            if (SIDEBAR..CRUMB).contains(&hit.id)
                && let Some((_, path, _)) = self.sidebar.get((hit.id - SIDEBAR) as usize)
                && Path::new(path).is_absolute()
            {
                regions.push(region(hit.rect, path.clone()));
            }
        }
        if let Ok(mut targets) = self.drop_target.regions.lock() {
            *targets = regions;
        }
        let s = self.surface.scale();
        let size = ((if self.preferences.view == View::Icons {
            72.0
        } else {
            20.0
        }) * s)
            .round() as i32;
        for (index, _) in &self.geometry.rows {
            let Some(entry) = self.shown.get(*index) else {
                continue;
            };
            let key = (entry.path.clone(), size);
            if !self.decorations.contains_key(&key)
                && !self.pending_images.contains(&key)
                && self.backend.image(
                    entry.path.clone(),
                    size,
                    self.preferences.view == View::Icons,
                )
            {
                self.pending_images.insert(key);
            }
        }
    }

    pub fn tick(&mut self) -> (bool, Option<Duration>) {
        let a = self.scroll.tick(Instant::now());
        let b = self.sidebar_scroll.tick(Instant::now());
        let c = self.preview_scroll.tick(Instant::now());
        let next = match (a.1, b.1) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let next = next.into_iter().chain(c.1).min();
        (
            a.0 || b.0 || c.0,
            next.map(|at| at.saturating_duration_since(Instant::now())),
        )
    }
}

impl Drop for App {
    fn drop(&mut self) {
        use mypaper_ui::glass::Backdrop;
        self.backdrop.close(self.surface.hwnd());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_finishing_operation_does_not_cancel_a_new_navigation() {
        // SAFETY: this test thread owns its OLE apartment and releases the app before uninitializing.
        unsafe {
            windows::Win32::System::Ole::OleInitialize(None).unwrap();
        }
        let path = std::path::Path::new(r"C:\mypaper-targets\files\scratch")
            .join(format!("navigation-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let path_text = path.to_string_lossy().into_owned();
        let mut app = App::new(path_text.clone(), Some(Scheme::Dark)).unwrap();
        app.location = Some(Location {
            path: path_text,
            name: "Old folder".into(),
            parent: None,
            filesystem: true,
        });
        app.loading = false;
        app.busy = true;
        app.navigate("shell:MyComputerFolder".into(), true).unwrap();
        let navigation = app.epoch;
        app.complete_operation(Ok(())).unwrap();
        assert_eq!(
            app.epoch, navigation,
            "operation completion replaced the requested folder"
        );
        assert!(app.loading && app.directory_dirty);
        drop(app);
        // SAFETY: balances OleInitialize after the test's OLE objects have been released.
        unsafe {
            windows::Win32::System::Ole::OleUninitialize();
        }
        let _ = std::fs::remove_dir(path);
    }
}
