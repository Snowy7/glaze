//! Native desktop widgets and edit mode. One cropped DesktopChild surface per visible instance,
//! one deadline queue, and sleeping service workers. No WebView, polling loop or ambient motion.
mod control;
mod coverage;
mod data;
mod edit;
mod markdown;
mod metrics;
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
use coverage::WindowEvents;
#[cfg(test)]
use model::Timer;
use model::{Model, Task, Widget};
use mypaper_config::Config;
use mypaper_desktop::hold::{Fact, Holds, Reason};
use mypaper_desktop::{DesktopEvents, DesktopHost, Monitor, covered_monitors, monitor_infos};
use mypaper_theme::{Color, Paint, TextTone};
use mypaper_ui::{
    Button, Colors, Event, FileWatch, HitId, Kind, Rect, Rgba, Role, Surface, SurfaceId, TextField,
    TextStyle, Ticker, Tokens, Ui,
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
/// Explicit read-only performance checkpoint; never scheduled by the host.
pub const WM_DIAGNOSTICS: u32 = WM_APP + 104;

struct Entry {
    surface: Surface,
    topmost: bool,
    model: Model,
    monitor: usize,
    rect: Rect,
    tokens: Tokens,
    dpi_scale: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkCosts {
    clock_us: u64,
    coverage_us: u64,
    stats_us: u64,
    paint_us: u64,
    polls: u64,
    orders: u64,
    order_us: u64,
    plate_us: u64,
    poll_us: u64,
}
struct Drag {
    id: String,
    start: (i32, i32),
    original: Rect,
    pending: Rect,
    resize: bool,
    edges: [bool; 4],
}
struct EditSurface {
    surface: Surface,
    monitor: usize,
    /// Hover and press over the palette's tiles (unused on grids).
    hot: mypaper_ui::Interaction,
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
    layout_error: Option<String>,
    theme_path: Option<PathBuf>,
    theme_watcher: Option<FileWatch>,
    accent: Color,
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
    _watcher: Option<FileWatch>,
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
    order_due: Option<Instant>,
    startup_release: Option<Instant>,
    foreground: HWND,
    plates: BTreeMap<String, Vec<Plate>>,
    skin: look::Skin,
    presents: u64,
    clock_presents: u64,
    costs: WorkCosts,
    ticker: Ticker,
    motion_frame: bool,
}
impl Widgets {
    pub fn new(ui: &mut Ui, window: HWND, initial_facts: &[(usize, isize)]) -> Result<Self> {
        let host = DesktopHost::find()?;
        let desktop_events = DesktopEvents::new(host)?;
        let _control = control::Control::new()?;
        let _display = DisplayWatcher::new()?;
        let registry = registry::builtins();
        let path = files::layout()?;
        let missing = !path.exists();
        // A broken file never gets a starter layout or migration written over it.
        let (mut layout, layout_error) = match Layout::load(&path) {
            Ok(layout) => (layout, None),
            Err(error) => {
                if let Ok(bytes) = std::fs::read(&path) {
                    let _ = files::backup(&path, &bytes);
                }
                (
                    Layout::default(),
                    Some(format!(
                        "Invalid widgets.json: {error:#}. Correct the file before editing."
                    )),
                )
            }
        };
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
        let _watcher = FileWatch::new(&path, || inbox::post(WM_LAYOUT, 0))
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
            layout_error,
            theme_path: None,
            theme_watcher: None,
            accent: palette_accent(&config),
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
            order_due: None,
            startup_release: Some(Instant::now() + Duration::from_secs(30)),
            foreground: HWND::default(),
            plates: BTreeMap::new(),
            skin,
            presents: 0,
            clock_presents: 0,
            costs: WorkCosts::default(),
            ticker: Ticker::new()?,
            motion_frame: false,
        };
        widgets.follow_config();
        for &(wparam, lparam) in initial_facts {
            widgets.fact(wparam, lparam);
        }
        widgets.poll(ui);
        Ok(widgets)
    }

    /// Only handles the legacy display watcher's thread notification and clock changes; all
    /// other widget notifications use the shell inbox so COM modal waits preserve them.
    pub fn raw_message(&mut self, msg: &MSG) {
        if (msg.message == WM_DISPLAY && msg.hwnd.is_invalid()) || msg.message == WM_TIMECHANGE {
            self.monitors = monitor_infos();
            self.stats.refresh_devices();
            self.reconcile_needed = true;
            for entry in self.entries.values_mut() {
                entry.model.dirty = true;
                entry.model.next = Some(Instant::now());
            }
        }
    }
    pub fn handle(&mut self, ui: &mut Ui, msg: inbox::Msg) {
        match msg.message {
            WM_DIAGNOSTICS => {
                log::info!(
                    "widget checkpoint presents={:?} costs={:?}",
                    self.counters(),
                    self.costs
                );
                for (id, entry) in &self.entries {
                    log::info!(
                        "widget checkpoint id={id} rect={:?} visible={}",
                        entry.rect,
                        entry.model.visible
                    );
                }
            }
            WM_EDIT => {
                let on = mode(msg.lparam, self.editing);
                self.set_editing(ui, on);
            }
            WM_OVERLAY => {
                let mut next = self.layout.clone();
                next.overlay = mode(msg.lparam, next.overlay);
                self.commit_layout(next);
                self.reconcile_needed = true;
            }
            WM_LAYOUT => match Layout::load(&self.path) {
                Ok(mut layout) => {
                    self.layout_error = None;
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
                Err(e) => {
                    let message = format!(
                        "widgets.json: {e:#}. Fix the file to resume arranging; edits are not saved."
                    );
                    if let Ok(bytes) = std::fs::read(&self.path) {
                        let _ = files::backup(&self.path, &bytes);
                    }
                    self.layout_error = Some(message);
                    self.reconcile_needed = true;
                }
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
            self.accent = palette_accent(&config);
            let path = mypaper_theme::locate(Some(&config.theme))
                .ok()
                .map(|d| d.join("theme.json"));
            if path != self.theme_path {
                self.theme_watcher = path
                    .as_ref()
                    .and_then(|p| FileWatch::new(p, || inbox::post(inbox::WM_CONFIG, 0)).ok());
                self.theme_path = path;
            }
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
    pub fn costs(&self) -> WorkCosts {
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
            .chain([
                self.stats.deadline(),
                self.plugins.deadline(),
                self.order_due,
                self.startup_release,
            ])
            .flatten()
            .min()
            .map_or(u32::MAX, |d| {
                time::wait_ms(d.saturating_duration_since(Instant::now()))
            })
    }
    pub fn poll(&mut self, ui: &mut Ui) {
        self.costs.polls += 1;
        let instant = Instant::now();
        let now = Stamp::now();
        if self.startup_release.is_some_and(|at| at <= instant) {
            self.startup_release = None;
            release_startup_heap();
        }
        self.costs.clock_us += instant.elapsed().as_micros() as u64;
        let measured = Instant::now();
        let mut plates_changed = false;
        // A foreground transition can precede Explorer's asynchronous Show Desktop reorder.
        // One final repair after its animation settles avoids restacking for unrelated events.
        // SAFETY: a read-only snapshot; HWND is used only for identity comparison.
        let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if foreground != self.foreground {
            self.foreground = foreground;
            self.order_due = Some(instant + Duration::from_millis(250));
        }
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
            // Explorer can move its anchor without sending our windows a position message.
            // One band probe catches that; ordinary app/child churn needs no group repair.
            if self
                .entries
                .values()
                .find(|entry| {
                    entry.model.visible
                        && self
                            .registry
                            .get(&entry.model.placed.widget)
                            .is_some_and(|widget| {
                                widget.manifest.preset != mypaper_widgets::manifest::Preset::Overlay
                            })
                })
                .is_some_and(|entry| !entry.surface.desktop_order_valid())
            {
                self.order_due
                    .get_or_insert(instant + Duration::from_millis(50));
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
        self.costs.coverage_us += measured.elapsed().as_micros() as u64;
        let reasons = self.holds.reasons();
        let hidden = reasons.contains(Reason::Locked) || reasons.contains(Reason::DisplayOff);
        let held = !reasons.is_empty();
        if held != self.held || hidden != self.hidden {
            if held && self.editing {
                self.cancel_drag();
                self.editing = false;
                self.grids.clear();
                self.palette = None;
            }
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
            plates_changed = true;
            if let Err(e) = self.reconcile(ui, now, instant) {
                log::error!("native widgets: {e:#}");
            }
            self.reconcile_needed = false;
        }
        if self.order_due.is_some_and(|at| at <= instant) {
            let measured = Instant::now();
            self.costs.orders += 1;
            self.order_due = None;
            // Moving a paired glass window can displace an earlier member of the band.
            // A second pass converges the group; an already ordered surface is a no-op.
            for _ in 0..2 {
                for entry in self.entries.values().filter(|e| e.model.visible) {
                    entry.surface.show();
                }
            }
            for grid in &self.grids {
                grid.surface.show();
            }
            if let Some(palette) = &self.palette {
                palette.surface.show();
                let _ = palette.surface.raise_in_desktop_band();
            }
            self.costs.order_us += measured.elapsed().as_micros() as u64;
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.service_message(ui, msg, now, instant);
        }
        while let Some(answer) = self.worker.next() {
            self.answer(ui, answer, now, instant);
        }
        let measured = Instant::now();
        let deliveries = self.stats.tick(instant);
        self.costs.stats_us += measured.elapsed().as_micros() as u64;
        for delivery in deliveries {
            if let Delivery::Event {
                instance, stats, ..
            } = delivery
                && let Some(entry) = self.entries.get_mut(&instance)
            {
                let before = metrics::visible(&entry.model);
                let quick_changed =
                    entry.model.stats.as_ref().is_none_or(|old| {
                        old.battery != stats.battery || old.network != stats.network
                    });
                entry.model.stats = Some(stats);
                let after = metrics::visible(&entry.model);
                let spark = entry.model.str("variant", "rings") == "spark";
                if spark {
                    for metric in &after {
                        let history = entry.model.history.entry(metric.id.clone()).or_default();
                        let sample = if metric.id == "net" {
                            entry
                                .model
                                .stats
                                .as_ref()
                                .and_then(|s| s.net)
                                .map_or(0.0, |n| n.down)
                        } else {
                            metric.percent
                        };
                        history.push_back(sample);
                        if history.len() > 60 {
                            history.pop_front();
                        }
                    }
                }
                entry.model.dirty |= if entry.model.kind == Widget::QuickToggles {
                    quick_changed
                } else {
                    spark || before != after
                };
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
                        || (self.motion_frame
                            && (m.photo.fade.is_some()
                                || m.action_at.is_some()
                                || m.flip_at.is_some())))
                {
                    if m.kind == Widget::Clock {
                        let changed = m.last_clock.is_some_and(|old| {
                            old.clock(m.flag("format24", true), m.flag("showSeconds", false))
                                != now.clock(m.flag("format24", true), m.flag("showSeconds", false))
                        });
                        if changed && m.str("variant", "desk") == "flip" {
                            m.previous_clock = m.last_clock;
                            m.flip_at = Some(instant);
                        }
                        m.last_clock = Some(now);
                    }
                    if m.flip_at.is_some_and(|at| {
                        instant.saturating_duration_since(at) >= Duration::from_millis(280)
                    }) {
                        m.flip_at = None;
                    }
                    if m.action_at.is_some_and(|at| {
                        instant.saturating_duration_since(at) >= Duration::from_millis(650)
                    }) {
                        m.action_at = None;
                    }
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
                self.costs.paint_us += measured.elapsed().as_micros() as u64;
                entry.model.schedule(now, instant);
            }
        }
        if self.motion_frame && self.drag.is_some() {
            self.drag_frame(ui, now, instant);
        }
        let moving = self.motion_frame && self.drag.is_some();
        if plates_changed || moving {
            let measured = Instant::now();
            self.publish_plates(moving);
            self.costs.plate_us += measured.elapsed().as_micros() as u64;
        }
        self.motion_frame = false;
        self.ticker.schedule(
            !self.held
                && (self.drag.is_some()
                    || self.entries.values().any(|e| {
                        e.model.visible
                            && (e.model.photo.fade.is_some()
                                || e.model.action_at.is_some()
                                || e.model.flip_at.is_some())
                    })),
        );
        self.costs.poll_us += instant.elapsed().as_micros() as u64;
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
                self.accent,
            );
            let placed = layout::place(instance, manifest, &grid, &appearance);
            let rect = screen_rect(&grid, placed.cell, placed.size, screen);
            let overlay = manifest.preset == mypaper_widgets::manifest::Preset::Overlay;
            let topmost = overlay && !self.editing;
            let visible = !self.hidden
                && (self.editing
                    || overlay
                    || !self.covered.get(monitor).copied().unwrap_or(false))
                && (self.editing || self.layout.overlay || !overlay);
            if !self.entries.contains_key(&instance.instance) {
                match Model::new(placed.clone(), &mut self.storage, now) {
                    Ok(model) => {
                        let surface =
                            widget_surface(topmost, host.icons_parent(), &instance.instance)?;
                        let tokens = tokens(&placed.appearance);
                        self.entries.insert(
                            instance.instance.clone(),
                            Entry {
                                surface,
                                topmost,
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
            if entry.topmost != topmost {
                let surface = widget_surface(topmost, host.icons_parent(), &instance.instance)?;
                finish_field(entry, &mut self.storage);
                entry.surface = surface;
                entry.topmost = topmost;
                entry.model.dirty = true;
            }
            let was_visible = entry.model.visible;
            let changed = entry.model.placed != placed;
            if changed {
                entry.model.generation += 1;
                entry.model.fetching = false;
                entry.model.scroll = 0.0;
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
                    entry.model.artwork_backdrop = None;
                    entry.model.artwork_tint = None;
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
                        if quick {
                            Duration::from_secs(2)
                        } else {
                            entry.model.stats_interval()
                        },
                        instant,
                        mypaper_widgets::stats::SampleFields {
                            gpu: !quick && entry.model.flag("gpu", true),
                            disks: !quick && entry.model.flag("disk", false),
                            net: !quick && entry.model.flag("net", true),
                            battery: quick || entry.model.flag("battery", true),
                            network: quick || entry.model.flag("net", true),
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
                update_artwork(m, ui, &data);
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
            data::Answer::Activated {
                instance,
                generation,
                hit,
                result,
            } => {
                if let Some(entry) = self
                    .entries
                    .get_mut(&instance)
                    .filter(|e| e.model.generation == generation)
                {
                    entry.model.action_hit = Some(hit);
                    entry.model.notice = result.err();
                    entry.model.dirty = true;
                }
            }
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
                m.photo.change = (m.photo_paths().len() > 1 || m.error.is_some()).then_some(
                    instant
                        + Duration::from_secs_f64(if m.error.is_some() {
                            5.0
                        } else {
                            m.num("interval", 15.0).clamp(5.0, 120.0)
                        }),
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
                m.photo.change = Some(Instant::now() + Duration::from_secs(5));
                m.schedule(Stamp::now(), Instant::now());
                m.dirty = true;
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
            m.timer = m.timer.enter(&m.placed.settings, mode);
            m.timer.ends_at = Some(now.ms + m.timer.total);
        }
        m.dirty = true;
        if let Err(e) = m.persist(&mut self.storage) {
            log::warn!("timer storage: {e:#}");
        }
        if activity {
            self.record_activity("timer", &Stamp::at(at).date.key(), 1);
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
        if matches!(event, Event::VisibilityChanged | Event::Activated) {
            if let Some(entry) = self.entries.values().find(|e| e.surface.id() == surface)
                && entry.model.visible
                && (matches!(event, Event::Activated)
                    || !entry.surface.is_visible()
                    || !entry.surface.desktop_order_valid())
            {
                // Explorer restores windows in a batch. Repair the whole desktop band once
                // after that batch, without a periodic z-order check or a content redraw.
                self.order_due
                    .get_or_insert(Instant::now() + Duration::from_millis(50));
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
            let palette = self.palette.as_mut().unwrap();
            let response = palette.hot.handle(palette.surface.shown(), event);
            if response != mypaper_ui::interact::Response::Unchanged {
                self.repaint_palette(ui);
            }
            if let mypaper_ui::interact::Response::Activate(hit) = response {
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
                Event::Key(key @ 37..=40) => self.edit_key(&id, *key),
                Event::Key(27) => {
                    self.cancel_drag();
                    self.set_editing(ui, false);
                }
                Event::Press(p, Button::Left) => {
                    if self.layout_error.is_some() {
                        return;
                    }
                    let entry = &self.entries[&id];
                    let hit = entry.surface.shown().hit_at(p.at).map(|h| h.id);
                    if matches!(hit, Some(view::REMOVE | view::DUPLICATE)) {
                        return;
                    }
                    // SAFETY: capture is paired with release/cancel/drop on our own surface.
                    unsafe {
                        entry.surface.take_focus();
                        SetCapture(entry.surface.hwnd());
                        let _ = SetFocus(Some(entry.surface.hwnd()));
                    }
                    self.motion_frame = true;
                    self.drag = Some(Drag {
                        id,
                        start: p.screen,
                        original: entry.rect,
                        pending: entry.rect,
                        resize: hit.is_some_and(|h| view::resize_edges(h).is_some()),
                        edges: hit.and_then(view::resize_edges).unwrap_or([false; 4]),
                    });
                }
                Event::Move(p) => {
                    if let Some(drag) = &mut self.drag
                        && drag.id == id
                    {
                        let dx = (p.screen.0 - drag.start.0) as f32;
                        let dy = (p.screen.1 - drag.start.1) as f32;
                        drag.pending = if drag.resize {
                            let [left, top, right, bottom] = drag.edges;
                            Rect {
                                left: if left {
                                    (drag.original.left + dx).min(drag.original.right - 48.0)
                                } else {
                                    drag.original.left
                                },
                                top: if top {
                                    (drag.original.top + dy).min(drag.original.bottom - 48.0)
                                } else {
                                    drag.original.top
                                },
                                right: if right {
                                    (drag.original.right + dx).max(drag.original.left + 48.0)
                                } else {
                                    drag.original.right
                                },
                                bottom: if bottom {
                                    (drag.original.bottom + dy).max(drag.original.top + 48.0)
                                } else {
                                    drag.original.bottom
                                },
                            }
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
        if let Event::Key(key) = event
            && self.widget_key(ui, &id, *key)
        {
            return;
        }
        match event {
            Event::Focus(hit) => {
                let m = &mut self.entries.get_mut(&id).unwrap().model;
                m.focus = Some(*hit);
                m.dirty = true;
            }
            Event::Wheel(_, delta) => self.scroll(&id, -f32::from(*delta) / 120.0 * 48.0),
            Event::Press(p, Button::Left) => {
                let entry = self.entries.get_mut(&id).unwrap();
                entry.surface.take_focus();
                entry.model.pressed = entry.surface.shown().hit_at(p.at).map(|h| h.id);
                entry.model.focus = entry.model.pressed.filter(|&h| h != view::BODY);
                entry.model.notice = None;
                entry.model.dirty = true;
            }
            Event::Move(p) => {
                let entry = self.entries.get_mut(&id).unwrap();
                let hit = entry.surface.shown().hit_at(p.at);
                let hover = hit.map(|h| h.id);
                if hover != entry.model.hover {
                    entry.model.hover = hover;
                    entry.model.dirty = true;
                }
            }
            Event::Leave => {
                let m = &mut self.entries.get_mut(&id).unwrap().model;
                m.dirty |= m.hover.take().is_some() | m.pressed.take().is_some();
            }
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
                    // Activation messages can already be queued when a click opens the field.
                    // Only commit a focusout that still describes the current foreground.
                    if matches!(event, Event::Deactivated)
                        // SAFETY: read-only foreground query against our live surface.
                        && unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
                            == entry.surface.hwnd()
                    {
                        return;
                    }
                    finish_field(entry, &mut self.storage);
                    entry.model.dirty = true;
                }
            }
            Event::Release(p, Button::Left) => {
                self.entries.get_mut(&id).unwrap().model.pressed = None;
                if let Some(hit) = self.entries[&id].surface.shown().hit_at(p.at).map(|h| h.id) {
                    self.action(ui, &id, hit);
                }
            }
            Event::Invoke(hit) => self.action(ui, &id, *hit),
            _ => {}
        }
    }
    fn scroll(&mut self, id: &str, delta: f32) {
        let m = &mut self.entries.get_mut(id).unwrap().model;
        let next = (m.scroll + delta).clamp(0.0, m.scroll_max.get());
        if m.scroll != next {
            m.scroll = next;
            m.dirty = true;
            m.hover = None;
        }
    }
    fn widget_key(&mut self, ui: &mut Ui, id: &str, key: u16) -> bool {
        let entry = &self.entries[id];
        if key == 27 && entry.model.field.is_some() {
            let entry = self.entries.get_mut(id).unwrap();
            if entry.model.kind == Widget::Launcher
                && let Some(field) = &entry.model.field
                && field.text().is_ok_and(|text| !text.is_empty())
            {
                let _ = field.set_text("");
                entry.model.notice = None;
                entry.model.dirty = true;
                return true;
            }
            if entry.model.kind == Widget::Todo || entry.model.kind == Widget::Calendar {
                if let Some(field) = entry.model.field.take() {
                    field.blur();
                }
                entry.surface.set_text_field(None);
                entry.model.task_edit = None;
                entry.model.event_draft = None;
                entry.model.dirty = true;
            } else {
                finish_field(entry, &mut self.storage);
            }
            return true;
        }
        if entry.model.field_keyboard && key != 9 {
            return false;
        }
        match key {
            9 => {
                if entry.model.kind == Widget::Todo && entry.model.field_keyboard {
                    self.submit_field(id);
                    let entry = self.entries.get_mut(id).unwrap();
                    finish_field(entry, &mut self.storage);
                }
                let entry = self.entries.get_mut(id).unwrap();
                if let Some(field) = &entry.model.field {
                    if let Some(draft) = &mut entry.model.event_draft
                        && let Ok(text) = field.text()
                    {
                        draft.capture(text);
                    }
                    field.blur();
                    entry.surface.set_text_field(None);
                    entry.model.field_keyboard = false;
                }
                let hits: Vec<_> = entry
                    .surface
                    .shown()
                    .hits
                    .iter()
                    .filter(|h| h.role != Role::Pane)
                    .cloned()
                    .collect();
                entry.model.focus = mypaper_ui::focus::step(
                    &hits,
                    entry.model.focus,
                    if shift_down() { -1 } else { 1 },
                );
                if entry.model.focus == Some(view::FIELD)
                    && let Some(field) = &entry.model.field
                {
                    entry.surface.set_text_field(Some(field.clone()));
                    field.focus();
                    entry.model.field_keyboard = true;
                }
                entry.model.dirty = true;
            }
            13 | 32 => {
                let hit = entry.model.focus.or_else(|| {
                    entry
                        .surface
                        .shown()
                        .hits
                        .iter()
                        .find(|h| h.role != Role::Pane)
                        .map(|h| h.id)
                });
                if let Some(hit) = hit {
                    self.action(ui, id, hit);
                }
            }
            113 | 46 if entry.model.kind == Widget::Todo => {
                let index = entry.model.focus.and_then(|hit| {
                    if (view::EDIT_TASK..view::EDIT_TASK + 300).contains(&hit) {
                        Some(hit - view::EDIT_TASK)
                    } else if (view::ITEM..view::ITEM + 300).contains(&hit) {
                        Some(hit - view::ITEM)
                    } else {
                        None
                    }
                });
                if let Some(index) = index {
                    self.action(
                        ui,
                        id,
                        if key == 113 {
                            view::EDIT_TASK
                        } else {
                            view::DELETE_TASK
                        } + index,
                    );
                }
            }
            33 | 34 => {
                let page = entry.model.scroll_page.get() * 0.9;
                self.scroll(id, if key == 33 { -page } else { page });
            }
            36 => self.scroll(id, -f32::MAX),
            35 => self.scroll(id, f32::MAX),
            38 | 40 => self.scroll(id, if key == 38 { -32.0 } else { 32.0 }),
            _ => return false,
        }
        true
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
                    m.action_hit = Some(hit);
                    m.action_at = Some(Instant::now());
                    self.worker.send(data::Work::Activate {
                        instance: id.into(),
                        generation: m.generation,
                        hit,
                        target: tile.3.into(),
                    });
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
                    let target = target.to_owned();
                    m.action_hit = Some(hit);
                    m.action_at = Some(Instant::now());
                    self.worker.send(data::Work::Activate {
                        instance: id.into(),
                        generation: m.generation,
                        hit,
                        target,
                    });
                }
            }
            Widget::Calendar => {
                if m.event_draft.is_some() {
                    self.event_action(id, hit);
                    return;
                }
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
                } else if hit == view::TODAY {
                    m.selected = now.date;
                    m.month = now.date.shift_month(0);
                    m.scroll = 0.0;
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
                        let first = m.selected.add_days(-i64::from(
                            (m.selected.weekday() + 7 - calendar_start(m)) % 7,
                        ));
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
                                let mode = if m.timer.mode == "custom" {
                                    "custom"
                                } else {
                                    m.timer.next_mode(&m.placed.settings)
                                };
                                m.timer = m.timer.enter(&m.placed.settings, mode);
                            }
                            m.timer.ends_at = Some(now.ms + m.timer.remaining);
                            m.timer.finished_at = None;
                        }
                    }
                    view::RESET => m.timer = m.timer.enter(&m.placed.settings, &m.timer.mode),
                    view::NEXT => {
                        let mode = m.timer.next_mode(&m.placed.settings);
                        m.timer = m.timer.enter(&m.placed.settings, mode);
                    }
                    hit if (view::MODE..view::MODE + 4).contains(&hit) => {
                        let mode =
                            ["focus", "short", "long", "custom"][(hit - view::MODE) as usize];
                        m.timer = m.timer.enter(&m.placed.settings, mode);
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
                    if let Some(line) = lines.get_mut(index)
                        && let Some(at) = markdown::task_box(line)
                    {
                        let checked = line.as_bytes()[at] != b' ';
                        line.replace_range(at..at + 1, if checked { " " } else { "x" });
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
                        m.remove_counted(i);
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
            Widget::Weather if hit == view::RETRY => {
                m.fetch_at = Some(Instant::now());
                m.error = None;
            }
            Widget::Weather if hit == view::PRIVACY => self
                .worker
                .send(data::Work::Launch("ms-settings:privacy-location".into())),
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
        if m.kind == Widget::Calendar {
            let event = index.and_then(|i| m.events.get(i));
            m.event_draft = Some(model::EventDraft {
                title: event.and_then(|e| e["title"].as_str()).unwrap_or("").into(),
                date: event
                    .and_then(|e| e["date"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| m.selected.key()),
                time: event.and_then(|e| e["time"].as_str()).unwrap_or("").into(),
                color: event.and_then(|e| e["color"].as_u64()).unwrap_or(0) as usize,
                part: model::EventPart::Title,
            });
        }
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
                m.field_keyboard = true;
                m.focus = Some(view::FIELD);
                m.field = Some(field);
                m.dirty = true;
            }
            Err(e) => log::error!("RichEdit unavailable: {e:#}"),
        }
    }
    fn submit_field(&mut self, id: &str) {
        let entry = self.entries.get_mut(id).unwrap();
        let m = &mut entry.model;
        let Some(field) = m.field.clone() else {
            return;
        };
        let Ok(text) = field.text() else {
            return;
        };
        if m.kind == Widget::Calendar {
            self.event_action(id, view::PRIMARY);
            return;
        }
        if m.kind == Widget::Todo {
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
                    m.remove_counted(index);
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
                    m.insert_counted(0);
                    m.scroll = 0.0;
                } else {
                    m.tasks.push(task);
                    m.scroll = f32::MAX; // clamped by the next viewport layout to the newly added row.
                }
            }
            let _ = field.set_text("");
            m.save_at = Some(Instant::now());
            m.dirty = true;
        } else if m.kind == Widget::Launcher {
            if let Some(url) = search_url(&text, &m.placed.settings) {
                self.worker.send(data::Work::Launch(url));
                let _ = field.set_text("");
                m.notice = None;
            } else if !text.trim().is_empty() {
                m.notice = Some("Set a valid HTTP(S) search URL containing {q} in Studio.".into());
            }
            m.dirty = true;
        }
    }
    fn event_action(&mut self, id: &str, hit: HitId) {
        let entry = self.entries.get_mut(id).unwrap();
        let m = &mut entry.model;
        if hit == view::CANCEL {
            m.event_draft = None;
            m.task_edit = None;
            if let Some(field) = m.field.take() {
                field.blur();
            }
            entry.surface.set_text_field(None);
            m.notice = None;
            m.dirty = true;
            return;
        }
        let Some(draft) = &mut m.event_draft else {
            return;
        };
        let Some(field) = &m.field else { return };
        if let Ok(text) = field.text() {
            draft.capture(text);
        }
        if hit == view::PRIMARY {
            match draft.event() {
                Ok(mut event) => {
                    if let Some(index) = m.task_edit.take() {
                        if let Some(existing) = m.events.get_mut(index) {
                            event["id"] = existing["id"].clone();
                            *existing = event;
                        }
                    } else if m.events.len() < 500 {
                        m.events.push(event);
                    } else {
                        m.notice = Some("The calendar holds up to 500 events".into());
                        m.dirty = true;
                        return;
                    }
                    m.event_draft = None;
                    if let Some(field) = m.field.take() {
                        field.blur();
                    }
                    entry.surface.set_text_field(None);
                    m.save_at = Some(Instant::now());
                    m.notice = None;
                }
                Err(error) => m.notice = Some(error),
            }
        } else if (view::EVENT_COLOR..view::EVENT_COLOR + 6).contains(&hit) {
            draft.color = (hit - view::EVENT_COLOR) as usize;
        } else {
            draft.part = match hit {
                view::EVENT_DATE => model::EventPart::Date,
                view::EVENT_TIME => model::EventPart::Time,
                _ => model::EventPart::Title,
            };
            let text = match draft.part {
                model::EventPart::Title => &draft.title,
                model::EventPart::Date => &draft.date,
                model::EventPart::Time => &draft.time,
            };
            let _ = field.set_text(text);
            entry.surface.set_text_field(Some(field.clone()));
            field.focus();
            m.field_keyboard = true;
            m.focus = Some(view::FIELD);
        }
        m.dirty = true;
    }
    fn finish_fields(&mut self) {
        for entry in self.entries.values_mut() {
            finish_field(entry, &mut self.storage);
        }
    }

    fn set_editing(&mut self, ui: &mut Ui, on: bool) {
        if (on && (self.held || !self.holds.reasons().is_empty())) || on == self.editing {
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
                hot: Default::default(),
            });
        }
        for entry in self.entries.values() {
            if entry.model.placed.widget != "media-pill" {
                entry.surface.keep_above_desktop()?;
            }
        }
        if let Some((index, monitor)) = self.monitors.iter().enumerate().find(|(_, m)| m.primary) {
            let names = self
                .registry
                .widgets
                .iter()
                .map(|(id, i)| (id.clone(), i.manifest.name.clone()))
                .collect::<Vec<_>>();
            let (list, size) = edit::palette(
                &names,
                monitor.dpi as f32 / 96.0,
                self.layout_error.as_deref(),
                &Default::default(),
                &mut ui.text,
            );
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
            // The keyboard comes here, so Escape leaves edit mode at once.
            surface.take_focus();
            self.palette = Some(EditSurface {
                surface,
                monitor: index,
                hot: Default::default(),
            });
        }
        Ok(())
    }
    /// Draws the palette again with its hover and press state (only what changed reaches the
    /// screen).
    fn repaint_palette(&mut self, ui: &mut Ui) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let Some(monitor) = self.monitors.get(palette.monitor) else {
            return;
        };
        let names: Vec<_> = self
            .registry
            .widgets
            .iter()
            .map(|(id, i)| (id.clone(), i.manifest.name.clone()))
            .collect();
        let (list, size) = edit::palette(
            &names,
            monitor.dpi as f32 / 96.0,
            self.layout_error.as_deref(),
            &palette.hot,
            &mut ui.text,
        );
        let origin = palette.surface.origin();
        if let Err(e) = palette.surface.present(ui, origin, size, list) {
            log::warn!("widget palette: drawing: {e:#}");
        }
    }
    fn edit_action(&mut self, ui: &mut Ui, id: &str, hit: HitId) {
        if self.layout_error.is_some() {
            return;
        }
        if hit == view::REMOVE {
            let mut next = self.layout.clone();
            if next.remove(id).is_ok() && self.commit_layout(next) {
                self.forget(id);
                if let Some(mut entry) = self.entries.remove(id) {
                    finish_field(&mut entry, &mut self.storage);
                }
                if let Ok(root) = files::user("widget-data") {
                    mypaper_widgets::pick::remove_instance(&root, id);
                    let _ = self.storage.remove(id);
                }
                self.reconcile_needed = true;
            }
        } else if hit == view::DUPLICATE {
            let Some(original) = self.layout.get(id).cloned() else {
                return;
            };
            let grid = grid_for(&self.layout, &self.monitors[self.entries[id].monitor]);
            let manifest = &self.registry.get(&original.widget).unwrap().manifest;
            let (total, here) = self
                .layout
                .count_of(&original.widget, |i| i.monitor == original.monitor);
            if !manifest.instances.allows(total, here) {
                self.edit_feedback(id, "This widget has reached its instance limit");
                return;
            }
            let Some(cell) = edit::vacancy(
                &grid,
                original.size,
                &self.layout.instances,
                &original.monitor,
            ) else {
                self.edit_feedback(id, "No free space for a duplicate");
                return;
            };
            let mut next = self.layout.clone();
            let Ok(copy) = next.duplicate(id, cell) else {
                return;
            };
            let copied = files::user("widget-data").and_then(|root| {
                anyhow::ensure!(!root.join(&copy).exists(), "duplicate files already exist");
                self.storage.duplicate(id, &copy)?;
                if let Err(error) = mypaper_widgets::pick::copy_instance_checked(&root, id, &copy) {
                    mypaper_widgets::pick::remove_instance(&root, &copy);
                    let _ = self.storage.remove(&copy);
                    return Err(error);
                }
                Ok(())
            });
            if let Err(error) = copied {
                self.edit_feedback(id, &format!("Could not duplicate: {error:#}"));
                return;
            }
            if self.commit_layout(next) {
                self.reconcile_needed = true;
            } else if let Ok(root) = files::user("widget-data") {
                mypaper_widgets::pick::remove_instance(&root, &copy);
                let _ = self.storage.remove(&copy);
            }
        }
        let _ = ui;
    }
    fn edit_feedback(&mut self, id: &str, message: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.model.notice = Some(message.into());
            entry.model.dirty = true;
        }
    }
    fn add(&mut self, ui: &mut Ui, id: String) {
        if self.layout_error.is_some() {
            return;
        }
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
            let mut next = self.layout.clone();
            next.add(&id, &monitor.device, cell, size);
            if self.commit_layout(next) {
                self.reconcile_needed = true;
                self.poll(ui);
            }
        }
    }
    fn place_edit(
        &mut self,
        id: &str,
        monitor: usize,
        cell: [u32; 2],
        size: [u32; 2],
        resize: bool,
    ) {
        let screen = &self.monitors[monitor];
        let grid = grid_for(&self.layout, screen);
        let Some(instances) = edit::resolve(
            &grid,
            &self.layout.instances,
            id,
            &screen.device,
            cell,
            size,
            resize,
        ) else {
            self.edit_feedback(
                id,
                "Not enough room. Move other widgets or choose a smaller size.",
            );
            return;
        };
        let mut next = self.layout.clone();
        next.instances = instances;
        if self.commit_layout(next) {
            self.reconcile_needed = true;
            if let Some(entry) = self.entries.get_mut(id) {
                entry.model.notice = None;
            }
        }
    }
    fn edit_key(&mut self, id: &str, key: u16) {
        let Some(instance) = self.layout.get(id) else {
            return;
        };
        let monitor = self.entries[id].monitor;
        let grid = grid_for(&self.layout, &self.monitors[monitor]);
        let shift = shift_down();
        let axis = if key == 37 || key == 39 { 0 } else { 1 };
        let delta = if key == 37 || key == 38 { -1 } else { 1 };
        let mut cell = instance.cell;
        let mut size = instance.size;
        if shift {
            size[axis] = size[axis].saturating_add_signed(delta).max(1);
            size = self
                .registry
                .get(&instance.widget)
                .unwrap()
                .manifest
                .clamp_size(size);
        } else {
            cell[axis] = cell[axis].saturating_add_signed(delta);
        }
        let (cell, size) = grid.clamp(cell, size);
        self.place_edit(id, monitor, cell, size, shift);
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
            let (mut cell, size) = grid.clamp(cell, size);
            if drag.resize
                && let Some(original) = self.layout.get(&drag.id)
                && original.monitor == screen.device
            {
                if drag.edges[0] {
                    cell[0] = (original.cell[0] + original.size[0]).saturating_sub(size[0]);
                }
                if drag.edges[1] {
                    cell[1] = (original.cell[1] + original.size[1]).saturating_sub(size[1]);
                }
            }
            let (cell, size) = grid.clamp(cell, size);
            self.place_edit(&drag.id, monitor, cell, size, drag.resize);
        }
        self.reconcile_needed = true;
        for entry in self.entries.values_mut() {
            entry.model.dirty = true;
        }
        self.poll(ui);
    }
    fn commit_layout(&mut self, next: Layout) -> bool {
        if self.layout_error.is_some() {
            return false;
        }
        match next.save(&self.path) {
            Ok(()) => {
                self.layout = next;
                true
            }
            Err(error) => {
                self.layout_error = Some(format!("Could not save layout: {error:#}"));
                self.reconcile_needed = true;
                false
            }
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
                        radius: view::plate_radius(&e.model, e.rect, e.dpi_scale * e.tokens.scale),
                        material: Material {
                            clarity: 1.0
                                - (liquid.tint.unwrap_or(0.06)
                                    * a.get("opacity").and_then(Value::as_f64).unwrap_or(0.46)
                                        as f32
                                    / 0.46)
                                    .clamp(0.0, 1.0),
                            blur: (a.get("blur").and_then(Value::as_f64).unwrap_or(12.0) as f32
                                * liquid.blur.unwrap_or(12.0)
                                / 12.0)
                                * monitor.dpi as f32
                                / 96.0,
                            refraction: liquid.refraction.unwrap_or(0.6)
                                * liquid.intensity.unwrap_or(1.0),
                            highlight: liquid.highlight.unwrap_or(0.55)
                                * liquid.intensity.unwrap_or(1.0),
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

/// Return free startup allocation caches to Windows once providers/fonts have settled. Live
/// widget data and renderer caches remain allocated; this never becomes an idle polling task.
fn release_startup_heap() {
    use windows::Win32::System::Memory::{HeapOptimizeResources, HeapSetInformation};
    use windows::Win32::System::SystemServices::{
        HEAP_OPTIMIZE_RESOURCES_CURRENT_VERSION, HEAP_OPTIMIZE_RESOURCES_INFORMATION,
    };
    let info = HEAP_OPTIMIZE_RESOURCES_INFORMATION {
        Version: HEAP_OPTIMIZE_RESOURCES_CURRENT_VERSION,
        Flags: 0,
    };
    // SAFETY: a documented process-wide LFH cache optimization, with a complete versioned
    // structure. Windows decommits only unused heap memory, without moving live allocations.
    if let Err(error) = unsafe {
        HeapSetInformation(
            None,
            HeapOptimizeResources,
            Some(std::ptr::from_ref(&info).cast()),
            size_of_val(&info),
        )
    } {
        log::debug!("widget startup heap release: {error}");
    }
}

fn widget_surface(topmost: bool, parent: HWND, instance: &str) -> Result<Surface> {
    Surface::new_deferred_accessibility(
        if topmost {
            Kind::Popup { activate: true }
        } else {
            Kind::DesktopChild { parent }
        },
        &format!("mypaper widget {instance}"),
        Role::Pane,
    )
}

/// Timeline and transport changes reuse decoded art and its blurred/tinted material.
fn update_artwork(model: &mut Model, ui: &Ui, data: &Value) {
    if model.data.get("artwork") == data.get("artwork") {
        return;
    }
    let decoded = data
        .get("artwork")
        .and_then(|value| decode_url(ui, value, (256, 256)));
    (model.artwork_backdrop, model.artwork_tint) = decoded
        .as_ref()
        .map_or((None, None), |art| artwork_material(art));
    model.artwork = decoded.map(|art| Arc::new(rounded_square(&art, 0.12)));
}

/// A centred square crop of `image` with its corners cut round (radius = `radius` × side) to
/// transparent, antialiased over one pixel. Covers are drawn rounded this way: a layered
/// window's bitmap op has no clip shape, and this runs once per track, not per frame.
fn rounded_square(image: &mypaper_ui::Image, radius: f32) -> mypaper_ui::Image {
    let side = image.width.min(image.height);
    let (x0, y0) = ((image.width - side) / 2, (image.height - side) / 2);
    let r = side as f32 * radius;
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        let start = (((y0 + y) * image.width + x0) * 4) as usize;
        pixels.extend_from_slice(&image.pixels[start..start + side as usize * 4]);
    }
    let edge = side as f32;
    for y in 0..side {
        for x in 0..side {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (r - px).max(px - (edge - r)).max(0.0);
            let dy = (r - py).max(py - (edge - r)).max(0.0);
            if dx == 0.0 || dy == 0.0 {
                continue;
            }
            // Coverage of the pixel by the corner's circle; premultiplied, so scale all four.
            let coverage = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            let at = ((y * side + x) * 4) as usize;
            for channel in &mut pixels[at..at + 4] {
                *channel = (*channel as f32 * coverage).round() as u8;
            }
        }
    }
    mypaper_ui::Image {
        width: side,
        height: side,
        pixels,
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
    if entry.model.kind == Widget::Clock && entry.model.last_clock.is_none() {
        entry.model.last_clock = Some(now);
    }
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
    let drawn = entry.surface.present_sparse(
        ui,
        (entry.rect.left as i32, entry.rect.top as i32),
        size,
        list,
    )?;
    if !entry.surface.is_visible() {
        entry.surface.show();
        if host.is_some() && entry.model.placed.widget != "media-pill" {
            entry.surface.keep_above_desktop()?;
        }
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
    entry.model.field_keyboard = false;
    if entry.model.kind == Widget::Calendar {
        entry.model.event_draft = None;
        entry.model.task_edit = None;
    }
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
fn palette_accent(config: &Config) -> Color {
    mypaper_theme::locate(Some(&config.theme))
        .ok()
        .and_then(|dir| mypaper_tweaks::palette::read().filter(|p| p.is_for(&dir)))
        .map_or_else(|| look::accent(None), |p| p.palette.accent)
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
        font: TextStyle::new(layer.font.as_deref().unwrap_or(view::DEFAULT_FAMILY), 14.0),
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

fn shift_down() -> bool {
    // SAFETY: a read-only modifier query on the owning input thread.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0 }
}
fn calendar_start(m: &Model) -> u32 {
    match m.str("weekStart", "auto").as_str() {
        "sunday" => 0,
        "monday" => 1,
        "saturday" => 6,
        _ => time::week_start(),
    }
}
/// `m:ss`, for media positions.
fn media_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A tiny box-filtered artwork backdrop and average tint, computed only when artwork changes.
fn artwork_material(image: &mypaper_ui::Image) -> (Option<Arc<mypaper_ui::Image>>, Option<Rgba>) {
    if image.width == 0 || image.height == 0 {
        return (None, None);
    }
    let (width, height) = (16u32.min(image.width), 16u32.min(image.height));
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let mut total = [0u64; 4];
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u64; 4];
            let mut count = 0;
            for py in y * image.height / height..(y + 1) * image.height / height {
                for px in x * image.width / width..(x + 1) * image.width / width {
                    let at = ((py * image.width + px) * 4) as usize;
                    for channel in 0..4 {
                        sum[channel] += u64::from(image.pixels[at + channel]);
                    }
                    count += 1;
                }
            }
            for channel in 0..4 {
                pixels.push((sum[channel] / count.max(1)) as u8);
                total[channel] += sum[channel];
            }
        }
    }
    let alpha = total[3].max(1) as f32;
    let tint = Rgba::new(
        total[2] as f32 / alpha,
        total[1] as f32 / alpha,
        total[0] as f32 / alpha,
        1.0,
    );
    (
        Some(Arc::new(mypaper_ui::Image {
            width,
            height,
            pixels,
        })),
        Some(tint),
    )
}
