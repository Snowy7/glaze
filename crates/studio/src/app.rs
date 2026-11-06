//! Studio's state and behaviour: where the user is, what's focused and hovered, drags, the
//! overlays, and what every action does. Views (`view.rs`, `pages/`) read it to build each frame;
//! input from the window comes back through [`App::event`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mypaper_ui::controls::{Behavior, Input, Key, ScrollArea, Target, key};
use mypaper_ui::{Button, Event, Point, Rect};
use serde_json::{Value, json};

use crate::actions::{Action, Bar, Choices, Edit, FieldId, Page, Panel, Section, Slot, Source};
use crate::docs::{DocId, Docs, Op, Saved};
use crate::library::Library;
use crate::settings::{Kind, Place, Setting};
use crate::system::System;
use crate::theme::{Appearance, Theme};

/// Everything the views read about where the user is.
#[derive(Debug, Clone, Default)]
pub struct Nav {
    pub page: Option<Page>,
    pub section: Option<Section>,
    pub wallpaper: Option<usize>,
    /// Wallpapers filter: 0 all, 1 yours, 2 built in, 3 those with their own performance.
    pub filter: u8,
    pub look_windows: bool,
    /// Look edits the look the wallpaper brings (its theme file), not the user's.
    pub look_theme: bool,
    /// The Look stage shows the display's real layout instead of the specimen.
    pub stage_real: bool,
    pub display: usize,
    pub widget: Option<String>,
    pub bar: Option<Bar>,
    pub module: Option<Slot>,
    pub panel: Option<Panel>,
    pub integration: usize,
    pub plugins: bool,
    pub more: HashSet<Key>,
    pub scroll: HashMap<Key, f32>,
}

impl Nav {
    pub fn page(&self) -> Page {
        self.page.unwrap_or(Page::Home)
    }

    pub fn section(&self) -> Section {
        self.section.unwrap_or(Section::General)
    }

    pub fn scroll(&self, key: Key) -> f32 {
        self.scroll.get(&key).copied().unwrap_or(0.0)
    }
}

/// What one built frame said: the targets (hit id - 1), scroll views, title-area parts, the
/// regions F6 moves between, and where each setting row is (for the palette's deep links).
#[derive(Default)]
pub struct Frame {
    pub targets: Vec<Target<Action>>,
    pub scrolls: Vec<ScrollArea>,
    pub regions: Vec<Rect>,
    pub anchors: HashMap<&'static str, (Key, f32)>,
    /// Targets from here on belong to the open overlay (modal for the keyboard).
    pub overlay_from: usize,
}

/// A drag in progress.
#[derive(Debug, Clone)]
pub enum Drag {
    Slider {
        edit: Edit,
        track: Rect,
        min: f64,
        max: f64,
        step: f64,
    },
    Thumb {
        view: Key,
        track: Rect,
        content: f32,
        grab: f32,
        start: f32,
    },
    /// A widget on the canvas: moving, or resizing by its corner.
    Widget {
        instance: String,
        from: Point,
        resize: bool,
        moved: bool,
    },
    /// A module on the bars stage or a list row.
    Module { slot: Slot, from: Point, moved: bool },
}

/// The overlay over the page (one at a time).
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    PluginConsent,
    PluginUninstall(String),
    Palette { selected: usize },
    AddWallpaper { source: Option<Source>, picked: Option<String> },
    AddWidget,
    AddModule { bar: Bar },
    FirstRun { step: u8, starter: usize, taskbar: usize },
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    /// The action button's label and what it does ("Undo").
    pub action: Option<(String, Action)>,
    pub until: Instant,
    pub alert: bool,
}

/// The one live text field (windowless RichEdit) and what it edits.
pub struct Field {
    pub id: FieldId,
    pub field: mypaper_ui::TextField,
}

/// Messages from Studio's worker threads and the file watcher.
pub enum Msg {
    PluginPrepared(Result<mypaper_widgets::plugin_manager::Package, String>),
    PluginInstalled(Result<String, String>),
    Files(Vec<crate::watch::Change>),
    Image(crate::images::Loaded),
    Places(u64, Result<Vec<serde_json::Value>, String>),
    PeekEnded,
}

/// What the glass and image workers post back to wake the loop.
pub const WM_STUDIO: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 41;

pub struct App {
    /// Present only for the opt-in desktop panel; legacy Studio keeps its own navigation.
    pub panel: Option<crate::panel::State>,
    pub plugins: crate::pages::plugins::State,
    pub docs: Docs,
    pub theme: Theme,
    pub scale: f32,
    pub size: (u32, u32),
    pub nav: Nav,
    pub library: Library,
    pub system: System,
    pub registry: mypaper_widgets::registry::Registry,
    pub monitors: Vec<mypaper_desktop::Monitor>,
    pub input: Input,
    pub frame: Frame,
    pub drag: Option<Drag>,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    pub images: RefCell<crate::images::Images>,
    pub stage: RefCell<crate::stage::Cache>,
    pub glass: RefCell<crate::glass::Glass>,
    pub flash: Option<(&'static str, Instant)>,
    /// A setting the palette opened: scrolled to once its row is drawn.
    pub pending_reveal: Option<&'static str>,
    /// A drag on a value Studio writes only at the end (a wallpaper's own file).
    pub pending_value: Option<(Edit, Value)>,
    /// Where setting rows were drawn this frame (moved into `frame` after it).
    pub anchors: RefCell<HashMap<&'static str, (Key, f32)>>,
    pub field: Option<Field>,
    /// Search texts by field (palette, wallpapers, widgets, places…).
    pub texts: HashMap<FieldId, String>,
    pub places: Vec<Value>,
    pub places_query: u64,
    pub window: Option<windows::Win32::Foundation::HWND>,
    /// A select asked for its menu: the control's rect (client pixels) and its options. The
    /// window's loop opens the kit's menu there and answers with [`App::chose`].
    pub picking: Option<(Rect, Choices)>,
    pub dirty: bool,
    pub quit: bool,
    /// Goldens fix the clock and leave out live data.
    pub fixture: bool,
    pub peeking: bool,
    pub inbox: Option<std::sync::mpsc::Sender<Msg>>,
    /// The thread to wake with [`WM_STUDIO`].
    pub thread: u32,
}

impl App {
    /// Studio as it opens: the files, the wallpapers, the machine.
    pub fn new(text: &mypaper_ui::Text, scale: f32, size: (u32, u32)) -> Self {
        let mut docs = Docs::load();
        let panel = crate::panel::enabled();
        if !panel {
            migrate(&mut docs);
        }
        let appearance = Appearance::parse(
            docs.get(DocId::Studio, "/appearance")
                .and_then(Value::as_str)
                .unwrap_or("match"),
        );
        let theme = Theme::resolve(appearance, text);
        let library = Library::scan(theme_name(&docs).as_deref());
        let mut app = Self::with(docs, theme, library, scale, size, false);
        app.monitors = mypaper_desktop::monitor_infos();
        app.system = System::probe();
        if panel {
            app.panel = Some(crate::panel::State::default());
            app.nav.display = app.monitors.iter().position(|m| m.primary).unwrap_or(0);
            app.system.gpus();
        } else {
            app.restore_place();
        }
        if !panel && first_run_due(&app.docs) {
            app.overlay = Some(Overlay::FirstRun {
                step: 0,
                starter: 0,
                taskbar: 0,
            });
        }
        if panel && first_run_due(&app.docs) {
            app.panel.as_mut().unwrap().page = crate::panel::Page::FirstRun;
            app.overlay = Some(Overlay::FirstRun { step: 0, starter: 0, taskbar: 0 });
        }
        app
    }

