//! Native desktop widgets and edit mode. One cropped DesktopChild surface per visible instance,
//! one deadline queue, and sleeping service workers. No WebView, polling loop or ambient motion.
mod control;
mod data;
mod edit;
pub(crate) mod model;
#[cfg(test)]
mod tests;
pub(crate) mod time;
pub(crate) mod view;

use crate::glass::{self, Material, Plate};
use crate::inbox;
use crate::integrations::media::WidgetMedia as Media;
use anyhow::Result;
use base64::Engine;
use model::{Model, Task, Timer, Widget};
use mypaper_config::Config;
use mypaper_desktop::hold::{Fact, Holds, Reason};
use mypaper_desktop::{
    ConfigWatcher, DesktopEvents, DesktopHost, Monitor, WindowEvents, covered_monitors,
    monitor_infos, watch_file,
};
use mypaper_theme::{Color, Paint, TextTone};
use mypaper_ui::{
    Button, Colors, Event, HitId, Kind, Rect, Rgba, Role, Surface, SurfaceId, TextField, TextStyle,
    Ticker, Tokens, Ui,
};
use mypaper_widgets::{
    appicon::Icons,
    audio::{Audio, Wasapi},
    bridge::{MediaAction, Out, Target},
    display::{DisplayWatcher, WM_DISPLAY},
    files,
    layout::{self, Grid, Layout, RectF},
    look,
    plugins::Plugins,
    registry::{self, Registry},
    stats::{Delivery, Stats},
    storage::{self, Storage},
    wake::{Msg, Waker},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use time::Stamp;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_APP, WM_TIMECHANGE};
use windows::core::{PCWSTR, w};

pub const WM_EDIT: u32 = WM_APP + 100;
pub const WM_OVERLAY: u32 = WM_APP + 101;
pub const WM_LAYOUT: u32 = WM_APP + 102;
pub const WM_DATA: u32 = WM_APP + 103;

struct Entry {
    surface: Surface,
    model: Model,
    monitor: usize,
    rect: Rect,
    tokens: Tokens,
    dpi_scale: f32,
}
struct Drag {
    id: String,
    start: (i32, i32),
    original: Rect,
    pending: Rect,
    resize: bool,
}
struct EditSurface {
    surface: Surface,
    monitor: usize,
}

/// Read-only diagnostics used by the locked live harness.
#[allow(dead_code, reason = "live harness diagnostics")]
#[derive(Debug)]
pub struct Probe {
    pub instance: String,
    pub hwnd: HWND,
    pub rect: Rect,
    pub visible: bool,
    pub dpi: u32,
    pub ordered: bool,
    pub editing_text: bool,
}

/// Owned by the shell's UI thread. Call `poll` after dispatch, route queued UI events with
/// `on_event`, and include `timeout_ms` in the message wait. All methods are non-reentrant.
pub struct Widgets {
    layout: Layout,
    path: PathBuf,
    registry: Registry,
    storage: Storage,
    entries: BTreeMap<String, Entry>,
    host: Option<DesktopHost>,
    desktop_events: DesktopEvents,
    monitors: Vec<Monitor>,
    covered: Vec<bool>,
    windows: WindowEvents,
    _control: control::Control,
    _display: DisplayWatcher,
    _watcher: Option<ConfigWatcher>,
    editing: bool,
    drag: Option<Drag>,
    grids: Vec<EditSurface>,
    palette: Option<EditSurface>,
    rx: mpsc::Receiver<Msg>,
    media: Media,
    audio: Audio<Wasapi>,
    stats: Stats,
    icons: Icons,
    plugins: Plugins,
    worker: data::Worker,
    holds: Holds,
    held: bool,
    hidden: bool,
    reconcile_needed: bool,
    plates: BTreeMap<String, Vec<Plate>>,
    skin: look::Skin,
    presents: u64,
    clock_presents: u64,
    costs: (u64, u64, u64),
    ticker: Ticker,
    motion_frame: bool,
}
impl Widgets {
    pub fn new(ui: &mut Ui, window: HWND) -> Result<Self> {
        let host = DesktopHost::find()?;
        let desktop_events = DesktopEvents::new(host)?;
        let _control = control::Control::new()?;
        let _display = DisplayWatcher::new()?;
        let registry = registry::builtins();
        let path = files::layout()?;
        let missing = !path.exists();
        // A broken file never gets a starter layout or migration written over it.
        let mut layout = Layout::load(&path)?;
        if layout.migrate(|id| registry.replacement(id)) {
            layout.save(&path)?;
        }
        let monitors = monitor_infos();
        if missing && let Some(primary) = monitors.iter().find(|m| m.primary) {
            let grid = grid_for(&layout, primary);
            // The first-run default fits the plan's six-widget fixture. Existing layouts are
            // never reseeded, including an intentionally empty layout.
            let mut starter = Layout::default();
            for id in [
                "clock",
                "calendar",
                "now-playing",
                "weather",
                "system-monitor",
                "notes",
            ] {
                let size = registry.get(id).expect("builtin").manifest.size.default;
                if let Some(cell) = edit::vacancy(&grid, size, &starter.instances, &primary.device)
                {
                    starter.add(id, &primary.device, cell, size);
                }
            }
            starter.save(&path)?;
            layout = starter;
        }
        let _watcher = watch_file(&path, || inbox::post(WM_LAYOUT, 0))
            .inspect_err(|e| log::warn!("widget layout watcher: {e:#}"))
            .ok();
        let (tx, rx) = mpsc::channel();
        let wake = Waker::for_window(tx, window, WM_DATA);
        let config = Config::load().unwrap_or_default();
        let mut holds = Holds::default();
        holds.apply(&config);
        holds.set(Fact::OnBattery, mypaper_desktop::on_battery());
        let skin = load_skin(&config);
        let mut widgets = Self {
            layout,
            path,
            registry,
            storage: Storage::new(files::user("widget-data")?),
            entries: BTreeMap::new(),
            host: Some(host),
            desktop_events,
            monitors,
            covered: Vec::new(),
            windows: WindowEvents::install(),
            _control,
            _display,
            _watcher,
            editing: false,
            drag: None,
            grids: Vec::new(),
            palette: None,
            rx,
            media: Media::new(wake.clone()),
            audio: Audio::new(Wasapi::new(wake.clone())),
            stats: Stats::default(),
            icons: Icons::new(wake.clone()),
            plugins: Plugins::new(wake),
            worker: data::Worker::new(window),
            holds,
            held: false,
            hidden: false,
            reconcile_needed: true,
            plates: BTreeMap::new(),
            skin,
            presents: 0,
            clock_presents: 0,
            costs: (0, 0, 0),
            ticker: Ticker::new()?,
            motion_frame: false,
        };
        widgets.poll(ui);
        Ok(widgets)
    }