    /// Studio with fixed files, wallpapers and monitors, for golden images.
    #[cfg(test)]
    pub fn fixture(docs: Docs, theme: Theme, library: Library, size: (u32, u32)) -> Self {
        let mut app = Self::with(docs, theme, library, 1.0, size, true);
        app.monitors = vec![
            mypaper_desktop::Monitor {
                device: "\\\\.\\DISPLAY1".into(),
                rect: mypaper_desktop::Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                work: mypaper_desktop::Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                dpi: 96,
                primary: true,
            },
            mypaper_desktop::Monitor {
                device: "\\\\.\\DISPLAY2".into(),
                rect: mypaper_desktop::Rect {
                    x: -2560,
                    y: 9,
                    width: 2560,
                    height: 1440,
                },
                work: mypaper_desktop::Rect {
                    x: -2560,
                    y: 9,
                    width: 2560,
                    height: 1440,
                },
                dpi: 96,
                primary: false,
            },
        ];
        app.system = System::fixture();
        app
    }

    fn with(docs: Docs, theme: Theme, library: Library, scale: f32, size: (u32, u32), fixture: bool) -> Self {
        Self {
            panel: None,
            plugins: Default::default(),
            docs,
            theme,
            scale,
            size,
            nav: Nav::default(),
            library,
            system: System::default(),
            registry: mypaper_widgets::registry::builtins(),
            monitors: Vec::new(),
            input: Input::default(),
            frame: Frame::default(),
            drag: None,
            overlay: None,
            toast: None,
            images: RefCell::new(crate::images::Images::default()),
            stage: RefCell::new(crate::stage::Cache::default()),
            glass: RefCell::new(crate::glass::Glass::default()),
            flash: None,
            pending_reveal: None,
            pending_value: None,
            anchors: RefCell::new(HashMap::new()),
            field: None,
            texts: HashMap::new(),
            places: Vec::new(),
            places_query: 0,
            window: None,
            picking: None,
            dirty: true,
            quit: false,
            fixture,
            peeking: false,
            inbox: None,
            thread: 0,
        }
    }

    // --- Reading ------------------------------------------------------------------------------

    pub fn get(&self, doc: DocId, path: &str) -> Option<&Value> {
        self.docs.get(doc, path)
    }

    /// A setting's current value, its default when unset.
    pub fn value(&self, s: &Setting) -> Value {
        if let Some(v) = self.get(s.doc, s.path)
            && !v.is_null()
        {
            return v.clone();
        }
        match s.kind {
            Kind::Switch { default } => json!(default),
            Kind::Choice { default, .. } | Kind::Select { default, .. } => {
                serde_json::from_str(default).unwrap_or(Value::Null)
            }
            Kind::Slider { default, .. } => json!(default),
            Kind::Text { .. } => json!(""),
            Kind::Custom => Value::Null,
        }
    }

    pub fn text(&self, id: &FieldId) -> &str {
        self.texts.get(id).map_or("", String::as_str)
    }

    pub fn px(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// The display the stages show.
    pub fn display(&self) -> Option<&mypaper_desktop::Monitor> {
        self.monitors
            .get(self.nav.display)
            .or_else(|| self.monitors.iter().find(|m| m.primary))
            .or(self.monitors.first())
    }

    pub fn is_open(&self, more: Key) -> bool {
        self.nav.more.contains(&more)
    }

    // --- Input --------------------------------------------------------------------------------

    /// The target under `p`, topmost first.
    fn target_at(&self, p: Point) -> Option<&Target<Action>> {
        self.frame
            .targets
            .iter()
            .rev()
            .find(|t| t.visible.is_some_and(|r| r.contains(p)))
    }

    fn target(&self, key: Key) -> Option<&Target<Action>> {
        self.frame.targets.iter().find(|t| t.key == key)
    }

    /// Handles one window event. Sets `dirty` when the next frame differs.
    pub fn event(&mut self, event: Event) {
        if crate::panel::event(self, &event) { return; }
        match event {
            Event::Move(p) => self.pointer_move(p.at),
            Event::Leave => {
                if self.drag.is_none() && self.input.hover.take().is_some() {
                    self.dirty = true;
                }
                self.input.pointer = None;
            }
            Event::Press(p, Button::Left) => self.press(p.at),
            Event::Release(p, Button::Left) => self.release(p.at),
            Event::DoubleClick(p, Button::Left) => {
                if let Some(Action::SelectWallpaper(i)) = self.target_at(p.at).map(|t| t.action.clone()) {
                    self.run(Action::UseWallpaper(i));
                }
            }
            Event::Press(p, Button::Right) => {
                // Right-clicking the bare wallpaper on Home: the desktop menu's settings.
                if self.nav.page() == Page::Home
                    && let Some(Action::Nav(Page::Wallpapers)) = self.target_at(p.at).map(|t| t.action.clone())
                {
                    self.go(Page::Settings);
                    self.nav.section = Some(Section::Desktop);
                }
            }
            Event::Wheel(p, delta) => self.wheel(p.at, delta),
            Event::Key(vk) => self.key(vk),
            Event::CaptureLost => {
                if self.drag.is_some() {
                    self.end_drag();
                }
            }
            Event::Invoke(id) => {
                if let Some(t) = self.frame.targets.get(id as usize - 1) {
                    let action = t.action.clone();
                    self.run(action);
                }
            }
            Event::Focus(id) => {
                if let Some(t) = self.frame.targets.get(id as usize - 1) {
                    self.input.focus = Some(t.key);
                    self.dirty = true;
                }
            }
            Event::TextChanged => self.text_changed(),
            Event::TextSubmit => self.text_submit(),
            Event::Resized(w, h) => {
                self.size = (w, h);
                self.dirty = true;
            }
            Event::SettingChanged => {
                self.retheme();
                self.monitors = mypaper_desktop::monitor_infos();
            }
            Event::Close => self.quit = true,
            _ => {}
        }
    }

    fn pointer_move(&mut self, p: Point) {
        self.input.pointer = Some(p);
        if let Some(drag) = self.drag.clone() {
            self.drag_to(drag, p);
            return;
        }
        let hover = self.target_at(p).map(|t| t.key);
        if hover != self.input.hover {
            self.input.hover = hover;
            self.dirty = true;
        }
    }

    fn press(&mut self, p: Point) {
        let Some(t) = self.target_at(p) else {
            self.blur_field();
            return;
        };
        let (key, behavior, action) = (t.key, t.behavior.clone(), t.action.clone());
        self.input.pressed = Some(key);
        self.input.focus = Some(key);
        self.input.focus_visible = false;
        self.dirty = true;
        match behavior {
            Behavior::Slider {
                track,
                min,
                max,
                step,
                ..
            } => {
                if let Action::Slider(edit) = action {
                    let drag = Drag::Slider {
                        edit,
                        track,
                        min,
                        max,
                        step,
                    };
                    self.capture(true);
                    self.drag = Some(drag.clone());
                    self.drag_to(drag, p);
                }
            }
            Behavior::Thumb {
                view,
                track,
                content,
            } => {
                self.capture(true);
                self.drag = Some(Drag::Thumb {
                    view,
                    track,
                    content,
                    grab: p.y,
                    start: self.nav.scroll(view),
                });
            }
            Behavior::Field => {
                if let Action::Field(id) = action {
                    self.focus_field(id);
                }
            }
            Behavior::Drag => self.start_drag(&action, p),
            _ => {
                if !matches!(action, Action::Field(_)) {
                    self.blur_field_soft();
                }
            }
        }
    }

    fn release(&mut self, p: Point) {
        if self.drag.is_some() {
            let clicked = matches!(
                &self.drag,
                Some(Drag::Widget { moved: false, .. } | Drag::Module { moved: false, .. })
            );
            let pressed = self.input.pressed;
            self.end_drag();
            if clicked
                && let Some(t) = pressed.and_then(|k| self.target(k))
            {
                let action = t.action.clone();
                self.run(action);
            }
            return;
        }
        let pressed = self.input.pressed.take();
        self.dirty = true;
        if let Some(t) = self.target_at(p)
            && Some(t.key) == pressed
            && !matches!(t.behavior, Behavior::Field | Behavior::Slider { .. } | Behavior::Thumb { .. })
        {
            let action = t.action.clone();
            self.run(action);
        }
    }

    fn wheel(&mut self, p: Point, delta: i16) {
        // The innermost scroll view under the pointer (views are listed outer before inner).
        let Some(area) = self
            .frame
            .scrolls
            .iter()
            .rev()
            .find(|a| a.viewport.contains(p))
            .copied()
        else {
            return;
        };
        let step = f32::from(delta) / 120.0 * self.px(56.0);
        self.scroll_to(area.key, self.nav.scroll(area.key) - step, area);
    }

    fn scroll_to(&mut self, key: Key, offset: f32, area: ScrollArea) {
        let max = (area.content - area.viewport.height()).max(0.0);
        let offset = offset.clamp(0.0, max);
        if (self.nav.scroll(key) - offset).abs() > 0.1 {
            self.nav.scroll.insert(key, offset);
            self.dirty = true;
            // The pointer now sits over something else.
            if let Some(p) = self.input.pointer {
                self.input.hover = None;
                self.pointer_move(p);
            }
        }
    }

    fn capture(&self, on: bool) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
        if let Some(hwnd) = self.window {
            // SAFETY: capture of our own window, released at the drag's end.
            unsafe {
                if on {
                    SetCapture(hwnd);
                } else {
                    let _ = ReleaseCapture();
                }
            }
        }
    }

    fn drag_to(&mut self, drag: Drag, p: Point) {
        match drag {
            Drag::Slider {
                edit,
                track,
                min,
                max,
                step,
                ..
            } => {
                let t = ((p.x - track.left) / track.width().max(1.0)).clamp(0.0, 1.0) as f64;
                let value = round_step(min + (max - min) * t, step, min);
                self.set_live(&edit, number(value));
            }
            Drag::Thumb {
                view,
                track,
                content,
                grab,
                start,
            } => {
                if let Some(area) = self.frame.scrolls.iter().find(|a| a.key == view).copied() {
                    let ratio = content / track.height().max(1.0);
                    self.scroll_to(view, start + (p.y - grab) * ratio, area);
                }
            }
            Drag::Widget { .. } | Drag::Module { .. } => {
                crate::pages::drag_to(self, drag, p);
            }
        }
    }

    fn start_drag(&mut self, action: &Action, p: Point) {
        match action {
            Action::SelectWidget(Some(instance)) => {
                let resize = matches!(self.target_at(p).map(|t| t.key), Some(k) if k == key(("resize", instance.as_str())));
                self.capture(true);
                self.drag = Some(Drag::Widget {
                    instance: instance.clone(),
                    from: p,
                    resize,
                    moved: false,
                });
            }
            Action::SelectModule(Some(slot)) => {
                self.capture(true);
                self.drag = Some(Drag::Module {
                    slot: slot.clone(),
                    from: p,
                    moved: false,
                });
            }
            other => {
                let other = other.clone();
                self.run(other);
            }
        }
    }

    fn end_drag(&mut self) {
        let drag = self.drag.take();
        let history_label = match &drag {
            Some(Drag::Slider { edit, .. }) => Some(edit.label.to_string()),
            Some(Drag::Widget { resize, .. }) => Some(if *resize { "Resize widget" } else { "Move widget" }.into()),
            Some(Drag::Module { .. }) => Some("Move module".into()),
            _ => None,
        };
        self.capture(false);
        self.input.pressed = None;
        self.dirty = true;
        match drag {
            Some(Drag::Slider { edit, .. }) => {
                if let Some((edit, value)) = self.pending_value.take() {
                    self.set(&edit, value);
                    return;
                }
                if let Err(e) = self.docs.commit_live(&edit.label) {
                    self.alert(format!("{e:#}"));
                }
                self.after_edit(&[edit.doc]);
            }
            Some(drag @ (Drag::Widget { .. } | Drag::Module { .. })) => {
                crate::pages::drop_drag(self, drag);
            }
            _ => {}
        }
        if let Some(label) = history_label { crate::panel::record_drag(self, &label); }
    }