    /// Only handles the legacy display watcher's thread notification and clock changes; all
    /// other widget notifications use the shell inbox so COM modal waits preserve them.
    pub fn raw_message(&mut self, msg: &MSG) {
        if (msg.message == WM_DISPLAY && msg.hwnd.is_invalid()) || msg.message == WM_TIMECHANGE {
            self.monitors = monitor_infos();
            self.reconcile_needed = true;
            for entry in self.entries.values_mut() {
                entry.model.dirty = true;
                entry.model.next = Some(Instant::now());
            }
        }
    }
    pub fn handle(&mut self, ui: &mut Ui, msg: inbox::Msg) {
        match msg.message {
            WM_EDIT => {
                let on = mode(msg.lparam, self.editing);
                self.set_editing(ui, on);
            }
            WM_OVERLAY => {
                self.layout.overlay = mode(msg.lparam, self.layout.overlay);
                self.save_layout();
                self.reconcile_needed = true;
            }
            WM_LAYOUT => match Layout::load(&self.path) {
                Ok(mut layout) => {
                    if layout.migrate(|id| self.registry.replacement(id))
                        && let Err(e) = layout.save(&self.path)
                    {
                        log::warn!("widget migration: {e:#}");
                    }
                    if layout != self.layout {
                        self.finish_fields();
                        self.layout = layout;
                        self.reconcile_needed = true;
                    }
                }
                Err(e) => log::warn!("widgets.json: {e:#}; retaining the valid live layout"),
            },
            inbox::WM_CONFIG | inbox::WM_PALETTE => self.follow_config(),
            _ => {}
        }
    }
    pub fn fact(&mut self, wparam: usize, lparam: isize) {
        if let Some(fact) = Fact::from_wparam(wparam) {
            self.holds.set(fact, lparam != 0);
            self.reconcile_needed = true;
        }
    }
    fn follow_config(&mut self) {
        if let Ok(config) = Config::load() {
            self.holds.apply(&config);
            self.skin = load_skin(&config);
            self.reconcile_needed = true;
        }
    }
    #[allow(
        dead_code,
        reason = "live harness uses window geometry and z-order without mutating the host"
    )]
    pub fn live_windows(&self) -> Vec<Probe> {
        self.entries
            .values()
            .map(|e| Probe {
                instance: e.model.placed.instance.clone(),
                hwnd: e.surface.hwnd(),
                rect: e.rect,
                visible: e.surface.is_visible(),
                dpi: (e.dpi_scale * 96.0).round() as u32,
                ordered: e.surface.desktop_order_valid(),
                editing_text: e.model.field.is_some(),
            })
            .collect()
    }
    #[allow(dead_code, reason = "live fixture diagnostics")]
    pub fn costs(&self) -> (u64, u64, u64) {
        self.costs
    }
    pub fn counters(&self) -> (u64, u64) {
        (self.presents, self.clock_presents)
    }

    pub fn timer_handle(&self) -> windows::Win32::Foundation::HANDLE {
        self.ticker.handle()
    }
    pub fn frame_ready(&mut self) {
        self.motion_frame = true;
    }
    pub fn timeout_ms(&self) -> u32 {
        let entries = self
            .entries
            .values()
            .flat_map(|e| [e.model.next, e.model.fetch_at, e.model.save_at]);
        entries
            .chain([self.stats.deadline(), self.plugins.deadline()])
            .flatten()
            .min()
            .map_or(u32::MAX, |d| {
                time::wait_ms(d.saturating_duration_since(Instant::now()))
            })
    }
    pub fn poll(&mut self, ui: &mut Ui) {
        self.costs.2 += 1;
        let instant = Instant::now();
        let now = Stamp::now();
        if self.desktop_events.take_lost() {
            self.finish_fields();
            self.entries.clear();
            self.grids.clear();
            self.palette = None;
            self.host = None;
            self.desktop_events.set_host(None);
            self.reconcile_needed = true;
        }
        if self.desktop_events.take_ready() {
            match DesktopHost::find_once() {
                Ok(host) => {
                    self.host = Some(host);
                    self.desktop_events.set_host(Some(host));
                    self.monitors = monitor_infos();
                    self.reconcile_needed = true;
                }
                Err(e) => log::warn!("widget desktop: {e:#}"),
            }
        }
        if self.windows.take_changed() {
            for entry in self.entries.values().filter(|e| e.model.visible) {
                entry.surface.show();
                let _ = entry.surface.keep_above_desktop();
            }
            for grid in &self.grids {
                grid.surface.show();
            }
            if let Some(palette) = &self.palette {
                palette.surface.show();
            }

            let covered =
                if std::env::var_os("MYPAPER_WIDGETS_IGNORE_COVERED").is_some_and(|v| v == "1") {
                    vec![false; self.monitors.len()]
                } else {
                    covered_monitors(&self.monitors.iter().map(|m| m.rect).collect::<Vec<_>>())
                };
            if covered != self.covered {
                self.covered = covered;
                self.reconcile_needed = true;
            }
        }
        let reasons = self.holds.reasons();
        let hidden = reasons.contains(Reason::Locked) || reasons.contains(Reason::DisplayOff);
        let held = !reasons.is_empty();
        if held != self.held || hidden != self.hidden {
            self.held = held;
            self.hidden = hidden;
            self.reconcile_needed = true;
            self.finish_fields();
            self.worker.hold(held);
            self.media.set_held(held);
            self.audio.set_held(held);
            self.stats.set_held(held, instant);
            self.plugins.set_held(held);
            for entry in self.entries.values_mut() {
                entry.model.next = None;
                entry.model.fetch_at = None;
                entry.model.dirty = true;
                entry.model.photo.fade = None;
                entry.model.photo.previous = None;
            }
        }
        if self.reconcile_needed {
            if let Err(e) = self.reconcile(ui, now, instant) {
                log::error!("native widgets: {e:#}");
            }
            self.reconcile_needed = false;
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.service_message(ui, msg, now, instant);
        }
        while let Some(answer) = self.worker.next() {
            self.answer(ui, answer, now, instant);
        }
        let measured = Instant::now();
        let deliveries = self.stats.tick(instant);
        self.costs.0 += measured.elapsed().as_micros() as u64;
        for delivery in deliveries {
            if let Delivery::Event {
                instance, stats, ..
            } = delivery
                && let Some(entry) = self.entries.get_mut(&instance)
            {
                let history = [
                    stats.cpu,
                    stats.memory.used as f64 / stats.memory.total.max(1) as f64 * 100.0,
                    stats.gpu.unwrap_or(0.0),
                ];
                entry.model.stats = Some(stats);
                entry.model.history.push_back(history);
                if entry.model.history.len() > 60 {
                    entry.model.history.pop_front();
                }
                entry.model.dirty = true;
            }
        }
        let plugin_out = self.plugins.tick(instant);
        for out in plugin_out {
            self.deliver(ui, out, now);
        }
        let ids: Vec<_> = self.entries.keys().cloned().collect();
        for id in ids {
            let (save, fetch, photo, timer) = if let Some(entry) = self.entries.get_mut(&id) {
                let m = &mut entry.model;
                let save = m.save_at.is_some_and(|at| at <= instant);
                if save {
                    m.save_at = None;
                }
                if m.visible
                    && !self.held
                    && (m.next.is_some_and(|at| at <= instant)
                        || (self.motion_frame && m.photo.fade.is_some()))
                {
                    m.next = None;
                    m.dirty = true;
                    if m.kind == Widget::Photo
                        && m.photo.fade.is_some_and(|start| {
                            instant.saturating_duration_since(start) >= Duration::from_millis(350)
                        })
                    {
                        m.photo.fade = None;
                        m.photo.previous = None;
                    }
                }
                let fetch = m.visible
                    && !self.held
                    && !m.fetching
                    && m.fetch_at.is_some_and(|at| at <= instant);
                let photo = m.visible
                    && !self.held
                    && !m.fetching
                    && m.kind == Widget::Photo
                    && m.photo.change.is_some_and(|at| at <= instant);
                let timer = m.visible
                    && !self.held
                    && m.kind == Widget::Timer
                    && m.timer.ends_at.is_some_and(|end| end <= now.ms);
                (save, fetch, photo, timer)
            } else {
                continue;
            };
            if save && let Err(e) = self.entries[&id].model.persist(&mut self.storage) {
                log::error!("saving {id}: {e:#}");
                self.entries.get_mut(&id).unwrap().model.save_at =
                    Some(instant + Duration::from_secs(2));
            }
            if timer {
                self.finish_timer(&id, now);
            }
            if fetch {
                self.fetch(&id);
            }
            if photo {
                self.load_photo(&id, true);
            }
            if let Some(entry) = self.entries.get_mut(&id)
                && entry.model.visible
                && entry.model.dirty
                && !self.held
            {
                let measured = Instant::now();
                match paint(entry, ui, now, instant, self.editing, self.host) {
                    Err(e) => log::error!("drawing {id}: {e:#}"),
                    Ok(drawn) => {
                        if drawn {
                            self.presents += 1;
                            if entry.model.kind == Widget::Clock {
                                self.clock_presents += 1;
                            }
                        }
                        entry.model.dirty = false;
                    }
                }
                self.costs.1 += measured.elapsed().as_micros() as u64;
                entry.model.schedule(now, instant);
            }
        }
        if self.motion_frame && self.drag.is_some() {
            self.drag_frame(ui, now, instant);
        }
        self.publish_plates(self.motion_frame && self.drag.is_some());
        self.motion_frame = false;
        self.ticker.schedule(
            !self.held
                && (self.drag.is_some()
                    || self
                        .entries
                        .values()
                        .any(|e| e.model.visible && e.model.photo.fade.is_some())),
        );
    }

    fn reconcile(&mut self, ui: &mut Ui, now: Stamp, instant: Instant) -> Result<()> {
        let Some(host) = self.host else {
            return Ok(());
        };
        let devices: Vec<_> = self.monitors.iter().map(|m| m.device.clone()).collect();
        let primary = self.monitors.iter().position(|m| m.primary).unwrap_or(0);
        let installed: BTreeSet<_> = self
            .layout
            .instances
            .iter()
            .map(|i| i.instance.clone())
            .collect();
        let removed: Vec<_> = self
            .entries
            .keys()
            .filter(|id| !installed.contains(*id))
            .cloned()
            .collect();
        for id in removed {
            self.forget(&id);
            self.entries.remove(&id);
        }
        for instance in &self.layout.instances {
            let Some(manifest) = self.registry.get(&instance.widget).map(|i| &i.manifest) else {
                continue;
            };
            let Some(device) = layout::show_on(&instance.monitor, &devices, primary) else {
                continue;
            };
            let monitor = devices.iter().position(|d| d == device).unwrap();
            let screen = &self.monitors[monitor];
            let grid = grid_for(&self.layout, screen);
            let appearance = look::resolve(
                &instance.appearance,
                &self.layout.appearance,
                &look::base(&self.skin),
                look::accent(None),
            );
            let placed = layout::place(instance, manifest, &grid, &appearance);
            let rect = screen_rect(&grid, placed.cell, placed.size, screen);
            let visible = !self.hidden
                && (self.editing || !self.covered.get(monitor).copied().unwrap_or(false))
                && (self.editing || self.layout.overlay || instance.widget != "media-pill");
            if !self.entries.contains_key(&instance.instance) {
                match Model::new(placed.clone(), &mut self.storage, now) {
                    Ok(model) => {
                        let surface = Surface::new(
                            Kind::DesktopChild {
                                parent: host.icons_parent(),
                            },
                            &format!("mypaper widget {}", instance.instance),
                            Role::Pane,
                        )?;
                        let tokens = tokens(&placed.appearance);
                        self.entries.insert(
                            instance.instance.clone(),
                            Entry {
                                surface,
                                model,
                                monitor,
                                rect,
                                tokens,
                                dpi_scale: screen.dpi as f32 / 96.0,
                            },
                        );
                    }
                    Err(e) => {
                        log::error!("{} storage: {e:#}; left untouched", instance.instance);
                        continue;
                    }
                }
            }
            let entry = self.entries.get_mut(&instance.instance).unwrap();
            let was_visible = entry.model.visible;
            let changed = entry.model.placed != placed;
            if changed {
                entry.model.generation += 1;
                entry.model.fetching = false;
                entry.model.icons.clear();
                entry.model.fetch_at = None;
                entry.model.photo = model::Photo::default();
                entry.model.dirty = true;
            }
            entry.tokens = tokens(&placed.appearance);
            entry.model.placed = placed;
            entry.rect = rect;
            entry.monitor = monitor;
            entry.dpi_scale = screen.dpi as f32 / 96.0;
            entry.model.visible = visible;
            if !visible {
                if was_visible {
                    entry.model.generation += 1;
                    entry.model.fetching = false;
                    finish_field(entry, &mut self.storage);
                    entry.surface.hide();
                    entry.surface.release();
                    entry.surface.discard_events();
                    entry.model.artwork = None;
                    entry.model.icons.clear();
                    entry.model.photo = model::Photo::default();
                }
                entry.model.next = None;
                entry.model.fetch_at = None;
                self.stats.forget(&instance.instance);
                self.media.forget(&instance.instance);
                self.audio.forget(&instance.instance);
            } else if !self.held && (!was_visible || changed || entry.model.dirty) {
                entry.model.dirty = true;
                entry.model.schedule(now, instant);
                if matches!(
                    entry.model.kind,
                    Widget::Weather | Widget::Crypto | Widget::Rss
                ) {
                    entry.model.fetch_at = Some(instant);
                }
                if matches!(entry.model.kind, Widget::System | Widget::QuickToggles) {
                    let quick = entry.model.kind == Widget::QuickToggles;
                    self.stats.subscribe_fields(
                        &instance.instance,
                        "system",
                        Duration::from_secs(2),
                        instant,
                        mypaper_widgets::stats::SampleFields {
                            gpu: !quick && entry.model.flag("gpu", true),
                            disks: !quick && entry.model.flag("disk", false),
                            net: !quick && entry.model.flag("net", true),
                            battery: quick || entry.model.flag("battery", true),
                            network: quick,
                        },
                    );
                }
                if entry.model.kind == Widget::Media {
                    self.media.subscribe(&instance.instance, "media");
                    self.media.current(Target {
                        instance: instance.instance.clone(),
                        id: json!("media"),
                    });
                    if entry.model.str("variant", "card") == "viz" {
                        self.audio.subscribe(&instance.instance, "spectrum", 64);
                    }
                }
                if entry.model.kind == Widget::Photo {
                    entry.model.photo.change = Some(instant);
                }
            }
            if self.held {
                entry.model.next = None;
                entry.model.fetch_at = None;
            }
        }
        self.audio.set_visible(
            self.entries
                .iter()
                .filter(|(_, e)| e.model.visible && !self.held)
                .map(|(id, _)| id.clone())
                .collect(),
        );
        let ids: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                e.model.visible && e.model.kind == Widget::Launcher && e.model.icons.is_empty()
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.load_icons(ui, &id, now);
        }
        self.refresh_activity();
        if self.editing {
            self.make_edit_surfaces(ui)?;
        }
        Ok(())
    }
    fn forget(&mut self, id: &str) {
        self.stats.forget(id);
        self.media.forget(id);
        self.audio.forget(id);
        self.plugins.forget(id);
    }
    fn service_message(&mut self, ui: &mut Ui, msg: Msg, now: Stamp, instant: Instant) {
        match msg {
            Msg::Media { token, result } => {
                let out = self.media.on_result(token, result);
                for item in out {
                    self.deliver(ui, item, now);
                }
            }
            Msg::Audio { generation, frame } => {
                let out = self.audio.on_frame(generation, &frame);
                for item in out {
                    self.deliver(ui, item, now);
                }
            }
            Msg::Answered(target, result) => self.deliver(ui, Out::Reply(target, result), now),
            Msg::Plugin {
                id,
                generation,
                line,
            } => {
                let out = self.plugins.on_line(&id, generation, line, instant);
                for item in out {
                    self.deliver(ui, item, now);
                }
            }
            Msg::Picked(_) => {}
        }
    }
    fn deliver(&mut self, ui: &mut Ui, out: Out, now: Stamp) {
        let (instance, tag, result) = match out {
            Out::Reply(target, result) => (
                target.instance,
                target.id.as_str().unwrap_or("data").to_owned(),
                result,
            ),
            Out::Event { instance, data, .. } => (
                instance,
                data["sub"].as_str().unwrap_or("data").to_owned(),
                Ok(data["value"].clone()),
            ),
        };
        let Some(entry) = self.entries.get_mut(&instance).filter(|e| e.model.visible) else {
            return;
        };
        let Ok(mut data) = result else {
            return;
        };
        let m = &mut entry.model;
        if let Some(index) = tag
            .strip_prefix("icon-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(image) = decode_url(ui, &data, (96, 96)) {
                m.icons.insert(index, image);
            }
        } else if tag == "spectrum" {
            m.spectrum = data
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .map(|v| v as f32)
                .collect();
        } else {
            if m.kind == Widget::Media {
                m.artwork = data
                    .get("artwork")
                    .and_then(|v| decode_url(ui, v, (256, 256)));
                if let Some(object) = data.as_object_mut() {
                    object.insert("_at".into(), json!(now.ms));
                }
            }
            m.data = data;
        }
        m.dirty = true;
    }
    fn answer(&mut self, ui: &mut Ui, answer: data::Answer, now: Stamp, instant: Instant) {
        match answer {
            data::Answer::Data {
                instance,
                generation,
                result,
            } => {
                let Some(entry) = self
                    .entries
                    .get_mut(&instance)
                    .filter(|e| e.model.generation == generation)
                else {
                    return;
                };
                let m = &mut entry.model;
                m.fetching = false;
                if !m.visible {
                    return;
                }
                let delay = if result.is_ok() {
                    m.error = None;
                    match m.kind {
                        Widget::Weather => 600.0,
                        Widget::Rss => m.num("refreshMinutes", 20.0).clamp(15.0, 120.0) * 60.0,
                        _ => m.num("refreshMinutes", 5.0).clamp(2.0, 30.0) * 60.0,
                    }
                } else {
                    60.0
                };
                match result {
                    Ok(value) => {
                        if matches!(m.kind, Widget::Crypto | Widget::Rss)
                            && let Err(e) =
                                self.storage.set(&instance, "cache", Some(value.clone()))
                        {
                            log::warn!("widget cache: {e:#}");
                        }
                        m.data = value;
                    }
                    Err(error) => m.error = Some(error),
                }
                m.fetch_at = (!self.held).then_some(instant + Duration::from_secs_f64(delay));
                m.dirty = true;
            }
            data::Answer::Photo {
                instance,
                generation,
                result,
            } => {
                let Some(entry) = self
                    .entries
                    .get_mut(&instance)
                    .filter(|e| e.model.generation == generation)
                else {
                    return;
                };
                let m = &mut entry.model;
                m.fetching = false;
                if !m.visible {
                    return;
                }
                match result.and_then(|bytes| {
                    ui.decode_image(
                        &bytes,
                        (
                            (entry.rect.width() * 1.5).min(1200.0) as u32,
                            (entry.rect.height() * 1.5).min(1200.0) as u32,
                        ),
                    )
                    .map_err(|e| e.to_string())
                }) {
                    Ok(image) => {
                        m.photo.previous = m.photo.current.take();
                        m.photo.current = Some(Arc::new(image));
                        m.photo.fade = m.photo.previous.is_some().then_some(instant);
                        m.error = None;
                    }
                    Err(e) => m.error = Some(e),
                }
                m.photo.change = (m.photo_paths().len() > 1).then_some(
                    instant + Duration::from_secs_f64(m.num("interval", 15.0).clamp(5.0, 120.0)),
                );
                m.dirty = true;
                m.schedule(now, instant);
            }
        }
    }
    fn fetch(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            let m = &mut entry.model;
            m.fetching = true;
            m.fetch_at = None;
            self.worker.send(data::Work::Fetch {
                instance: id.into(),
                generation: m.generation,
                kind: m.kind,
                settings: m.placed.settings.clone(),
            });
        }
    }
    fn load_photo(&mut self, id: &str, advance: bool) {
        let Some(entry) = self.entries.get_mut(id) else {
            return;
        };
        let m = &mut entry.model;
        let paths = m.photo_paths();
        m.photo.change = None;
        if paths.is_empty() {
            m.next = None;
            return;
        }
        if advance {
            let step = if m.flag("shuffle", false) && paths.len() > 2 {
                1 + (time::epoch_ms().unsigned_abs() as usize % (paths.len() - 1))
            } else {
                1
            };
            m.photo.index = (m.photo.index + step) % paths.len();
        }
        m.photo.index %= paths.len();
        let value = &paths[m.photo.index];
        let path = if let Some(url) = mypaper_widgets::pick::parse_data_url(value) {
            if url.instance != id {
                m.error = Some("Photo belongs to another instance".into());
                return;
            }
            files::user("widget-data")
                .unwrap_or_default()
                .join(id)
                .join("files")
                .join(url.name)
        } else {
            PathBuf::from(value)
        };
        m.fetching = true;
        self.worker.send(data::Work::Photo {
            instance: id.into(),
            generation: m.generation,
            path,
        });
    }
    fn load_icons(&mut self, ui: &mut Ui, id: &str, now: Stamp) {
        let targets: Vec<_> = self.entries[id]
            .model
            .list("shortcuts")
            .iter()
            .enumerate()
            .filter_map(|(i, app)| Some((i, app["target"].as_str()?.to_owned())))
            .collect();
        for (i, path) in targets {
            let target = Target {
                instance: id.into(),
                id: json!(format!("icon-{i}")),
            };
            if let Some(reply) = self.icons.get(target.clone(), &path, 64) {
                self.deliver(ui, Out::Reply(target, reply), now);
            }
        }
    }
    fn finish_timer(&mut self, id: &str, now: Stamp) {
        let m = &mut self.entries.get_mut(id).unwrap().model;
        let at = m.timer.ends_at.take().unwrap_or(now.ms);
        m.timer.remaining = 0;
        m.timer.finished_at = Some(at);
        let activity = m.timer.mode == "focus";
        if activity {
            m.timer.sessions += 1;
        }
        if m.flag("autoAdvance", false) && m.timer.mode != "custom" && now.ms - at < 2500 {
            let mode = m.timer.next_mode(&m.placed.settings);
            m.timer = Timer::idle(&m.placed.settings, mode, m.timer.sessions);
            m.timer.ends_at = Some(now.ms + m.timer.total);
        }
        m.dirty = true;
        if let Err(e) = m.persist(&mut self.storage) {
            log::warn!("timer storage: {e:#}");
        }
        if activity {
            self.record_activity("timer", &now.date.key(), 1);
        }
    }
    fn record_activity(&mut self, source: &str, day: &str, delta: i32) {
        let ns = storage::shared("activity");
        let mut write = || -> Result<()> {
            let value = self.storage.get(&ns, source)?;
            let mut counts = value.as_object().cloned().unwrap_or_default();
            let next = (counts.get(day).and_then(Value::as_i64).unwrap_or(0) + i64::from(delta))
                .clamp(0, 9999);
            if next == 0 {
                counts.remove(day);
            } else {
                counts.insert(day.into(), json!(next));
            }
            self.storage.set(&ns, source, Some(Value::Object(counts)))
        };
        if let Err(e) = write() {
            log::warn!("activity: {e:#}");
        }
        self.refresh_activity();
    }
    fn refresh_activity(&mut self) {
        let ns = storage::shared("activity");
        let todo = self.storage.get(&ns, "todo").unwrap_or(Value::Null);
        let timer = self.storage.get(&ns, "timer").unwrap_or(Value::Null);
        for entry in self
            .entries
            .values_mut()
            .filter(|e| e.model.kind == Widget::Heatmap)
        {
            let m = &mut entry.model;
            let source = m.str("source", "all");
            let mut counts = BTreeMap::new();
            for (name, data) in [("todo", &todo), ("timer", &timer)] {
                if source != "all" && source != name {
                    continue;
                }
                for (day, value) in data.as_object().into_iter().flatten() {
                    if mypaper_ui::calendar::Date::parse(day).is_some()
                        && let Some(n) = value.as_u64()
                    {
                        let count = counts.entry(day.clone()).or_insert(0u32);
                        *count = (*count + n.min(9999) as u32).min(19998);
                    }
                }
            }
            if m.counts != counts {
                m.counts = counts;
                m.dirty = true;
            }
        }
    }

    pub fn on_event(&mut self, ui: &mut Ui, surface: SurfaceId, event: &Event) {
        if matches!(event, Event::DpiChanged(_)) {
            self.monitors = monitor_infos();
            if self.drag.is_none() {
                self.finish_fields();
                self.reconcile_needed = true;
            } else {
                self.motion_frame = true;
            }
            return;
        }
        if matches!(event, Event::Cancelled | Event::CaptureLost) && self.drag.is_some() {
            self.cancel_drag();
            return;
        }
        if matches!(event, Event::VisibilityChanged) {
            if let Some(entry) = self.entries.values().find(|e| e.surface.id() == surface)
                && entry.model.visible
            {
                entry.surface.show();
            }
            return;
        }
        if let Some(palette) = &self.palette
            && palette.surface.id() == surface
        {
            if matches!(event, Event::Key(27)) {
                self.set_editing(ui, false);
                return;
            }
            let hit = match event {
                Event::Release(p, Button::Left) => {
                    palette.surface.shown().hit_at(p.at).map(|h| h.id)
                }
                Event::Invoke(hit) => Some(*hit),
                _ => None,
            };
            if let Some(hit) = hit {
                let ids: Vec<_> = self.registry.widgets.keys().cloned().collect();
                if let Some(id) = ids.get(hit as usize) {
                    self.add(ui, id.clone());
                } else {
                    self.set_editing(ui, false);
                }
            }
            return;
        }
        if self.grids.iter().any(|s| s.surface.id() == surface) {
            if matches!(event, Event::Key(27)) {
                self.set_editing(ui, false);
            }
            return;
        }
        let Some(id) = self
            .entries
            .iter()
            .find(|(_, e)| e.surface.id() == surface)
            .map(|(id, _)| id.clone())
        else {
            return;
        };
        if self.editing {
            match event {
                Event::Key(27) => {
                    self.cancel_drag();
                    self.set_editing(ui, false);
                }
                Event::Press(p, Button::Left) => {
                    let entry = &self.entries[&id];
                    let hit = entry.surface.shown().hit_at(p.at).map(|h| h.id);
                    if matches!(hit, Some(view::REMOVE | view::DUPLICATE)) {
                        return;
                    }
                    // SAFETY: capture is paired with release/cancel/drop on our own surface.
                    unsafe {
                        SetCapture(entry.surface.hwnd());
                        let _ = SetFocus(Some(entry.surface.hwnd()));
                    }
                    self.motion_frame = true;
                    self.drag = Some(Drag {
                        id,
                        start: p.screen,
                        original: entry.rect,
                        pending: entry.rect,
                        resize: hit == Some(view::RESIZE),
                    });
                }
                Event::Move(p) => {
                    if let Some(drag) = &mut self.drag
                        && drag.id == id
                    {
                        let dx = (p.screen.0 - drag.start.0) as f32;
                        let dy = (p.screen.1 - drag.start.1) as f32;
                        drag.pending = if drag.resize {
                            Rect::new(
                                drag.original.left,
                                drag.original.top,
                                (drag.original.width() + dx).max(48.0),
                                (drag.original.height() + dy).max(48.0),
                            )
                        } else {
                            drag.original.offset(dx, dy)
                        };
                    }
                }
                Event::Release(p, Button::Left) => {
                    if self.drag.is_some() {
                        self.drop_drag(ui);
                    } else {
                        let hit = self.entries[&id].surface.shown().hit_at(p.at).map(|h| h.id);
                        if let Some(hit) = hit {
                            self.edit_action(ui, &id, hit);
                        }
                    }
                }
                Event::Invoke(hit) => self.edit_action(ui, &id, *hit),
                _ => {}
            }
            return;
        }
        match event {
            Event::TextChanged => {
                let m = &mut self.entries.get_mut(&id).unwrap().model;
                m.dirty = true;
                if m.kind == Widget::Notes
                    && let Some(field) = &m.field
                    && let Ok(text) = field.text()
                    && text != m.note
                {
                    m.note = text;
                    m.save_at = Some(Instant::now() + Duration::from_millis(400));
                }
            }
            Event::TextSubmit => self.submit_field(&id),
            Event::Deactivated | Event::Key(27) => {
                if let Some(entry) = self.entries.get_mut(&id) {
                    finish_field(entry, &mut self.storage);
                    entry.model.dirty = true;
                }
            }
            Event::Release(p, Button::Left) => {
                if let Some(hit) = self.entries[&id].surface.shown().hit_at(p.at).map(|h| h.id) {
                    self.action(ui, &id, hit);
                }
            }
            Event::Invoke(hit) => self.action(ui, &id, *hit),
            _ => {}
        }
    }
    fn action(&mut self, ui: &mut Ui, id: &str, hit: HitId) {
        let now = Stamp::now();
        let entry = self.entries.get_mut(id).unwrap();
        let m = &mut entry.model;
        m.dirty = true;
        match m.kind {
            Widget::Media => {
                let action = match hit {
                    view::PRIMARY => MediaAction::Toggle,
                    view::PREVIOUS => MediaAction::Previous,
                    view::NEXT => MediaAction::Next,
                    _ => return,
                };
                self.media.control(
                    Target {
                        instance: id.into(),
                        id: json!("media"),
                    },
                    action,
                );
            }
            Widget::QuickToggles => {
                if let Some(tile) = hit
                    .checked_sub(view::ITEM)
                    .and_then(|i| view::TOGGLES.get(i as usize))
                {
                    self.worker.send(data::Work::Launch(tile.3.into()));
                }
            }
            Widget::Launcher => {
                if hit == view::FIELD {
                    self.open_field(id, None);
                } else if let Some(target) = hit
                    .checked_sub(view::ITEM)
                    .and_then(|i| m.list("shortcuts").get(i as usize))
                    .and_then(|v| v["target"].as_str())
                {
                    self.worker.send(data::Work::Launch(target.into()));
                }
            }
            Widget::Calendar => {
                if hit == view::PRIMARY {
                    self.open_field(id, None);
                } else if hit >= view::EDIT_TASK {
                    let index = (hit - view::EDIT_TASK) as usize;
                    if index < m.events.len() {
                        self.open_field(id, Some(index));
                    }
                } else if hit >= view::DELETE_TASK {
                    let index = (hit - view::DELETE_TASK) as usize;
                    if index < m.events.len() {
                        m.events.remove(index);
                        m.save_at = Some(Instant::now());
                    }
                } else if hit == view::PREVIOUS || hit == view::NEXT {
                    let delta = if hit == view::PREVIOUS { -1 } else { 1 };
                    if m.str("variant", "month") == "strip" {
                        m.selected = m.selected.add_days(delta as i64 * 7);
                        m.month = m.selected.shift_month(0);
                    } else {
                        m.month = m.month.shift_month(delta);
                    }
                } else if hit > view::ITEM && hit <= view::ITEM + 31 {
                    if m.str("variant", "month") == "strip" {
                        let first = m.selected.add_days(-i64::from(m.selected.weekday()));
                        if let Some(day) = (0..7)
                            .map(|i| first.add_days(i))
                            .find(|d| d.day == hit - view::ITEM)
                        {
                            m.selected = day;
                        }
                    } else {
                        m.selected = mypaper_ui::calendar::Date {
                            day: hit - view::ITEM,
                            ..m.month
                        };
                    }
                }
            }
            Widget::Timer => {
                match hit {
                    view::PRIMARY => {
                        if let Some(end) = m.timer.ends_at.take() {
                            m.timer.remaining = (end - now.ms).max(0);
                        } else {
                            if m.timer.remaining == 0 {
                                let mode = m.timer.next_mode(&m.placed.settings);
                                m.timer = Timer::idle(&m.placed.settings, mode, m.timer.sessions);
                            }
                            m.timer.ends_at = Some(now.ms + m.timer.remaining);
                            m.timer.finished_at = None;
                        }
                    }
                    view::RESET => {
                        m.timer = Timer::idle(&m.placed.settings, &m.timer.mode, m.timer.sessions)
                    }
                    view::NEXT => {
                        let mode = m.timer.next_mode(&m.placed.settings);
                        m.timer = Timer::idle(&m.placed.settings, mode, m.timer.sessions);
                    }
                    hit if (view::MODE..view::MODE + 4).contains(&hit) => {
                        let mode =
                            ["focus", "short", "long", "custom"][(hit - view::MODE) as usize];
                        m.timer = Timer::idle(&m.placed.settings, mode, m.timer.sessions);
                    }
                    _ => {}
                }
                if let Err(e) = m.persist(&mut self.storage) {
                    log::warn!("timer: {e:#}");
                }
                m.schedule(now, Instant::now());
            }
            Widget::Photo if hit == view::PRIMARY => self.load_photo(id, true),
            Widget::Notes => {
                if hit == view::FIELD {
                    self.open_field(id, None);
                } else if let Some(index) = hit.checked_sub(view::NOTE_TASK).map(|n| n as usize) {
                    let mut lines: Vec<_> = m.note.lines().map(str::to_owned).collect();
                    if let Some(line) = lines.get_mut(index) {
                        if line.starts_with("- [ ] ") {
                            line.replace_range(3..4, "x");
                        } else if line.starts_with("- [x] ") || line.starts_with("- [X] ") {
                            line.replace_range(3..4, " ");
                        }
                    }
                    let newline = m.note.ends_with('\n');
                    m.note = lines.join("\n");
                    if newline {
                        m.note.push('\n');
                    }
                    m.save_at = Some(Instant::now());
                }
            }
            Widget::Todo => {
                if hit == view::FIELD {
                    self.open_field(id, None);
                } else if hit >= view::EDIT_TASK {
                    let index = (hit - view::EDIT_TASK) as usize;
                    if index < m.tasks.len() {
                        self.open_field(id, Some(index));
                    }
                } else if hit >= view::DELETE_TASK {
                    let i = (hit - view::DELETE_TASK) as usize;
                    if i < m.tasks.len() {
                        m.tasks.remove(i);
                        m.counted.clear();
                        m.save_at = Some(Instant::now());
                    }
                } else if let Some(index) = hit
                    .checked_sub(view::ITEM)
                    .map(|i| i as usize)
                    .filter(|&i| i < m.tasks.len())
                {
                    let task = &mut m.tasks[index];
                    task.done = !task.done;
                    let done = task.done;
                    let day = if done {
                        let day = now.date.key();
                        m.counted.insert(index, day.clone());
                        Some(day)
                    } else {
                        m.counted.remove(&index)
                    };
                    m.save_at = Some(Instant::now());
                    if let Some(day) = day {
                        self.record_activity("todo", &day, if done { 1 } else { -1 });
                    }
                }
            }
            Widget::Rss => {
                if let Some(url) = hit
                    .checked_sub(view::ITEM)
                    .and_then(|i| m.data["items"].get(i as usize))
                    .and_then(|v| v["link"].as_str())
                    .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                {
                    self.worker.send(data::Work::Launch(url.into()));
                }
            }
            Widget::Crypto => {
                if let Some(coin) = hit
                    .checked_sub(view::ITEM)
                    .and_then(|i| m.data["coins"].get(i as usize))
                    .and_then(|v| v["id"].as_str())
                {
                    self.worker.send(data::Work::Launch(format!(
                        "https://www.coingecko.com/en/coins/{}",
                        mypaper_plugin_weather::http::encode(coin)
                    )));
                }
            }
            Widget::Weather if hit == view::PRIMARY => {
                let location = m.data["current"]["location"]["name"]
                    .as_str()
                    .unwrap_or("weather");
                self.worker.send(data::Work::Launch(format!(
                    "https://www.msn.com/en-us/weather/forecast/in-{}",
                    mypaper_plugin_weather::http::encode(location)
                )));
            }
            Widget::Quote if hit == view::PRIMARY => {
                m.quote_offset = m.quote_offset.wrapping_add(1)
            }
            _ => {}
        }
        let _ = ui;
    }
    fn open_field(&mut self, id: &str, index: Option<usize>) {
        self.finish_fields();
        let entry = self.entries.get_mut(id).unwrap();
        let m = &mut entry.model;
        m.task_edit = index;
        let scale = self.monitors[entry.monitor].dpi as f32 / 96.0 * entry.tokens.scale;
        let size = if m.kind == Widget::Notes {
            14.0 * m.num("fontScale", 1.0) as f32
        } else {
            13.0
        };
        let result = TextField::new(
            entry.surface.hwnd(),
            Rect::new(
                16.0 * scale,
                16.0 * scale,
                entry.rect.width() - 32.0 * scale,
                entry.rect.height() - 32.0 * scale,
            ),
            m.kind == Widget::Notes,
            size * scale,
            entry.tokens.colors.text,
        );
        match result {
            Ok(field) => {
                let text = if m.kind == Widget::Notes {
                    m.note.as_str()
                } else if m.kind == Widget::Calendar {
                    index
                        .and_then(|i| m.events.get(i))
                        .and_then(|e| e["title"].as_str())
                        .unwrap_or("")
                } else if let Some(index) = index {
                    m.tasks[index].text.as_str()
                } else {
                    ""
                };
                if let Err(e) = field.set_text(text) {
                    log::error!("text field: {e:#}");
                    return;
                }
                entry.surface.set_text_field(Some(field.clone()));
                field.focus();
                m.field = Some(field);
                m.dirty = true;
            }
            Err(e) => log::error!("RichEdit unavailable: {e:#}"),
        }
    }
    fn submit_field(&mut self, id: &str) {
        let entry = self.entries.get_mut(id).unwrap();
        let m = &mut entry.model;
        let Some(field) = &m.field else {
            return;
        };
        let Ok(text) = field.text() else {
            return;
        };
        if m.kind == Widget::Calendar {
            let clean = text.trim();
            if clean.is_empty() {
                return;
            }
            let (clock, title) = clean
                .split_once(' ')
                .filter(|(clock, _)| {
                    clock.len() == 5
                        && clock.as_bytes()[2] == b':'
                        && clock[..2].parse::<u32>().is_ok_and(|h| h < 24)
                        && clock[3..].parse::<u32>().is_ok_and(|n| n < 60)
                })
                .map_or((None, clean), |(clock, title)| (Some(clock), title));
            let title = title.chars().take(80).collect::<String>();
            if let Some(index) = m.task_edit.take() {
                if let Some(event) = m.events.get_mut(index) {
                    event["title"] = json!(title);
                    if let Some(time) = clock {
                        event["time"] = json!(time);
                    }
                }
            } else if m.events.len() < 500 {
                m.events.push(json!({"id":format!("event-{}",time::epoch_ms()),"date":m.selected.key(),"title":title,"time":clock,"color":0}));
            }
            let _ = field.set_text("");
            m.save_at = Some(Instant::now());
            m.dirty = true;
        } else if m.kind == Widget::Todo {
            let clean = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect::<String>();
            if let Some(index) = m.task_edit.take() {
                if clean.is_empty() {
                    m.tasks.remove(index);
                } else if let Some(task) = m.tasks.get_mut(index) {
                    task.text = clean;
                }
            } else if !clean.is_empty() && m.tasks.len() < 300 {
                let task = Task {
                    text: clean,
                    done: false,
                };
                if m.str("addPosition", "bottom") == "top" {
                    m.tasks.insert(0, task);
                    m.counted.clear();
                } else {
                    m.tasks.push(task);
                }
            }
            let _ = field.set_text("");
            m.save_at = Some(Instant::now());
            m.dirty = true;
        } else if m.kind == Widget::Launcher
            && let Some(url) = search_url(&text, &m.placed.settings)
        {
            self.worker.send(data::Work::Launch(url));
            let _ = field.set_text("");
            m.dirty = true;
        }
    }
    fn finish_fields(&mut self) {
        for entry in self.entries.values_mut() {
            finish_field(entry, &mut self.storage);
        }
    }

    fn set_editing(&mut self, ui: &mut Ui, on: bool) {
        if on == self.editing {
            return;
        }
        self.finish_fields();
        self.cancel_drag();
        self.editing = on;
        self.grids.clear();
        self.palette = None;
        for entry in self.entries.values_mut() {
            entry.model.dirty = true;
        }
        self.reconcile_needed = true;
        self.poll(ui);
    }
    fn make_edit_surfaces(&mut self, ui: &mut Ui) -> Result<()> {
        self.grids.clear();
        self.palette = None;
        let Some(host) = self.host else {
            return Ok(());
        };
        for (index, monitor) in self.monitors.iter().enumerate() {
            let mut surface = Surface::new(
                Kind::DesktopChild {
                    parent: host.icons_parent(),
                },
                "Widget grid",
                Role::Pane,
            )?;
            let size = (monitor.rect.width as u32, monitor.rect.height as u32);
            let scale = monitor.dpi as f32 / 96.0;
            surface.present(
                ui,
                (monitor.rect.x, monitor.rect.y),
                size,
                edit::grid_view(&grid_for(&self.layout, monitor), size, scale),
            )?;
            surface.show();
            surface.keep_above_desktop()?;
            self.grids.push(EditSurface {
                surface,
                monitor: index,
            });
        }
        for entry in self.entries.values() {
            entry.surface.keep_above_desktop()?;
        }
        if let Some((index, monitor)) = self.monitors.iter().enumerate().find(|(_, m)| m.primary) {
            let names = self
                .registry
                .widgets
                .iter()
                .map(|(id, i)| (id.clone(), i.manifest.name.clone()))
                .collect::<Vec<_>>();
            let (list, size) = edit::palette(&names, monitor.dpi as f32 / 96.0);
            let origin = (
                monitor.work.x + (monitor.work.width - size.0 as i32) / 2,
                monitor.work.y + 12,
            );
            let mut surface = Surface::new(
                Kind::DesktopChild {
                    parent: host.icons_parent(),
                },
                "Add widgets",
                Role::Toolbar,
            )?;
            surface.present(ui, origin, size, list)?;
            surface.show();
            surface.raise_in_desktop_band()?;
            self.palette = Some(EditSurface {
                surface,
                monitor: index,
            });
        }
        Ok(())
    }
    fn edit_action(&mut self, ui: &mut Ui, id: &str, hit: HitId) {
        if hit == view::REMOVE {
            self.forget(id);
            if let Some(mut entry) = self.entries.remove(id) {
                finish_field(&mut entry, &mut self.storage);
            }
            let _ = self.layout.remove(id);
            self.save_layout();
            self.reconcile_needed = true;
        } else if hit == view::DUPLICATE {
            let Some(original) = self.layout.get(id).cloned() else {
                return;
            };
            let monitor = self.entries[id].monitor;
            let grid = grid_for(&self.layout, &self.monitors[monitor]);
            let manifest = &self.registry.get(&original.widget).unwrap().manifest;
            let (total, here) = self
                .layout
                .count_of(&original.widget, |i| i.monitor == original.monitor);
            if manifest.instances.allows(total, here)
                && let Some(cell) = edit::vacancy(
                    &grid,
                    original.size,
                    &self.layout.instances,
                    &original.monitor,
                )
                && let Ok(copy) = self.layout.duplicate(id, cell)
            {
                if let (Ok(from), Ok(to)) = (
                    files::user(&format!("widget-data/{id}.json")),
                    files::user(&format!("widget-data/{copy}.json")),
                ) && from.exists()
                {
                    let _ = std::fs::copy(from, to);
                }
                if let Ok(root) = files::user("widget-data") {
                    mypaper_widgets::pick::copy_instance(&root, id, &copy);
                }
                self.save_layout();
                self.reconcile_needed = true;
            }
        }
        let _ = ui;
    }
    fn add(&mut self, ui: &mut Ui, id: String) {
        let Some(index) = self.palette.as_ref().map(|p| p.monitor) else {
            return;
        };
        let monitor = &self.monitors[index];
        let grid = grid_for(&self.layout, monitor);
        let manifest = &self.registry.get(&id).unwrap().manifest;
        let (total, here) = self.layout.count_of(&id, |i| i.monitor == monitor.device);
        if !manifest.instances.allows(total, here) {
            return;
        }
        let size = manifest.clamp_size(manifest.size.default);
        if let Some(cell) = edit::vacancy(&grid, size, &self.layout.instances, &monitor.device) {
            self.layout.add(&id, &monitor.device, cell, size);
            self.save_layout();
            self.reconcile_needed = true;
            self.poll(ui);
        }
    }
    fn cancel_drag(&mut self) {
        if self.drag.take().is_some() {
            for entry in self.entries.values_mut() {
                entry.model.dirty = true;
            }
            self.reconcile_needed = true;
        }
        // SAFETY: releases only this thread's capture, if any.
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    fn drag_frame(&mut self, ui: &mut Ui, now: Stamp, instant: Instant) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        if let Some(entry) = self.entries.get_mut(&drag.id) {
            let center = (
                drag.pending.left as i32 + drag.pending.width() as i32 / 2,
                drag.pending.top as i32 + drag.pending.height() as i32 / 2,
            );
            let mut dpi_changed = false;
            if let Some((index, monitor)) = self
                .monitors
                .iter()
                .enumerate()
                .find(|(_, m)| m.rect.contains_point(center.0, center.1))
            {
                dpi_changed =
                    entry.monitor != index || entry.dpi_scale != monitor.dpi as f32 / 96.0;
                entry.monitor = index;
                entry.dpi_scale = monitor.dpi as f32 / 96.0;
                if !drag.resize {
                    let grid = grid_for(&self.layout, monitor);
                    let size = grid.rect([0, 0], entry.model.placed.size);
                    drag.pending.right = drag.pending.left + size.width as f32 * entry.dpi_scale;
                    drag.pending.bottom = drag.pending.top + size.height as f32 * entry.dpi_scale;
                }
            }
            entry.rect = drag.pending;
            if drag.resize || dpi_changed {
                entry.model.dirty = true;
                if let Err(e) = paint(entry, ui, now, instant, true, self.host) {
                    log::warn!("resizing widget: {e:#}");
                }
            } else {
                if let Err(e) = entry
                    .surface
                    .move_to((entry.rect.left as i32, entry.rect.top as i32))
                {
                    log::warn!("moving widget: {e:#}");
                }
                let _ = entry.surface.keep_above_desktop();
            }
        }
    }
    fn drop_drag(&mut self, ui: &mut Ui) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        // SAFETY: paired with this gesture's capture.
        unsafe {
            let _ = ReleaseCapture();
        }
        let center = (
            drag.pending.left as i32 + drag.pending.width() as i32 / 2,
            drag.pending.top as i32 + drag.pending.height() as i32 / 2,
        );
        if let Some((monitor, screen)) = self
            .monitors
            .iter()
            .enumerate()
            .find(|(_, m)| m.rect.contains_point(center.0, center.1))
        {
            let grid = grid_for(&self.layout, screen);
            let local = drag
                .pending
                .offset(-screen.rect.x as f32, -screen.rect.y as f32);
            let (cell, size) = edit::snapped(&grid, local, screen.dpi as f64 / 96.0);
            let widget = self.layout.get(&drag.id).unwrap().widget.clone();
            let size = self
                .registry
                .get(&widget)
                .unwrap()
                .manifest
                .clamp_size(size);
            let (cell, size) = grid.clamp(cell, size);
            if edit::free(
                &grid,
                cell,
                size,
                &self.layout.instances,
                &screen.device,
                Some(&drag.id),
            ) && let Some(instance) = self
                .layout
                .instances
                .iter_mut()
                .find(|i| i.instance == drag.id)
            {
                instance.monitor = self.monitors[monitor].device.clone();
                instance.cell = cell;
                instance.size = size;
                self.save_layout();
            }
        }
        self.reconcile_needed = true;
        for entry in self.entries.values_mut() {
            entry.model.dirty = true;
        }
        self.poll(ui);
    }
    fn save_layout(&self) {
        if let Err(e) = self.layout.save(&self.path) {
            log::error!("widgets.json save: {e:#}");
        }
    }
    fn publish_plates(&mut self, force: bool) {
        for (index, monitor) in self.monitors.iter().enumerate() {
            let plates: Vec<_> = self
                .entries
                .values()
                .filter(|e| e.monitor == index && e.model.visible)
                .filter_map(|e| {
                    let a = &e.model.placed.appearance;
                    let surface = a.get("surface").and_then(Value::as_str).unwrap_or("glass");
                    if matches!(surface, "none" | "solid" | "tint") {
                        return None;
                    }
                    let liquid = look::liquid(&self.skin);
                    let tint = a
                        .get("tint")
                        .and_then(Value::as_str)
                        .and_then(Color::parse)
                        .unwrap_or(look::DEFAULT_TINT);
                    Some(Plate {
                        id: e.surface.hwnd().0 as usize as u64,
                        rect: e
                            .rect
                            .offset(-monitor.rect.x as f32, -monitor.rect.y as f32),
                        radius: a.get("radius").and_then(Value::as_f64).unwrap_or(22.0) as f32
                            * monitor.dpi as f32
                            / 96.0,
                        material: Material {
                            clarity: 1.0 - liquid.tint.unwrap_or(0.06),
                            blur: liquid.blur.unwrap_or(12.0),
                            refraction: liquid.refraction.unwrap_or(0.6),
                            highlight: liquid.highlight.unwrap_or(0.55),
                            tint: Rgba::rgb8(tint.0, liquid.tint.unwrap_or(0.06)),
                        },
                    })
                })
                .collect();
            if force || self.plates.get(&monitor.device) != Some(&plates) {
                glass::publish(glass::Source::Widgets, monitor, &plates);
                self.plates.insert(monitor.device.clone(), plates);
            }
        }
    }
}
impl Drop for Widgets {
    fn drop(&mut self) {
        self.finish_fields();
        self.cancel_drag();
        for monitor in &self.monitors {
            glass::publish(glass::Source::Widgets, monitor, &[]);
        }
        log::info!(
            "native widgets stopped: presents={} clock_presents={}",
            self.counters().0,
            self.counters().1
        );
    }
}