    // --- Keyboard -----------------------------------------------------------------------------

    fn key(&mut self, vk: u16) {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
        // SAFETY: plain key-state queries for the message being handled.
        let down = |vk: i32| unsafe { GetKeyState(vk) } < 0;
        self.key_with(vk, (down(0x11), down(0x10), down(0x12)));
    }

    /// One key press with its modifiers (Ctrl, Shift, Alt).
    pub fn key_with(&mut self, vk: u16, (ctrl, shift, alt): (bool, bool, bool)) {
        self.dirty = true;
        if self.panel.is_some() && crate::panel::on_key(self, vk, (ctrl, shift, alt)) {
            return;
        }
        match (ctrl, vk) {
            (true, 0x4B) => return self.run(Action::Palette), // K
            (true, 0x5A) if !shift => return self.run(Action::Undo), // Z
            (true, 0x5A) | (true, 0x59) => return self.run(Action::Redo), // Shift+Z, Y
            (true, 0x44) if !shift => return self.run(Action::Peek), // D
            (true, 0x44) => return self.run(Action::DuplicateWidget), // Ctrl+Shift+D
            (true, 0xBC) => return self.run(Action::Nav(Page::Settings)), // ,
            (true, 0x31..=0x36) => {
                return self.run(Action::Nav(Page::ALL[usize::from(vk - 0x31)]));
            }
            (true, 0x46) => {
                // Ctrl+F: the page's search.
                let id = match self.nav.page() {
                    Page::Wallpapers => Some(FieldId::WallpaperSearch),
                    Page::Integrations => Some(FieldId::IntegrationSearch),
                    _ if self.overlay == Some(Overlay::AddWidget) => Some(FieldId::WidgetSearch),
                    _ => None,
                };
                if let Some(id) = id {
                    self.focus_field(id);
                }
                return;
            }
            _ => {}
        }
        if self.peeking {
            return;
        }
        match vk {
            0x1B => self.escape(),
            0x09 => self.tab(if shift { -1 } else { 1 }),
            0x75 => self.region(if shift { -1 } else { 1 }), // F6
            0x0D | 0x20 => self.activate(vk == 0x20),
            0x25..=0x28 => {
                if alt {
                    self.reorder(if matches!(vk, 0x26 | 0x25) { -1 } else { 1 });
                } else if !crate::pages::key(self, vk, shift) {
                    self.arrows(vk, shift);
                }
            }
            0x24 | 0x23 => self.slider_key(if vk == 0x24 { f64::MIN } else { f64::MAX }),
            _ => {
                crate::pages::key(self, vk, shift);
            }
        }
    }

    fn escape(&mut self) {
        if self.field.is_some() && !matches!(self.overlay, Some(Overlay::Palette { .. })) {
            self.blur_field();
            return;
        }
        if self.overlay.is_some() {
            self.run(Action::Dismiss);
            return;
        }
        self.run(Action::Back);
    }

    /// Targets the keyboard moves through: in the open overlay, else everywhere.
    fn focusable(&self) -> Vec<usize> {
        let modal = self.overlay.is_some();
        let first_overlay = self.frame.overlay_start();
        self.frame
            .targets
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                !matches!(t.action, Action::None | Action::Dismiss)
                    && !matches!(t.behavior, Behavior::Thumb { .. })
                    && (!modal || *i >= first_overlay)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn region_of(&self, rect: Rect) -> usize {
        let c = Point::new((rect.left + rect.right) / 2.0, (rect.top + rect.bottom) / 2.0);
        self.frame
            .regions
            .iter()
            .position(|r| r.contains(c))
            .unwrap_or(usize::MAX)
    }

    fn tab(&mut self, delta: i32) {
        let list = self.focusable();
        if list.is_empty() {
            return;
        }
        let at = self
            .input
            .focus
            .and_then(|k| list.iter().position(|&i| self.frame.targets[i].key == k));
        let next = match at {
            Some(i) => list[(i as i32 + delta).rem_euclid(list.len() as i32) as usize],
            None if delta > 0 => list[0],
            None => list[list.len() - 1],
        };
        self.focus_target(next);
    }

    fn region(&mut self, delta: i32) {
        let list = self.focusable();
        if list.is_empty() || self.frame.regions.is_empty() {
            return;
        }
        let current = self
            .input
            .focus
            .and_then(|k| self.target(k))
            .map_or(usize::MAX, |t| self.region_of(t.rect));
        let count = self.frame.regions.len() as i32;
        for step in 1..=count {
            let want = if current == usize::MAX {
                if delta > 0 { step - 1 } else { count - step }
            } else {
                (current as i32 + delta * step).rem_euclid(count)
            } as usize;
            if let Some(&i) = list
                .iter()
                .find(|&&i| self.region_of(self.frame.targets[i].rect) == want)
            {
                self.focus_target(i);
                return;
            }
        }
    }

    /// Focuses target `i` from the keyboard, scrolling it into view.
    fn focus_target(&mut self, i: usize) {
        let t = &self.frame.targets[i];
        let (key, rect, scroll) = (t.key, t.rect, t.scroll);
        self.input.focus = Some(key);
        self.input.focus_visible = true;
        if let Some(view) = scroll
            && let Some(area) = self.frame.scrolls.iter().find(|a| a.key == view).copied()
        {
            let offset = self.nav.scroll(view);
            let pad = self.px(16.0);
            if rect.top < area.viewport.top {
                self.scroll_to(view, offset - (area.viewport.top - rect.top) - pad, area);
            } else if rect.bottom > area.viewport.bottom {
                self.scroll_to(view, offset + (rect.bottom - area.viewport.bottom) + pad, area);
            }
        }
        self.dirty = true;
    }

    fn activate(&mut self, space: bool) {
        let Some(t) = self.input.focus.and_then(|k| self.target(k)) else {
            return;
        };
        if space && matches!(t.behavior, Behavior::Slider { .. }) {
            return;
        }
        let action = t.action.clone();
        match action {
            Action::Field(id) => self.focus_field(id),
            // A switch among the palette's results flips in place; Enter still goes there.
            Action::PaletteRun(i) if space && crate::search::toggle(self, i) => {}
            action => self.run(action),
        }
    }

    /// Arrows: within a group (segments, tiles, rows) move to the neighbour, and pick it when
    /// the group is a choice; on a slider, step it.
    fn arrows(&mut self, vk: u16, shift: bool) {
        let Some(t) = self.input.focus.and_then(|k| self.target(k)) else {
            return;
        };
        if let Behavior::Slider { .. } = t.behavior {
            let up = matches!(vk, 0x26 | 0x27);
            self.slider_step(if up { 1.0 } else { -1.0 } * if shift { 10.0 } else { 1.0 });
            return;
        }
        let Some(group) = t.group else { return };
        let delta = if matches!(vk, 0x25 | 0x26) { -1 } else { 1 };
        let members: Vec<usize> = self
            .frame
            .targets
            .iter()
            .enumerate()
            .filter(|(_, m)| m.group == Some(group))
            .map(|(i, _)| i)
            .collect();
        let Some(at) = members.iter().position(|&i| self.frame.targets[i].key == t.key) else {
            return;
        };
        let next = members[(at as i32 + delta).clamp(0, members.len() as i32 - 1) as usize];
        self.focus_target(next);
        let choice = matches!(
            self.frame.targets[next].action,
            Action::Set(..) | Action::SetStyle(_) | Action::WindowsLook(_) | Action::Preset(_)
        );
        if choice {
            let action = self.frame.targets[next].action.clone();
            self.run(action);
        }
    }

    fn slider_step(&mut self, steps: f64) {
        let Some(t) = self.input.focus.and_then(|k| self.target(k)) else {
            return;
        };
        if let (
            Behavior::Slider {
                min,
                max,
                step,
                value,
                ..
            },
            Action::Slider(edit),
        ) = (&t.behavior, &t.action)
        {
            let v = round_step((value + step * steps).clamp(*min, *max), *step, *min);
            let edit = edit.clone();
            self.set(&edit, number(v));
        }
    }

    fn slider_key(&mut self, to: f64) {
        if let Some(t) = self.input.focus.and_then(|k| self.target(k))
            && let (Behavior::Slider { min, max, .. }, Action::Slider(edit)) = (&t.behavior, &t.action)
        {
            let v = to.clamp(*min, *max);
            let edit = edit.clone();
            self.set(&edit, number(v));
        }
    }

    /// Alt+arrows: move the focused row of a sortable list.
    fn reorder(&mut self, delta: i32) {
        let Some(action) = self.input.focus.and_then(|k| self.target(k)).map(|t| t.action.clone()) else {
            return;
        };
        match action {
            Action::SelectModule(Some(slot)) => self.run(Action::MoveModule(slot, delta)),
            Action::PinRemove(which, i) => self.run(Action::PinMove(which, i, delta)),
            Action::MenuRemove(i) => self.run(Action::MenuMove(i, delta)),
            _ => {}
        }
    }

    // --- Text fields --------------------------------------------------------------------------

    /// Focuses a text field, creating its RichEdit (the one live field).
    pub fn focus_field(&mut self, id: FieldId) {
        if self.field.as_ref().is_some_and(|f| f.id == id) {
            return;
        }
        self.blur_field();
        let Some(hwnd) = self.window else {
            self.input.focus = Some(field_key(&id));
            return;
        };
        let initial = self.field_initial(&id);
        let size = self.theme.ty.style(mypaper_ui::controls::Font::Body, self.scale).size
            * if self.panel.is_some() { 13.0 / 14.0 } else { 1.0 };
        let color = self.theme.palette.ink1;
        match mypaper_ui::TextField::new(hwnd, Rect::new(-1000.0, -1000.0, 10.0, 10.0), false, size, color) {
            Ok(field) => {
                let _ = field.set_text(&initial);
                field.focus();
                self.field = Some(Field { id: id.clone(), field });
                self.input.focus = Some(field_key(&id));
                self.dirty = true;
            }
            Err(e) => log::warn!("text field: {e:#}"),
        }
    }

    fn field_initial(&self, id: &FieldId) -> String {
        if let Some(value) = crate::panel::field_initial(self, id) { return value; }
        if let FieldId::Text(DocId::Studio, path) = id
            && let Some(rest) = path.strip_prefix("/plugins/setting/")
            && let Some((plugin, key)) = rest.split_once('/') {
            let key = key.replace("~1", "/").replace("~0", "~");
            let schema = self.plugins.installed.iter().find(|m| m.id == plugin).and_then(|m| m.settings.iter().find(|s| s["key"].as_str() == Some(&key)));
            let value = self.plugins.preferences.settings.get(plugin).and_then(|v| v.get(&key)).or_else(|| schema.map(|s| &s["default"]));
            return value.map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        }
        match id {
            FieldId::Text(doc, path) => self
                .get(*doc, path)
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default(),
            FieldId::Hex(path) => self
                .get(DocId::Config, path)
                .and_then(Value::as_str)
                .filter(|s| s.starts_with('#'))
                .unwrap_or("")
                .to_owned(),
            other => self.text(other).to_owned(),
        }
    }

    /// Ends editing: setting fields commit what was typed.
    pub fn blur_field(&mut self) {
        if let Some(f) = self.field.take() {
            let text = f.field.text().unwrap_or_default();
            f.field.blur();
            self.commit_field(&f.id, text, false);
            self.dirty = true;
        }
    }

    /// Ends editing only for fields that live in the page (a click elsewhere).
    fn blur_field_soft(&mut self) {
        if self
            .field
            .as_ref()
            .is_some_and(|f| !matches!(f.id, FieldId::Palette))
        {
            self.blur_field();
        }
    }

    fn text_changed(&mut self) {
        let Some(f) = &self.field else { return };
        let text = f.field.text().unwrap_or_default();
        let id = f.id.clone();
        if self.texts.get(&id) != Some(&text) {
            if self.panel.is_some() && matches!(id, FieldId::Palette) {
                self.panel.as_mut().unwrap().page = crate::panel::Page::Search;
            } else if matches!(id, FieldId::Palette) {
                self.overlay = Some(Overlay::Palette { selected: 0 });
            }
            if matches!(id, FieldId::PlaceSearch) {
                self.search_places(&text);
            }
            self.texts.insert(id, text);
        }
        self.dirty = true;
    }

    fn text_submit(&mut self) {
        let Some(f) = &self.field else { return };
        let text = f.field.text().unwrap_or_default();
        let id = f.id.clone();
        match id {
            FieldId::Palette => {
                if let Some(Overlay::Palette { selected }) = self.overlay {
                    self.run(Action::PaletteRun(selected));
                }
            }
            FieldId::AddUrl => self.run(Action::AddCommit(true)),
            _ => {
                self.commit_field(&id, text, true);
                self.blur_field();
            }
        }
    }

    fn commit_field(&mut self, id: &FieldId, text: String, _submitted: bool) {
        match id {
            FieldId::Text(doc, path) => {
                let current = self.field_initial(id);
                if text != current {
                    let value = if text.trim().is_empty() {
                        None
                    } else {
                        Some(Value::String(text.trim().to_owned()))
                    };
                    let label = Setting::get_by_path(*doc, path).map_or("Text", |s| s.label);
                    self.set(&Edit::new(*doc, path.clone(), label), value.unwrap_or(Value::Null));
                }
            }
            FieldId::Hex(path) => {
                // "#ff8800" or "ff8800".
                let hex = format!("#{}", text.trim().trim_start_matches('#'));
                if let Some(color) = mypaper_config::Color::parse(&hex) {
                    self.apply("Colour", vec![Op {
                        doc: DocId::Config,
                        path: path.to_string(),
                        value: Some(json!(color.hex())),
                    }]);
                }
            }
            other => {
                self.texts.insert(other.clone(), text);
            }
        }
    }

    fn search_places(&mut self, query: &str) {
        self.places_query += 1;
        let (Some(inbox), id, query) = (self.inbox.clone(), self.places_query, query.trim().to_owned()) else {
            return;
        };
        if query.len() < 2 {
            self.places.clear();
            return;
        }
        let thread = self.thread;
        std::thread::spawn(move || {
            use mypaper_plugin_weather::service::Source;
            let found = mypaper_plugin_weather::client::OpenMeteo::new()
                .search(&query)
                .map_err(|e| format!("{e:?}"))
                .and_then(|list| serde_json::to_value(list).map_err(|e| e.to_string()))
                .map(|v| v.as_array().cloned().unwrap_or_default());
            let _ = inbox.send(Msg::Places(id, found));
            crate::wake(thread);
        });
    }

    // --- Edits --------------------------------------------------------------------------------

    /// Sets a value as one undoable step (a click, a key).
    pub fn set(&mut self, edit: &Edit, value: Value) {
        if crate::panel::intercept_set(self, edit, &value) { return; }
        let value = (!value.is_null()).then_some(value);
        if edit.doc == DocId::Studio && crate::pages::intercept(self, &edit.path, value.clone()) {
            self.dirty = true;
            return;
        }
        self.apply(&edit.label, vec![Op {
            doc: edit.doc,
            path: edit.path.to_string(),
            value,
        }]);
    }

    /// Sets a value during a drag (written once, at the end).
    fn set_live(&mut self, edit: &Edit, value: Value) {
        if edit.doc == DocId::Studio {
            // A wallpaper's own file: written when the drag ends.
            self.pending_value = Some((edit.clone(), value));
            self.dirty = true;
            return;
        }
        let mut ops = vec![Op {
            doc: edit.doc,
            path: edit.path.to_string(),
            value: Some(value),
        }];
        if edit.doc == DocId::Config && edit.path.starts_with("/look") {
            ops.extend(crate::look::projection_after(&self.docs, &ops));
        }
        match self.docs.live(&ops) {
            Ok(()) => self.dirty = true,
            Err(e) => self.alert(format!("{e:#}")),
        }
    }

    /// Applies edits as one step, keeping the shell's look files in step with the user's
    /// look (`look.rs`).
    pub fn apply(&mut self, label: &str, mut ops: Vec<Op>) {
        if ops
            .iter()
            .any(|op| op.doc == DocId::Config && op.path.starts_with("/look"))
        {
            let projected = crate::look::projection_after(&self.docs, &ops);
            ops.extend(projected);
        }
        let docs: Vec<DocId> = ops.iter().map(|o| o.doc).collect();
        let checkpoint = self.docs.undo_count();
        match self.docs.edit(label, &ops) {
            Ok(()) => {
                self.after_edit(&docs);
                crate::panel::record_edit(self, label, &ops, checkpoint);
            }
            Err(e) => {
                crate::panel::save_failed(self, docs.first().copied(), &format!("{e:#}"));
                if self.panel.is_none() { self.alert(format!("{e:#}")); }
            }
        }
        self.dirty = true;
    }

    /// What follows an edit: Studio's own look, the wallpapers' active state.
    fn after_edit(&mut self, docs: &[DocId]) {
        if docs.contains(&DocId::Studio) {
            self.retheme();
        }
        if docs.contains(&DocId::Config) {
            self.library.set_active(theme_name(&self.docs).as_deref());
        }
    }

    /// Re-reads Windows' appearance and Studio's own choice.
    pub fn retheme(&mut self) {
        // The text measure needs no Ui: Type::new only reads families, which stay the same.
        let appearance = Appearance::parse(
            self.get(DocId::Studio, "/appearance")
                .and_then(Value::as_str)
                .unwrap_or("match"),
        );
        let ty = self.theme.ty.clone();
        let mut theme = crate::theme::Theme::resolve_with(appearance, ty);
        if self.fixture {
            theme = self.theme.clone();
        }
        if theme != self.theme {
            self.theme = theme;
            self.dirty = true;
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, action: Option<(String, Action)>) {
        self.toast = Some(Toast {
            text: text.into(),
            action,
            until: Instant::now() + Duration::from_secs(6),
            alert: false,
        });
        self.dirty = true;
    }

    pub fn alert(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            action: None,
            until: Instant::now() + Duration::from_secs(8),
            alert: true,
        });
        self.dirty = true;
    }