fn paint(
    entry: &mut Entry,
    ui: &mut Ui,
    now: Stamp,
    instant: Instant,
    editing: bool,
    host: Option<DesktopHost>,
) -> Result<bool> {
    let size = (
        entry.rect.width().round().max(1.0) as u32,
        entry.rect.height().round().max(1.0) as u32,
    );
    let scale = entry
        .model
        .placed
        .appearance
        .get("scale")
        .and_then(Value::as_f64)
        .unwrap_or(1.0) as f32
        * entry.dpi_scale;
    let list = view::view(
        &entry.model,
        &entry.tokens,
        view::Frame {
            size,
            scale,
            now,
            instant,
            editing,
        },
        &mut ui.text,
    );
    let drawn = entry.surface.present(
        ui,
        (entry.rect.left as i32, entry.rect.top as i32),
        size,
        list,
    )?;
    entry.surface.show();
    if host.is_some() {
        entry.surface.keep_above_desktop()?;
    }
    if drawn {
        log::debug!(
            "widget present instance={} widget={}",
            entry.model.placed.instance,
            entry.model.placed.widget
        );
    }
    Ok(drawn)
}
fn finish_field(entry: &mut Entry, storage: &mut Storage) {
    if let Some(field) = entry.model.field.take() {
        if let Ok(text) = field.text() {
            if entry.model.kind == Widget::Notes {
                entry.model.note = text;
            } else if entry.model.kind == Widget::Todo
                && let Some(index) = entry.model.task_edit.take()
                && let Some(task) = entry.model.tasks.get_mut(index)
            {
                task.text = text.chars().take(240).collect();
            }
        }
        entry.surface.set_text_field(None);
        field.blur();
        entry.model.dirty = true;
    }
    if (entry.model.save_at.take().is_some()
        || matches!(
            entry.model.kind,
            Widget::Notes | Widget::Todo | Widget::Timer
        ))
        && let Err(e) = entry.model.persist(storage)
    {
        log::error!("widget storage: {e:#}");
    }
}
fn load_skin(config: &Config) -> look::Skin {
    let dir = mypaper_theme::locate(Some(&config.theme)).ok();
    look::load_skin(dir.as_deref())
}
pub(crate) fn tokens(appearance: &serde_json::Map<String, Value>) -> Tokens {
    let layer =
        serde_json::from_value::<mypaper_theme::Appearance>(Value::Object(appearance.clone()))
            .unwrap_or_default();
    let dark = layer.text != Some(TextTone::Dark);
    let accent = match layer.accent {
        Some(Paint::Color(color)) => color.0,
        _ => [220; 3],
    };
    Tokens {
        font: TextStyle::new(layer.font.as_deref().unwrap_or("Segoe UI Variable"), 14.0),
        colors: Colors::new(dark, accent),
        material: mypaper_ui::Material {
            tint: Rgba::rgb8(
                layer.tint.unwrap_or(look::DEFAULT_TINT).0,
                layer.opacity.unwrap_or(0.46),
            ),
        },
        radius: layer.radius.unwrap_or(22.0),
        scale: layer.scale.unwrap_or(1.0),
        ..Tokens::default()
    }
}
fn grid_for(layout: &Layout, monitor: &Monitor) -> Grid {
    let scale = monitor.dpi as f64 / 96.0;
    Grid::new(
        &layout.grid,
        icon_cell(),
        RectF {
            x: f64::from(monitor.work.x - monitor.rect.x) / scale,
            y: f64::from(monitor.work.y - monitor.rect.y) / scale,
            width: f64::from(monitor.work.width) / scale,
            height: f64::from(monitor.work.height) / scale,
        },
    )
}
fn screen_rect(grid: &Grid, cell: [u32; 2], size: [u32; 2], monitor: &Monitor) -> Rect {
    let r = grid.rect(cell, size);
    let scale = monitor.dpi as f32 / 96.0;
    Rect::new(
        monitor.rect.x as f32 + r.x as f32 * scale,
        monitor.rect.y as f32 + r.y as f32 * scale,
        r.width as f32 * scale,
        r.height as f32 * scale,
    )
}
fn mode(value: isize, current: bool) -> bool {
    match value {
        0 => false,
        1 => true,
        _ => !current,
    }
}
fn decode_url(ui: &Ui, value: &Value, size: (u32, u32)) -> Option<Arc<mypaper_ui::Image>> {
    let data = value.as_str()?.split_once(",")?.1;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    ui.decode_image(&bytes, size).ok().map(Arc::new)
}
fn icon_cell() -> [f64; 2] {
    let read = |name: PCWSTR| {
        let mut buffer = [0u16; 32];
        let mut bytes = size_of_val(&buffer) as u32;
        // SAFETY: the bounded buffer and its size stay valid across the registry call.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Control Panel\Desktop\WindowMetrics"),
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
            .ok()
            .ok()?;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
            .trim()
            .parse::<i32>()
            .ok()
    };
    [
        layout::icon_spacing_dip(read(w!("IconSpacing"))),
        layout::icon_spacing_dip(read(w!("IconVerticalSpacing"))),
    ]
}
fn search_url(query: &str, settings: &serde_json::Map<String, Value>) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if !query.contains(char::is_whitespace) {
        if query.starts_with("https://") || query.starts_with("http://") {
            return Some(query.into());
        }
        if !query.contains(':') && query.split('/').next().is_some_and(|s| s.contains('.')) {
            return Some(format!("https://{query}"));
        }
    }
    let template = match model::string(settings, "engine", "duckduckgo") {
        "google" => "https://www.google.com/search?q={q}",
        "bing" => "https://www.bing.com/search?q={q}",
        "brave" => "https://search.brave.com/search?q={q}",
        "ecosia" => "https://www.ecosia.org/search?q={q}",
        "startpage" => "https://www.startpage.com/do/search?q={q}",
        "custom" => model::string(settings, "customUrl", ""),
        _ => "https://duckduckgo.com/?q={q}",
    };
    (template.contains("{q}")
        && (template.starts_with("https://") || template.starts_with("http://")))
    .then(|| template.replace("{q}", &mypaper_plugin_weather::http::encode(query)))
}