    /// After an undoable edit: a toast offering Undo.
    pub fn toast_undo(&mut self, text: impl Into<String>) {
        self.toast(text, Some(("Undo".into(), Action::Undo)));
    }

    // --- Navigation ---------------------------------------------------------------------------

    pub fn go(&mut self, page: Page) {
        self.blur_field_soft();
        if self.nav.page() != page {
            self.nav.page = Some(page);
            self.input.focus = None;
            self.remember_place();
        }
        self.dirty = true;
    }

    /// Opens a setting's place, its More, scrolls to it and flashes it (the palette's Enter).
    pub fn reveal(&mut self, s: &'static Setting) {
        match s.place {
            Place::Page(page) => self.go(page),
            Place::Look { windows } => {
                self.go(Page::Look);
                self.nav.look_windows = windows;
            }
            Place::Bars(bar) => {
                self.go(Page::Bars);
                self.nav.bar = bar;
                self.nav.module = None;
                self.nav.panel = None;
            }
            Place::BarsPanel(panel) => {
                self.go(Page::Bars);
                self.nav.bar = None;
                self.nav.panel = Some(panel);
            }
            Place::Settings(section) => {
                self.go(Page::Settings);
                self.nav.section = Some(section);
            }
        }
        if let Some(group) = s.more {
            self.nav.more.insert(more_key(s.place, group));
        }
        self.flash = Some((s.id, Instant::now()));
        self.pending_reveal = Some(s.id);
        self.dirty = true;
    }

    /// After a frame, scroll a revealed setting into view (its row's place is known now).
    pub fn settle_reveal(&mut self) {
        let Some(id) = self.pending_reveal.take() else { return };
        if let Some(&(view, y)) = self.frame.anchors.get(id)
            && let Some(area) = self.frame.scrolls.iter().find(|a| a.key == view).copied()
        {
            let offset = self.nav.scroll(view);
            let target = offset + (y - area.viewport.top) - area.viewport.height() / 3.0;
            self.scroll_to(view, target, area);
        }
    }

    fn remember_place(&mut self) {
        if self.fixture || self.panel.is_some() {
            return;
        }
        let page = self.nav.page().key();
        if self.get(DocId::Studio, "/page").and_then(Value::as_str) != Some(page) {
            self.apply("Page", vec![Op {
                doc: DocId::Studio,
                path: "/page".into(),
                value: Some(json!(page)),
            }]);
        }
    }

    fn restore_place(&mut self) {
        self.nav.page = self
            .get(DocId::Studio, "/page")
            .and_then(Value::as_str)
            .and_then(Page::from_key);
    }

    /// When the loop must wake next (a toast or flash ending), if any.
    pub fn deadline(&self) -> Option<Instant> {
        let flash = self.flash.map(|(_, at)| at + Duration::from_millis(900));
        let toast = self.toast.as_ref().map(|t| t.until);
        let saved = match self.docs.saved {
            Saved::Done(at) => Some(at + Duration::from_secs(2)),
            _ => None,
        };
        let status = self.system.next_poll(self.watching_status());
        [flash, toast, saved, status].into_iter().flatten().min()
    }

    /// Live numbers only while someone can see them: Studio in front, on a page showing them.
    pub fn watching_status(&self) -> bool {
        !self.fixture
            && !self.peeking
            && self.window.is_some_and(|w| {
                // SAFETY: a read-only query.
                (unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }) == w
            })
            && (matches!(self.nav.page(), Page::Home | Page::Settings) || self.nav.page() == Page::Integrations && self.nav.plugins)
    }

    /// Timers: expire the toast and flash, refresh live status.
    pub fn tick(&mut self) {
        let now = Instant::now();
        if self.toast.as_ref().is_some_and(|t| t.until <= now) {
            self.toast = None;
            self.dirty = true;
        }
        if self.flash.is_some_and(|(_, at)| at + Duration::from_millis(900) <= now) {
            self.flash = None;
            self.dirty = true;
        }
        if let Saved::Done(at) = self.docs.saved
            && at + Duration::from_secs(2) <= now
        {
            self.docs.saved = Saved::Idle;
            self.dirty = true;
        }
        if self.watching_status() && self.system.poll_due(now) {
            self.system.refresh();
            if self.nav.page() == Page::Integrations && self.nav.plugins { self.plugins.refresh(); }
            self.dirty = true;
        }
    }

    /// A message from a worker or the watcher.
    pub fn message(&mut self, msg: Msg) {
        match msg {
            Msg::PluginPrepared(result) => {
                self.plugins.busy = None;
                match result {
                    Ok(package) => { self.plugins.pending = Some(package); self.overlay = Some(Overlay::PluginConsent); }
                    Err(error) => self.alert(error),
                }
                self.dirty = true;
            }
            Msg::PluginInstalled(result) => {
                self.plugins.busy = None;
                match result {
                    Ok(id) => { let _ = mypaper_widgets::plugin_manager::send(&mypaper_widgets::plugin_manager::Request::Enable(id.clone())); self.plugins.refresh(); self.plugins.selected = Some(id); self.toast("Plugin installed", None); }
                    Err(error) => self.alert(error),
                }
                self.dirty = true;
            }
            Msg::Files(changes) => {
                for change in changes {
                    match change {
                        crate::watch::Change::Doc(id) => {
                            if self.docs.reload(id) {
                                self.after_edit(&[id]);
                            }
                        }
                        crate::watch::Change::Themes => {
                            self.library = Library::scan(theme_name(&self.docs).as_deref());
                        }
                    }
                }
                self.dirty = true;
            }
            Msg::Image(loaded) => {
                self.images.borrow_mut().arrived(loaded);
                self.dirty = true;
            }
            Msg::Places(id, found) => {
                if id == self.places_query {
                    self.places = found.unwrap_or_default();
                    self.dirty = true;
                }
            }
            Msg::PeekEnded => {
                crate::peek::end(self);
            }
        }
    }

    // --- Actions ------------------------------------------------------------------------------

    pub fn run(&mut self, action: Action) {
        crate::panel::before_action(self, &action);
        self.dirty = true;
        match action {
            Action::None => {}
            Action::Minimize | Action::Maximize | Action::Close => {
                // Handled by the window (caption buttons go through WM_NCHITTEST); from the
                // keyboard or assistive technology they come here.
                self.window_command(action);
            }
            Action::Nav(page) => self.go(page),
            Action::Section(section) => {
                self.go(Page::Settings);
                self.nav.section = Some(section);
                self.input.focus = None;
                if section == Section::Performance && !self.fixture {
                    self.system.gpus();
                }
            }
            Action::Panel(command) => crate::panel::run(self, command),
            Action::Back if self.panel.is_some() => crate::panel::run(self, crate::panel::Command::Back),
            Action::Back => crate::pages::back(self),
            Action::Palette => {
                self.texts.remove(&FieldId::Palette);
                self.overlay = Some(Overlay::Palette { selected: 0 });
                self.focus_field(FieldId::Palette);
            }
            Action::Undo => match self.docs.undo() {
                Ok(Some(label)) => {
                    self.after_edit(&DocId::ALL);
                    self.toast(format!("Undid {}", label.to_lowercase()), Some(("Redo".into(), Action::Redo)));
                }
                Ok(None) => {}
                Err(e) => self.alert(format!("{e:#}")),
            },
            Action::Redo => match self.docs.redo() {
                Ok(Some(label)) => {
                    self.after_edit(&DocId::ALL);
                    self.toast(format!("Redid {}", label.to_lowercase()), Some(("Undo".into(), Action::Undo)));
                }
                Ok(None) => {}
                Err(e) => self.alert(format!("{e:#}")),
            },
            Action::Peek => crate::peek::start(self),
            Action::Pause => {
                let paused = self.get(DocId::Config, "/paused").and_then(Value::as_bool).unwrap_or(false);
                self.set(&Edit::new(DocId::Config, "/paused", if paused { "Resume" } else { "Pause" }), json!(!paused));
            }
            Action::StartMypaper => match crate::system::start() {
                Ok(()) => self.toast("Starting mypaper", None),
                Err(e) => self.alert(format!("{e:#}")),
            },
            Action::StopMypaper => match crate::system::stop() {
                Ok(()) => self.toast("mypaper is stopped. Windows is back as it was.", None),
                Err(e) => self.alert(format!("{e:#}")),
            },
            Action::Restart(part) => match crate::system::restart(part) {
                Ok(()) => self.toast(format!("Restarting the {part}"), None),
                Err(e) => self.alert(format!("{e:#}")),
            },
            Action::RestoreDesktop => crate::system::restore_desktop(),
            Action::CopyDiagnostics => {
                let text = crate::system::diagnostics(self);
                crate::system::copy_text(&text);
                self.toast("Copied diagnostics", None);
            }
            Action::OpenLogs => crate::system::open_logs(),
            Action::More(key) => {
                if !self.nav.more.remove(&key) {
                    self.nav.more.insert(key);
                }
            }
            Action::Set(edit, value) => self.set(&edit, value),
            Action::Toggle(edit, on) => self.set(&edit, json!(!on)),
            Action::Slider(_) => {}
            Action::Unset(edit) => self.apply(&edit.label, vec![Op {
                doc: edit.doc,
                path: edit.path.to_string(),
                value: None,
            }]),
            Action::Choose(choices) => self.choose(choices),
            Action::Field(id) => self.focus_field(id),
            Action::Dismiss => {
                if matches!(self.overlay, Some(Overlay::PluginConsent)) { self.plugins.pending = None; }
                if matches!(self.overlay, Some(Overlay::FirstRun { .. })) {
                    self.run(Action::FirstRunSkip);
                    return;
                }
                if matches!(self.overlay, Some(Overlay::Palette { .. })) {
                    self.blur_field();
                }
                self.overlay = None;
                self.input.focus = None;
            }
            Action::PaletteRun(i) => crate::search::run(self, i),
            Action::ToastButton => {
                if let Some((_, action)) = self.toast.take().and_then(|t| t.action) {
                    self.run(action);
                }
            }
            Action::Open(target) if target == "mypaper-custom:new" => crate::pages::widgets::new_custom(self),
            Action::Open(target) => crate::system::open(&target),
            Action::Reveal(path) => crate::system::reveal(&path),
            other => crate::pages::run(self, other),
        }
        crate::panel::after_action(self);
    }

    fn window_command(&mut self, action: Action) {
        use windows::Win32::UI::WindowsAndMessaging::{
            IsZoomed, PostMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, WM_SYSCOMMAND,
        };
        let Some(hwnd) = self.window else { return };
        // SAFETY: standard commands to our own window.
        unsafe {
            let command = match action {
                Action::Minimize => SC_MINIMIZE,
                Action::Maximize if IsZoomed(hwnd).as_bool() => SC_RESTORE,
                Action::Maximize => SC_MAXIMIZE,
                _ => {
                    self.quit = true;
                    return;
                }
            };
            let _ = PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                windows::Win32::Foundation::WPARAM(command as usize),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }

    /// A select's menu: asks the window for the kit's menu under the control (`picking`).
    fn choose(&mut self, choices: Choices) {
        let Some(rect) = self.input.focus.and_then(|k| self.target(k)).map(|t| t.rect) else {
            return;
        };
        self.picking = Some((rect, choices));
    }

    /// The select's menu chose option `i` of `choices`.
    pub fn chose(&mut self, choices: &Choices, i: usize) {
        if let Some((value, _)) = choices.options.get(i) {
            self.set(&choices.edit, value.clone());
        }
    }
}

impl Frame {
    /// The index of the first overlay target (overlays are built last).
    pub fn overlay_start(&self) -> usize {
        self.overlay_from
    }
}

/// The key a text field's frame uses (focus survives the field coming and going).
pub fn field_key(id: &FieldId) -> Key {
    key(("field", format!("{id:?}")))
}

/// The key of a More disclosure: its place and name.
pub fn more_key(place: Place, group: &str) -> Key {
    key(("more", format!("{place:?}"), group))
}

/// A slider's value as JSON: whole numbers as integers, since some fields only take those
/// (a preview delay in ms); fractional fields read integers too.
pub fn number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

fn round_step(v: f64, step: f64, min: f64) -> f64 {
    if step <= 0.0 {
        return v;
    }
    let r = ((v - min) / step).round() * step + min;
    // Keep two decimals' worth of noise out of the file.
    (r * 1000.0).round() / 1000.0
}

/// The config's wallpaper (theme) name.
pub fn theme_name(docs: &Docs) -> Option<String> {
    docs.get(DocId::Config, "/theme")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// D2's migration: adopt an active user theme's Windows styling into the user's look, once.
fn migrate(docs: &mut Docs) {
    if docs.error(DocId::Config).is_some() || docs.is_set(DocId::Config, "/look/windows") {
        return;
    }
    let Ok(mut config) = serde_json::from_value::<mypaper_config::Config>(docs.doc(DocId::Config).effective.clone()) else {
        return;
    };
    if mypaper_theme::migrate_windows(&mut config)
        && let Ok(layer) = serde_json::to_value(&config.look.windows)
    {
        let _ = docs.edit("Your Windows styling", &[Op {
            doc: DocId::Config,
            path: "/look/windows".into(),
            value: Some(layer),
        }]);
        log::info!("Windows styling moved into your look (D2)");
    }
}

/// First run: no settings of any kind yet, and Studio hasn't finished it.
fn first_run_due(docs: &Docs) -> bool {
    let done = docs.get(DocId::Studio, "/first_run_done").and_then(Value::as_bool) == Some(true);
    let fresh = DocId::ALL[..3]
        .iter()
        .all(|id| docs.doc(*id).path.as_ref().is_none_or(|p| !p.exists()));
    !done && fresh
}

impl Setting {
    /// The table entry editing `path` of `doc`, if one does.
    pub fn get_by_path(doc: DocId, path: &str) -> Option<&'static Setting> {
        crate::settings::SETTINGS
            .iter()
            .find(|s| s.doc == doc && s.path == path)
    }
}

