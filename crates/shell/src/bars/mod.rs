//! Native top bar and dock. Windows services live in `mypaper-bars`; UI and motion live here.
mod bar;
pub(crate) mod icons;
pub(crate) mod look;
mod maximized;
pub(crate) mod motion;
pub mod plugin_modules;
pub mod plugin_style;
mod preview;
pub(crate) mod services;
mod sprites;
mod styling;
#[cfg(test)]
mod tests;
mod tray_menu;
pub(crate) mod view;

use crate::{flyouts, glass};
use anyhow::Result;
use bar::Bar;
use mypaper_bars::{
    self as providers, BarsConfig, Module, ModuleRef, TaskbarMode,
    actions::{BarKind, Button as TrayButton, Direction, MediaAction, PowerAction},
    config::{self, Action, BuiltinAction, Extra, Pinned, Trigger},
    events::{self, Broadcast},
    style::Family,
    system,
    taskbar::{JournalLease, NativeShell, Taskbar, Wanted},
    trash::{self, TrashWatch},
    tray_data::Click,
};
use mypaper_config::Config;
use mypaper_desktop::{Monitor, fullscreen_app_monitor, monitor_infos, startup};
use mypaper_theme::{Appearance, LoadedTheme};
use mypaper_ui::{
    Button, Entry, Event, Menu, Outcome, Point, Rect, SurfaceId, Ticker, Ui, screen_at,
};
use notify::Watcher;
use services::Services;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use view::Target;
use windows::Win32::{
    Foundation::HANDLE,
    UI::{
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
        WindowsAndMessaging::{IsIconic, MSG},
    },
};

#[derive(Clone)]
enum Command {
    AppActions(Box<(providers::apps::Item, Rect, Monitor)>),
    Activate(isize),
    Close(isize),
    CloseAll(Vec<isize>),
    Launch(String),
    Pin(Pinned),
    Unpin(String),
    EmptyTrash,
}
struct Press {
    bar: usize,
    at: Point,
    target: Target,
    dragging: bool,
}

pub struct Bars {
    bars: Vec<Bar>,
    monitors: Vec<Monitor>,
    services: Services,
    config: BarsConfig,
    taskbar: Taskbar<NativeShell>,
    journal: Option<JournalLease>,
    broadcast: Option<Broadcast>,
    _control: Option<mypaper_desktop::ControlWindow>,
    trash: Option<TrashWatch>,
    ticker: Ticker,
    images: view::Images,
    menu: Option<Menu<Command>>,
    tray_menu: Option<tray_menu::Host>,
    press: Option<Press>,
    watches: Vec<notify::RecommendedWatcher>,
    theme_watch: Option<notify::RecommendedWatcher>,
    appearance: Appearance,
    theme_glass: Option<mypaper_theme::Liquid>,
    system: providers::accent::System,
    published: BTreeMap<String, Vec<glass::Plate>>,
    config_due: Option<Instant>,
    display_due: Option<Instant>,
    reservation_due: Option<Instant>,
    maximized_due: Option<Instant>,
    explorer_due: Option<Instant>,
    rehide_due: Option<Instant>,
    asleep: bool,
    display_off: bool,
    locked: bool,
    fullscreen: Option<String>,
    last_wheel: Option<Instant>,
    preview: Option<preview::Preview>,
    preview_due: Option<(Instant, usize, String)>,
    preview_close: Option<Instant>,
    tooltip: Option<mypaper_ui::Surface>,
    tooltip_due: Option<(Instant, usize, u32)>,
    /// When the shown tooltip started fading in.
    tooltip_fade: Option<Instant>,
    warm_due: Option<(Instant, usize, u32)>,
}

/// Keeps startup failures reversible before `Bars` owns the journal.
pub struct Startup {
    config: Option<BarsConfig>,
}
impl Drop for Startup {
    fn drop(&mut self) {
        if self.config.is_some()
            && let Ok(_lease) = JournalLease::acquire()
        {
            let _ = providers::replay();
        }
    }
}
pub fn prepare() -> Result<Startup> {
    let config = load_config()?;
    startup::ready("shell", "config_ms");
    let startup = Startup {
        config: Some(config),
    };
    Ok(startup)
}

impl Bars {
    /// Present the bars before starting Explorer services, hooks or provider workers.
    pub fn new(ui: &mut Ui, mut startup: Startup) -> Result<Self> {
        let config = startup.config.as_ref().unwrap().clone();
        let taskbar = Taskbar::native()?;
        let services = Services::new()?;
        startup::ready("shell", "services_ms");
        let mut bars = Self {
            bars: vec![],
            monitors: vec![],
            services,
            config,
            taskbar,
            journal: None,
            broadcast: None,
            _control: None,
            trash: None,
            ticker: Ticker::new()?,
            images: Default::default(),
            menu: None,
            tray_menu: None,
            press: None,
            watches: vec![],
            theme_watch: None,
            appearance: Appearance::default(),
            theme_glass: None,
            system: providers::accent::read(),
            published: BTreeMap::new(),
            config_due: None,
            display_due: None,
            reservation_due: None,
            maximized_due: None,
            explorer_due: None,
            rehide_due: None,
            asleep: false,
            display_off: false,
            locked: false,
            fullscreen: None,
            last_wheel: None,
            preview: None,
            preview_due: None,
            preview_close: None,
            tooltip: None,
            tooltip_due: None,
            tooltip_fade: None,
            warm_due: None,
        };
        bars.read_theme();
        startup::ready("shell", "theme_ms");
        bars.reconcile(ui)?;
        startup::ready("shell", "surfaces_ms");
        bars.paint(ui, Instant::now())?;
        startup::ready("shell", "bars_painted_ms");
        // Pool popup HWNDs after first paint so neither bar startup nor a cold click pays for them.
        bars.tray_menu = tray_menu::Host::new()
            .inspect_err(|e| log::warn!("tray menu: {e:#}"))
            .ok();
        bars.menu = Menu::new("Dock commands").ok();
        bars.tooltip = mypaper_ui::Surface::new_deferred_accessibility(
            mypaper_ui::Kind::Popup { activate: false },
            "Module description",
            mypaper_ui::Role::Tooltip,
        )
        .ok();
        if let Some(tip) = &bars.tooltip {
            tip.set_click_through(true);
        }
        bars.journal = Some(JournalLease::acquire()?);
        providers::replay()?;
        startup::ready("shell", "replayed_ms");
        bars.broadcast = Some(Broadcast::new()?);
        bars.services.start();
        bars._control =
            mypaper_desktop::ControlWindow::new(windows::core::w!("mypaper.bars.control"))
                .inspect_err(|e| log::warn!("bar power/session notifications: {e:#}"))
                .ok();
        bars.apply_taskbar();
        for bar in &mut bars.bars {
            bar.reserve();
        }
        bars.schedule_reservations();
        bars.services.refresh_apps();
        bars.subscribe();
        bars.update_items();
        bars.watch();
        startup.config = None;
        Ok(bars)
    }

    pub fn handles(&self) -> Vec<HANDLE> {
        std::iter::once(self.ticker.handle())
            .chain(self.services.handles())
            .collect()
    }

    /// Resolved configuration shared with the shell's panels.
    pub fn config(&self) -> &BarsConfig {
        &self.config
    }
    /// Resolve launcher shortcut pins on the existing STA icon worker, just like dock pins.
    pub fn describe_pin(&mut self, path: &str) -> Option<providers::icons::Described> {
        self.services
            .icons
            .get(&providers::icons::Source::Path(path.to_owned()))
            .cloned()
    }
    pub fn can_open_flyouts(&self) -> bool {
        !self.asleep && self.bars.iter().any(Bar::shows_content)
    }
    pub fn external_foreground(&self) -> isize {
        self.services.model.foreground.unwrap_or(0)
    }
    pub fn timeout(&self) -> u32 {
        [
            self.config_due,
            self.display_due,
            self.reservation_due,
            self.maximized_due,
            self.explorer_due,
            self.rehide_due,
            self.services.deadline(),
            self.preview_due.as_ref().map(|p| p.0),
            self.preview_close,
            self.tooltip_due.map(|t| t.0),
            self.warm_due.map(|t| t.0),
            self.bars.iter().filter_map(|b| b.hide_at).min(),
            self.bars
                .iter()
                .filter_map(|b| b.maximized.deadline())
                .min(),
        ]
        .into_iter()
        .flatten()
        .min()
        .map_or(u32::MAX, |at| {
            services::wait_ms(at.saturating_duration_since(Instant::now()))
        })
    }
    /// Time spent in each part of [`tick`](Self::tick) since the last call (diagnostics).
    pub fn take_tick_costs() -> Vec<(&'static str, Duration)> {
        TICK_COSTS.with_borrow_mut(std::mem::take)
    }
    /// Cumulative stats samples, displayed changes, sampler microseconds and all bar repaints.
    pub fn stats_counts(&self) -> (u64, u64, u64, u64) {
        let (samples, changed, micros) = self.services.stats_costs;
        (samples, changed, micros, bar::total_repaints())
    }
    /// Whether `hwnd` is one of the bars' own windows (a bar, an island or the edge strip). The
    /// shell loop uses it to tell pointer moves over the bars and dock from everything else.
    pub fn owns_hwnd(&self, hwnd: windows::Win32::Foundation::HWND) -> bool {
        self.bars.iter().any(|b| b.owns_hwnd(hwnd))
    }
    /// Which deadline [`timeout`](Self::timeout) waits for, for the shell loop's wake counts.
    pub fn timeout_source(&self) -> &'static str {
        [
            (self.config_due, "bars.config"),
            (self.display_due, "bars.display"),
            (self.reservation_due, "bars.reservation"),
            (self.maximized_due, "bars.maximized_settle"),
            (self.explorer_due, "bars.explorer"),
            (self.rehide_due, "bars.rehide"),
            (self.services.deadline(), "bars.services"),
            (self.preview_due.as_ref().map(|p| p.0), "bars.preview"),
            (self.preview_close, "bars.preview_close"),
            (self.tooltip_due.map(|t| t.0), "bars.tooltip"),
            (self.warm_due.map(|t| t.0), "bars.warm"),
            (
                self.bars.iter().filter_map(|b| b.hide_at).min(),
                "bars.hide",
            ),
            (
                self.bars
                    .iter()
                    .filter_map(|b| b.maximized.deadline())
                    .min(),
                "bars.maximized",
            ),
        ]
        .into_iter()
        .filter_map(|(at, name)| Some((at?, name)))
        .min_by_key(|d| d.0)
        .map_or("bars", |d| d.1)
    }
    pub fn config_changed(&mut self) {
        self.config_due = Some(Instant::now() + Duration::from_millis(100));
    }

    /// Thread messages from the services' hidden broadcast/reservation windows.
    pub fn message(&mut self, msg: &MSG) -> bool {
        if !msg.hwnd.is_invalid() {
            return false;
        }
        let now = Instant::now();
        match msg.message {
            providers::wake::WM_WAKE => {}
            events::APPBAR => {
                if msg.wParam.0 as u32 == windows::Win32::UI::Shell::ABN_POSCHANGED {
                    self.display_due
                        .get_or_insert(now + Duration::from_millis(50));
                }
            }
            events::DISPLAY => {
                self.display_due
                    .get_or_insert(now + Duration::from_millis(150));
            }
            events::EXPLORER => {
                if !providers::tray_host::own_broadcast() {
                    self.explorer_due = Some(now + Duration::from_secs(1));
                }
            }
            events::SETTINGS => {
                self.system = providers::accent::read();
                self.config_changed();
                crate::inbox::post(crate::inbox::WM_CONFIG, 0);
            }
            events::TIME => self.services.clock_due = Some(now),
            events::POWER => {
                self.services.model.battery = system::battery();
                self.dirty();
            }
            events::TRASH => {
                self.services.model.trash_full = trash::full();
                self.dirty();
            }
            events::SHELL => {
                if msg.wParam.0 == 0x8006 {
                    self.services.attention.insert(msg.lParam.0);
                    self.update_items();
                    for b in &mut self.bars {
                        let keys: Vec<_> = b
                            .items
                            .iter()
                            .filter(|i| i.windows.iter().any(|w| w.hwnd == msg.lParam.0))
                            .map(|i| i.key.clone())
                            .collect();
                        for key in keys {
                            b.animate_item(key, false, now);
                        }
                    }
                }
                if msg.wParam.0 == 8 {
                    self.services.refresh_apps();
                    self.dirty();
                }
            }
            _ => return false,
        }
        true
    }

    pub fn fact(&mut self, wparam: usize, lparam: isize) {
        use mypaper_desktop::hold::Fact;
        match Fact::from_wparam(wparam) {
            Some(Fact::DisplayOff) => self.display_off = lparam != 0,
            Some(Fact::SessionLocked) => self.locked = lparam != 0,
            Some(Fact::OnBattery) if self.config.performance == config::Performance::Auto => {
                self.config_changed();
                crate::inbox::post(crate::inbox::WM_CONFIG, 0);
            }
            _ => {}
        }
        self.asleep = self.display_off || self.locked;
    }

    /// All drawing is gated by data changes or active animation. The ticker cancels at rest.
    pub fn tick(&mut self, ui: &mut Ui) {
        let mut at = Instant::now();
        if let Some(menu) = &mut self.tray_menu {
            menu.process(ui);
        }
        at = tick_spent("tray_menu", at);
        let now = Instant::now();
        self.hover_tick(ui, now);
        at = tick_spent("hover", at);
        let changes = self.services.tick(now);
        let max_config = self.config_due.is_some_and(|at| at <= now);
        let max_display = self.display_due.is_some_and(|at| at <= now);
        at = tick_spent("services", at);
        if changes.config || changes.theme {
            self.config_changed();
            if load_config().is_ok() {
                crate::inbox::post(crate::inbox::WM_CONFIG, 0);
            }
        }
        // A plugin patched or reset a bar's style: restyle in place like a settings change.
        if plugin_style::take_changed() {
            self.config_due.get_or_insert(now);
        }
        // A plugin module drew a new view: repaint (the damage diff keeps it to that item).
        if plugin_modules::take_changed() {
            self.dirty();
        }
        if services::take_due(&mut self.config_due, now) {
            match load_config() {
                Ok(config) => {
                    self.config = config;
                    self.read_theme();
                    if let Err(e) = self.reconcile(ui) {
                        log::error!("bars: {e:#}");
                    }
                    self.subscribe();
                    self.update_items();
                }
                Err(e) => log::warn!("bars.json: {e:#}; keeping the last valid config"),
            }
        }
        if services::take_due(&mut self.explorer_due, now) {
            for b in &mut self.bars {
                if let Some(r) = &mut b.reservation {
                    r.reregister();
                }
            }
            self.services.tray_host.explorer_restarted();
            self.apply_taskbar();
            self.display_due = Some(now);
        }
        if services::take_due(&mut self.display_due, now) {
            let monitors = monitor_infos();
            let changed = monitors.len() != self.monitors.len()
                || monitors.iter().any(|m| {
                    !self
                        .monitors
                        .iter()
                        .any(|old| old.device == m.device && old.rect == m.rect && old.dpi == m.dpi)
                });
            if changed {
                if let Err(e) = self.reconcile(ui) {
                    log::error!("display layout: {e:#}");
                }
                self.update_items();
            } else {
                for b in &mut self.bars {
                    if let Some(m) = monitors.iter().find(|m| m.device == b.monitor.device) {
                        b.monitor = m.clone();
                    }
                    b.reserve();
                }
            }
            self.schedule_reservations();
        }
        if services::take_due(&mut self.reservation_due, now) {
            for bar in &mut self.bars {
                if !bar.reserved {
                    bar.reserve();
                }
            }
            self.schedule_reservations();
        }
        at = tick_spent("due", at);
        if changes.data {
            self.update_items();
            self.dirty();
        }
        if changes.stats {
            for bar in &mut self.bars {
                if bar.topics().contains(&providers::actions::Topic::Stats) {
                    bar.dirty = true;
                }
            }
        }
        at = tick_spent("items", at);
        if changes.data || changes.moved || changes.windows {
            let windows: Vec<_> = self
                .services
                .apps
                .groups
                .iter()
                .flat_map(|g| &g.windows)
                .filter(|w| !w.minimized)
                .filter_map(|w| {
                    let mut rect = windows::Win32::Foundation::RECT::default();
                    // SAFETY: enumerated application window; a stale handle fails harmlessly.
                    unsafe {
                        use windows::Win32::Graphics::Dwm::{
                            DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
                        };
                        if DwmGetWindowAttribute(
                            system::hwnd(w.hwnd),
                            DWMWA_EXTENDED_FRAME_BOUNDS,
                            (&raw mut rect).cast(),
                            size_of_val(&rect) as u32,
                        )
                        .is_err()
                        {
                            windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                                system::hwnd(w.hwnd),
                                &mut rect,
                            )
                            .ok()?;
                        }
                    }
                    Some(Rect::new(
                        rect.left as f32,
                        rect.top as f32,
                        (rect.right - rect.left) as f32,
                        (rect.bottom - rect.top) as f32,
                    ))
                })
                .collect();
            for b in &mut self.bars {
                b.windows(&windows);
            }
            let fullscreen = fullscreen_app_monitor()
                .and_then(|rect| monitor_infos().into_iter().find(|m| m.rect == rect))
                .map(|m| m.device);
            if fullscreen != self.fullscreen {
                self.fullscreen = fullscreen;
                self.subscribe();
            }
        }
        if changes.taskbar && self.config.taskbar.mode == TaskbarMode::Hide {
            self.rehide_due
                .get_or_insert(now + Duration::from_millis(50));
        }
        if services::take_due(&mut self.rehide_due, now)
            && self.config.taskbar.mode == TaskbarMode::Hide
        {
            providers::taskbar::hide_visible();
        }
        let maximized_event = changes.windows || changes.moved || max_config || max_display;
        let maximized_settled = services::take_due(&mut self.maximized_due, now);
        if maximized_event {
            // One follow-up snapshot also covers focus/maximize arriving before the shared
            // source has attached its location hook to the newly foreground process.
            self.maximized_due
                .get_or_insert(now + Duration::from_millis(150));
        }
        if maximized_event || maximized_settled {
            // Focus and monitor changes invalidate every monitor's independent z-order decision.
            // Fullscreen visibility is handled separately below.
            let windows = maximized::ZOrder::read();
            for b in &mut self.bars {
                let hide = b.hide_when_maximized() && windows.hides(b.monitor.rect);
                b.maximized.observe(hide, now);
            }
        }
        for b in &mut self.bars {
            if let Err(e) = b.tick_maximized(now) {
                log::warn!("maximized auto-hide: {e:#}");
            }
        }
        at = tick_spent("windows", at);
        let mut visibility_changed = false;
        for b in &mut self.bars {
            let hidden = self.asleep || self.fullscreen.as_deref() == Some(&b.monitor.device);
            if b.hidden != hidden {
                b.hidden = hidden;
                b.dirty = true;
                visibility_changed = true;
            }
        }
        if visibility_changed {
            self.subscribe();
        }
        if services::take_due(&mut self.preview_close, now) {
            self.preview = None;
        }
        if self.preview_due.as_ref().is_some_and(|p| p.0 <= now) {
            let (_, index, key) = self.preview_due.take().unwrap();
            self.show_preview(ui, index, &key);
        }
        at = tick_spent("preview", at);
        if let Err(e) = self.paint(ui, now) {
            log::error!("painting bars: {e:#}");
        }
        tick_spent("paint", at);
    }

    /// Shows app `key`'s window previews over its icon on bar `index`: swapped into the open
    /// popup at once, else in a new one. Cancels the pending open/close and hides the tooltip.
    fn show_preview(&mut self, ui: &mut Ui, index: usize, key: &str) {
        self.preview_due = None;
        self.preview_close = None;
        self.tooltip_due = None;
        if let Some(tip) = &mut self.tooltip {
            tip.hide();
            tip.release();
        }
        let Some(b) = self.bars.get(index).filter(|b| !b.hidden) else {
            return;
        };
        let Some((i, app)) = b
            .frame
            .targets
            .iter()
            .enumerate()
            .find_map(|(i, t)| match t {
                Target::Dock(app) if app.key == key => Some((i, app.clone())),
                _ => None,
            })
        else {
            return;
        };
        let Some(hit) = b.frame.list.hits.iter().find(|h| h.id == i as u32 + 1) else {
            return;
        };
        let origin = b.origin();
        let anchor = hit.rect.offset(origin.0 as f32, origin.1 as f32);
        let (tokens, scale, vertical) = (b.tokens.clone(), b.scale(), !b.horizontal());
        let ink = tokens.theme.color.label;
        let icon = view::app_icon(
            ui,
            &mut self.images,
            &app,
            ink,
            (16.0 * scale).round() as u32,
        );
        let shown = if let Some(preview) = &mut self.preview {
            preview.switch(ui, app, anchor, icon)
        } else {
            preview::Preview::open(ui, app, anchor, &tokens, scale, vertical, icon)
                .map(|p| self.preview = Some(p))
        };
        if let Err(e) = shown {
            log::warn!("dock previews: {e:#}");
            self.preview = None;
        }
    }

    /// Delayed hover work has no recurring timer and never runs on pointer-down.
    fn hover_tick(&mut self, ui: &mut Ui, now: Instant) {
        if self.warm_due.is_some_and(|t| t.0 <= now) {
            let (_, index, hit) = self.warm_due.take().unwrap();
            if let Some(b) = self
                .bars
                .get(index)
                .filter(|b| b.hover == Some(hit) && !b.hidden && b.tokens.material.is_some())
            {
                let origin = b.origin();
                if self.tooltip.is_none() {
                    self.tooltip = mypaper_ui::Surface::new_deferred_accessibility(
                        mypaper_ui::Kind::Popup { activate: false },
                        "Module description",
                        mypaper_ui::Role::Tooltip,
                    )
                    .ok();
                    if let Some(tip) = &self.tooltip {
                        tip.set_click_through(true);
                    }
                }
                if let Some(tip) = &self.tooltip {
                    crate::overlay_client::warm(
                        tip.hwnd(),
                        mypaper_ui::glass::Shape {
                            x: origin.0,
                            y: origin.1,
                            width: 1,
                            height: 1,
                        },
                    );
                }
            }
        }
        if self.tooltip_due.is_some_and(|t| t.0 <= now) {
            let (_, index, hit) = self.tooltip_due.take().unwrap();
            if let Some(b) = self
                .bars
                .get(index)
                .filter(|b| b.hover == Some(hit) && !b.hidden && self.preview.is_none())
                && let Some(item) = b.frame.list.hits.iter().find(|h| h.id == hit)
            {
                let label: String = item.label.chars().take(160).collect();
                let origin = b.origin();
                let anchor = item.rect.offset(origin.0 as f32, origin.1 as f32);
                let work = b.monitor.work;
                let bounds = Rect::new(
                    work.x as f32,
                    work.y as f32,
                    work.width as f32,
                    work.height as f32,
                );
                // The kit's tooltip, placed in screen px (below the item, or above it when
                // there is no room), then cut out with its shadow as the popup.
                let input = mypaper_ui::Interaction::default();
                let mut f =
                    mypaper_ui::Frame::new(&mut ui.text, &b.tokens.theme, b.scale(), &input);
                let body = f.tooltip(anchor, &label, bounds);
                let list = f.list;
                let reach = list
                    .ops
                    .iter()
                    .map(mypaper_ui::Op::bounds)
                    .fold(body, Rect::union);
                let (x, y) = (reach.left.floor(), reach.top.floor());
                let size = (
                    (reach.right.ceil() - x) as u32,
                    (reach.bottom.ceil() - y) as u32,
                );
                let list = list.translated(-x, -y);
                if self.tooltip.is_none() {
                    self.tooltip = mypaper_ui::Surface::new_deferred_accessibility(
                        mypaper_ui::Kind::Popup { activate: false },
                        "Module description",
                        mypaper_ui::Role::Tooltip,
                    )
                    .ok();
                }
                if let Some(tip) = &mut self.tooltip {
                    tip.set_click_through(true);
                    // Fades in (see `paint`), as the Dock's names do.
                    let _ = tip.set_alpha(0);
                    if tip.present(ui, (x as i32, y as i32), size, list).is_ok() {
                        tip.show();
                        self.tooltip_fade = Some(now);
                    }
                }
            }
        }
    }

    fn paint(&mut self, ui: &mut Ui, now: Instant) -> Result<()> {
        let menu = self.menu.as_ref().is_some_and(Menu::is_open) || self.preview.is_some();
        let before: std::collections::HashSet<_> = self
            .bars
            .iter()
            .filter(|b| b.shows_content())
            .map(|b| b.surface.id())
            .collect();
        let mut moving = false;
        for b in &mut self.bars {
            moving |= b.paint(
                ui,
                &self.services.model,
                &mut self.images,
                now,
                menu || self.press.is_some(),
            )?;
        }
        if let Some(at) = self.tooltip_fade
            && let Some(tip) = &mut self.tooltip
        {
            let t = (now.saturating_duration_since(at).as_secs_f32() / 0.12).min(1.0);
            let _ = tip.set_alpha((t * (2.0 - t) * 255.0).round() as u8);
            if t >= 1.0 {
                self.tooltip_fade = None;
            } else {
                moving = true;
            }
        }
        self.ticker.schedule(moving);
        let after: std::collections::HashSet<_> = self
            .bars
            .iter()
            .filter(|b| b.shows_content())
            .map(|b| b.surface.id())
            .collect();
        if before != after {
            self.subscribe();
        }
        for monitor in &self.monitors {
            let next = self.plates(monitor);
            if self.published.get(&monitor.device) != Some(&next) {
                glass::publish(glass::Source::Bars, monitor, &next);
                self.published.insert(monitor.device.clone(), next);
            }
        }
        Ok(())
    }

    /// The tray icons, pinned or not (the tray overflow flyout lists the unpinned ones).
    pub fn tray(&self) -> &[providers::tray_data::TrayItem] {
        &self.services.model.tray
    }

    /// Clicks tray icon `id` from outside a bar (the overflow flyout) as a bar click would;
    /// `anchor` is the icon's screen rectangle.
    pub fn click_tray(&self, id: &str, button: Button, point: (i32, i32), anchor: Rect) {
        if self.services.tray_host.active() {
            let click = match button {
                Button::Right => Click::Right,
                Button::Middle => Click::Middle,
                Button::Left => Click::Left,
            };
            let rect = [anchor.left, anchor.top, anchor.width(), anchor.height()].map(|v| v as i32);
            self.services
                .tray_host
                .click(id.into(), click, point, Some(rect));
        } else {
            let button = match button {
                Button::Right => TrayButton::Right,
                Button::Middle => TrayButton::Middle,
                Button::Left => TrayButton::Left,
            };
            self.services.tray.click(id.into(), button);
        }
    }
    pub fn double_click_tray(&self, id: &str, point: (i32, i32)) {
        if self.services.tray_host.active() {
            self.services
                .tray_host
                .click(id.to_owned(), Click::Double, point, None);
        } else {
            self.services.tray.click(id.to_owned(), TrayButton::Double);
        }
    }

    /// Current bar plates for the shell compositor. N4 can combine this with widget plates
    /// before calling the single monitor-replacement publisher.
    pub fn plates(&self, monitor: &Monitor) -> Vec<glass::Plate> {
        self.bars
            .iter()
            .filter(|b| b.monitor.device == monitor.device)
            .flat_map(Bar::plates)
            .collect()
    }

    pub fn event(&mut self, ui: &mut Ui, id: SurfaceId, event: &Event) {
        if let Some(menu) = &mut self.tray_menu
            && menu.event(ui, id, event)
        {
            return;
        }
        if let Some(preview) = &self.preview
            && preview.owns(id)
        {
            match event {
                Event::Wheel(_, delta) => {
                    if let Some(preview) = &mut self.preview {
                        preview.scroll(ui, *delta);
                    }
                }
                Event::Move(p) => {
                    self.preview_close = None;
                    if let Some(preview) = &mut self.preview {
                        preview.hover(ui, Some(p.at));
                    }
                }
                Event::Leave => {
                    if let Some(preview) = &mut self.preview {
                        preview.hover(ui, None);
                    }
                    self.preview_close = Some(Instant::now() + Duration::from_millis(280))
                }
                Event::Release(p, Button::Left) => {
                    let hit = preview.hit(p.at);
                    self.preview = None;
                    if let Some((hwnd, close)) = hit {
                        if close {
                            system::close(system::hwnd(hwnd));
                        } else {
                            system::activate(system::hwnd(hwnd));
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(menu) = &mut self.menu
            && menu.owns(id)
        {
            if let Some(Outcome::Chosen(command)) = menu.handle(ui, id, event) {
                self.command(command);
            }
            return;
        }
        let now = Instant::now();
        for b in &mut self.bars {
            if b.edge_surface.as_ref().is_some_and(|s| s.id() == id)
                && matches!(event, Event::Move(_) | Event::Press(..))
            {
                b.reveal(now);
                return;
            }
        }
        let Some(index) = self.bars.iter().position(|b| b.owns(id)) else {
            return;
        };
        let map = |mut p: mypaper_ui::Pointer| {
            p.at = self.bars[index].input_point(id, p.at);
            p
        };
        let event = match *event {
            Event::Move(p) => Event::Move(map(p)),
            Event::Press(p, b) => Event::Press(map(p), b),
            Event::Release(p, b) => Event::Release(map(p), b),
            Event::DoubleClick(p, b) => Event::DoubleClick(map(p), b),
            Event::Wheel(p, d) => Event::Wheel(map(p), d),
            e => e,
        };
        let event = &event;
        if matches!(
            event,
            Event::Leave | Event::Press(..) | Event::Key(_) | Event::Deactivated
        ) {
            self.tooltip_due = None;
            self.warm_due = None;
            if let Some(tip) = &mut self.tooltip {
                tip.hide();
                tip.release();
            }
        }
        match event {
            Event::Key(key) => {
                use windows::Win32::UI::Input::KeyboardAndMouse::*;
                let b = &mut self.bars[index];
                if [VK_TAB.0, VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(key) {
                    // SAFETY: read-only keyboard modifier query on the input thread.
                    let back = *key == VK_LEFT.0
                        || *key == VK_UP.0
                        || (*key == VK_TAB.0 && unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0);
                    b.focus = mypaper_ui::focus::step(
                        &b.frame.list.hits,
                        b.focus,
                        if back { -1 } else { 1 },
                    );
                    b.dirty = true;
                } else if [VK_RETURN.0, VK_SPACE.0].contains(key) {
                    if let Some(target) = b
                        .focus
                        .and_then(|id| b.frame.targets.get(id as usize - 1))
                        .cloned()
                    {
                        let point = b
                            .focus
                            .and_then(|id| b.frame.list.hits.iter().find(|h| h.id == id))
                            .map_or(b.origin(), |hit| {
                                let o = b.origin();
                                (
                                    o.0 + ((hit.rect.left + hit.rect.right) / 2.0) as i32,
                                    o.1 + ((hit.rect.top + hit.rect.bottom) / 2.0) as i32,
                                )
                            });
                        self.activate(ui, index, target, Trigger::Left, point);
                    }
                } else if *key == VK_ESCAPE.0 {
                    b.focus = None;
                    b.dirty = true;
                }
            }
            Event::Focus(hit) => {
                let b = &mut self.bars[index];
                b.focus = Some(*hit);
                b.dirty = true;
                b.surface.take_focus();
            }
            Event::Deactivated => {
                self.bars[index].focus = None;
                self.bars[index].dirty = true;
            }
            Event::DpiChanged(_) => self.display_due = Some(now),
            Event::Leave => {
                // Only dock drags hold capture; any other press dragged off the bar ends here
                // (its release goes to another window).
                if self
                    .press
                    .as_ref()
                    .is_some_and(|p| !matches!(p.target, Target::Dock(_)))
                {
                    self.press = None;
                }
                if self.press.is_none() {
                    self.bars[index].leave(now);
                    self.preview_due = None;
                    self.preview_close = Some(now + Duration::from_millis(280));
                }
            }
            Event::Move(p) => {
                let b = &mut self.bars[index];
                b.pointer = Some(p.at);
                b.hide_at = None;
                let hit = b.frame.target(p.at).map(|(id, _)| id);
                if hit != b.hover {
                    b.hover = hit;
                    b.dirty = true;
                    if let Some(tip) = &mut self.tooltip {
                        tip.hide();
                        tip.release();
                    }
                    // A running app's previews name its windows: no tooltip for it, and none
                    // while previews are open (it would cover them).
                    let previewed = self.preview.is_some()
                        || hit
                            .and_then(|h| b.frame.targets.get(h as usize - 1))
                            .is_some_and(|t| {
                                b.config.dock.preview.enabled
                                    && matches!(t, Target::Dock(app) if !app.windows.is_empty())
                            });
                    self.tooltip_due = hit
                        .filter(|_| !previewed)
                        .map(|h| (now + Duration::from_millis(450), index, h));
                    self.warm_due = hit
                        .filter(|h| {
                            b.frame
                                .targets
                                .get(*h as usize - 1)
                                .is_some_and(|target| opens_flyout(target, &b.config))
                        })
                        .map(|h| (now + Duration::from_millis(60), index, h));
                }
                b.zoom_to(Some(p.at), now);
                let delay = Duration::from_millis(b.config.dock.preview.delay as u64);
                if b.config.dock.preview.enabled
                    && self.press.is_none()
                    && !self.menu.as_ref().is_some_and(Menu::is_open)
                {
                    if let Some((_, Target::Dock(app))) = b.frame.target(p.at) {
                        self.preview_close = None;
                        if app.windows.is_empty() {
                            // An app with no windows here has no previews: the previous app's,
                            // pending or open, don't show over it (they close as off the icons).
                            self.preview_due = None;
                            if self.preview.is_some() {
                                self.preview_close = Some(now + Duration::from_millis(280));
                            }
                        } else if self.preview.as_ref().is_some_and(|v| v.key != app.key) {
                            // Open previews follow the pointer to the next app at once, in the
                            // same popup.
                            self.show_preview(ui, index, &app.key);
                        } else if self.preview.is_none()
                            && self.preview_due.as_ref().is_none_or(|v| v.2 != app.key)
                        {
                            self.preview_due = Some((now + delay, index, app.key));
                        }
                    } else {
                        self.preview_due = None;
                        self.preview_close = Some(now + Duration::from_millis(280));
                    }
                }
                if let Some(press) = &mut self.press {
                    press.dragging |= (press.at.x - p.at.x).hypot(press.at.y - p.at.y) > 6.0;
                }
            }
            Event::DoubleClick(p, Button::Left) => {
                if let Some((_, Target::Tray(tray))) = self.bars[index].frame.target(p.at)
                    && tray != providers::tray::OVERFLOW
                {
                    if self.services.tray_host.active() {
                        self.services
                            .tray_host
                            .click(tray, Click::Double, p.screen, None);
                    } else {
                        self.services.tray.click(tray, TrayButton::Double);
                    }
                    self.press = None;
                }
            }
            Event::Press(p, Button::Left) => {
                if let Some((hit, target)) = self.bars[index].frame.target(p.at) {
                    // Shown pressed in this same pass. The bar never takes the foreground on
                    // a click (the app in front keeps it, as with the macOS menu bar); a
                    // flyout it opens takes it itself.
                    self.bars[index].pressed = Some(hit);
                    self.bars[index].hover = Some(hit);
                    self.bars[index].focus = None;
                    self.bars[index].dirty = true;
                    if matches!(target, Target::Dock(_)) {
                        // SAFETY: capture belongs to this UI thread and is released on up/loss.
                        unsafe {
                            SetCapture(self.bars[index].surface.hwnd());
                        }
                    }
                    self.press = Some(Press {
                        bar: index,
                        at: p.at,
                        target,
                        dragging: false,
                    });
                }
            }
            Event::Release(p, button) => {
                for b in &mut self.bars {
                    if b.pressed.take().is_some() {
                        b.dirty = true;
                    }
                }
                let target = if *button == Button::Left {
                    let press = self.press.take();
                    // SAFETY: releasing capture on the same UI thread.
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    if let Some(press) = press.filter(|p| p.bar == index) {
                        if press.dragging {
                            self.drop_item(index, press.target, p.at);
                            None
                        } else {
                            Some(press.target)
                        }
                    } else {
                        None
                    }
                } else {
                    self.bars[index].frame.target(p.at).map(|(_, t)| t)
                };
                if let Some(target) = target {
                    self.activate(ui, index, target, trigger(*button), p.screen);
                }
            }
            Event::CaptureLost => {
                self.press = None;
                if self.bars[index].pressed.take().is_some() {
                    self.bars[index].dirty = true;
                }
            }
            Event::Wheel(p, delta) => {
                if *delta != 0
                    && self
                        .last_wheel
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(120))
                {
                    self.last_wheel = Some(now);
                    if let Some((_, t)) = self.bars[index].frame.target(p.at) {
                        self.activate(
                            ui,
                            index,
                            t,
                            if *delta > 0 {
                                Trigger::WheelUp
                            } else {
                                Trigger::WheelDown
                            },
                            p.screen,
                        );
                    }
                }
            }
            Event::Invoke(hit) => {
                if let Some(t) = self.bars[index]
                    .frame
                    .targets
                    .get(*hit as usize - 1)
                    .cloned()
                {
                    let b = &self.bars[index];
                    let p =
                        b.frame
                            .list
                            .hits
                            .iter()
                            .find(|h| h.id == *hit)
                            .map_or(b.origin(), |h| {
                                let o = b.origin();
                                (
                                    o.0 + ((h.rect.left + h.rect.right) / 2.0) as i32,
                                    o.1 + ((h.rect.top + h.rect.bottom) / 2.0) as i32,
                                )
                            });
                    self.activate(ui, index, t, Trigger::Left, p);
                }
            }
            _ => {}
        }
    }

    fn activate(
        &mut self,
        ui: &mut Ui,
        index: usize,
        target: Target,
        input: Trigger,
        point: (i32, i32),
    ) {
        let b = &self.bars[index];
        let anchor = b
            .frame
            .list
            .hits
            .iter()
            .find(|h| {
                h.rect.contains(Point::new(
                    (point.0 - b.origin().0) as f32,
                    (point.1 - b.origin().1) as f32,
                ))
            })
            .map_or(Rect::new(point.0 as f32, point.1 as f32, 1.0, 1.0), |h| {
                h.rect.offset(b.origin().0 as f32, b.origin().1 as f32)
            });
        let monitor = b.monitor.clone();
        let module = match &target {
            Target::Module(m) => Some(m.clone()),
            Target::Tray(_) => Some(ModuleRef::Builtin(Module::Tray)),
            Target::Workspace(_) => Some(ModuleRef::Builtin(Module::Workspaces)),
            _ => None,
        };
        if let Some(action) = module
            .as_ref()
            .and_then(|m| b.config.modules.get(m))
            .and_then(|o| o.click.get(input))
            .filter(|a| **a != Action::Builtin(BuiltinAction::Default))
            .cloned()
        {
            self.action(action, anchor, &monitor);
            return;
        }
        match target {
            Target::Module(module) => {
                let action = b
                    .config
                    .modules
                    .get(&module)
                    .and_then(|o| o.click.get(input))
                    .cloned();
                if let Some(action) =
                    action.filter(|a| *a != Action::Builtin(BuiltinAction::Default))
                {
                    self.action(action, anchor, &monitor);
                    return;
                }
                let module = match module {
                    ModuleRef::Builtin(module) => module,
                    ModuleRef::Custom(id) => {
                        match plugin_modules::panel_of(&id).filter(|_| input == Trigger::Left) {
                            Some(panel) => flyouts::open(
                                flyouts::Kind::Plugin(plugin_modules::panel_number(&panel)),
                                anchor,
                                &monitor,
                            ),
                            None => {
                                plugin_modules::click(&id);
                            }
                        }
                        return;
                    }
                    ModuleRef::Break => return,
                };
                let grouped = b.tokens.family == Family::Fluent;
                if input == Trigger::Left
                    && crate::integrations::module_click(
                        &ModuleRef::Builtin(module),
                        anchor,
                        &monitor,
                    )
                {
                    return;
                }
                let flyout = match (module, input) {
                    (Module::Start, Trigger::Left) => Some(flyouts::Kind::Launcher),
                    (Module::Clock | Module::Notifications, Trigger::Left) => {
                        Some(flyouts::Kind::Notifications)
                    }
                    (Module::Quick, Trigger::Left) => Some(flyouts::Kind::Quick),
                    (Module::Network, Trigger::Left) => Some(if grouped {
                        flyouts::Kind::Quick
                    } else {
                        flyouts::Kind::Wifi
                    }),
                    (Module::Volume, Trigger::Left) => Some(if grouped {
                        flyouts::Kind::Quick
                    } else {
                        flyouts::Kind::Mixer
                    }),
                    (Module::Battery, Trigger::Left) if grouped => Some(flyouts::Kind::Quick),
                    (Module::Power, Trigger::Left) => Some(flyouts::Kind::Power),
                    (Module::Language, Trigger::Left) => Some(flyouts::Kind::Language),
                    _ => None,
                };
                if let Some(kind) = flyout {
                    flyouts::open(kind, anchor, &monitor);
                    return;
                }
                match (module, input) {
                    (Module::Search, Trigger::Left) => system::search(),
                    (Module::Stats, Trigger::Left) => system::open("taskmgr.exe".into()),
                    (Module::Battery, Trigger::Left) => {
                        system::open("ms-settings:batterysaver".into())
                    }
                    (Module::Media, Trigger::Left) => {
                        self.services.media.control(MediaAction::Toggle)
                    }
                    (Module::Media, Trigger::WheelUp) => {
                        self.services.media.control(MediaAction::Previous)
                    }
                    (Module::Media, Trigger::WheelDown) => {
                        self.services.media.control(MediaAction::Next)
                    }
                    (Module::Volume, Trigger::Middle) => self.volume(None, true),
                    (Module::Volume, Trigger::WheelUp) => self.volume(Some(0.04), false),
                    (Module::Volume, Trigger::WheelDown) => self.volume(Some(-0.04), false),
                    _ => {}
                }
            }
            Target::Workspace(i) if input == Trigger::Left => {
                let current = self.services.model.workspaces.current;
                for _ in 0..current.abs_diff(i) {
                    system::switch_desktop(if i > current {
                        Direction::Right
                    } else {
                        Direction::Left
                    });
                }
            }
            Target::Tray(id) => {
                if !matches!(input, Trigger::Left | Trigger::Middle | Trigger::Right) {
                    return;
                }
                if id == providers::tray::OVERFLOW {
                    if input == Trigger::Left {
                        flyouts::open(flyouts::Kind::TrayOverflow, anchor, &monitor);
                    }
                } else if self.services.tray_host.active() {
                    self.services.tray_host.click(
                        id,
                        match input {
                            Trigger::Right => Click::Right,
                            Trigger::Middle => Click::Middle,
                            _ => Click::Left,
                        },
                        point,
                        Some([
                            anchor.left as i32,
                            anchor.top as i32,
                            anchor.width() as i32,
                            anchor.height() as i32,
                        ]),
                    );
                } else {
                    self.services.tray.click(
                        id,
                        match input {
                            Trigger::Right => TrayButton::Right,
                            Trigger::Middle => TrayButton::Middle,
                            _ => TrayButton::Left,
                        },
                    );
                }
            }
            Target::Dock(item) => {
                if (input == Trigger::Left && item.windows.is_empty()) || input == Trigger::Middle {
                    self.bars[index].animate_item(item.key.clone(), true, Instant::now());
                }
                self.preview = None;
                self.preview_due = None;
                match input {
                    Trigger::Left => {
                        if let Some(w) = item.windows.first() {
                            // SAFETY: read-only query on an enumerated window (a stale handle fails).
                            if w.active && !unsafe { IsIconic(system::hwnd(w.hwnd)) }.as_bool() {
                                system::minimize(system::hwnd(w.hwnd));
                                if let Some(next) = providers::apps::front_window(Some(w.hwnd)) {
                                    system::activate(system::hwnd(next));
                                }
                            } else {
                                system::activate(system::hwnd(w.hwnd));
                            }
                        } else {
                            system::open(item.path);
                        }
                        self.services.refresh_apps();
                        self.update_items();
                    }
                    Trigger::Middle => system::open(item.path),
                    Trigger::Right => {
                        let mut entries: Vec<_> = item
                            .windows
                            .iter()
                            .map(|w| Entry::action(&w.title, Command::Activate(w.hwnd)))
                            .collect();
                        entries.push(Entry::action(
                            "New window",
                            Command::Launch(item.path.clone()),
                        ));
                        entries.push(if item.pinned {
                            Entry::action("Unpin", Command::Unpin(item.path.clone()))
                        } else {
                            Entry::action(
                                "Pin",
                                Command::Pin(Pinned {
                                    path: item.path.clone(),
                                    name: Some(item.name.clone()),
                                }),
                            )
                        });
                        entries.extend(item.windows.iter().map(|w| {
                            Entry::action(&format!("Close {}", w.title), Command::Close(w.hwnd))
                        }));
                        if item.windows.len() > 1 {
                            entries.push(Entry::action(
                                "Close all windows",
                                Command::CloseAll(item.windows.iter().map(|w| w.hwnd).collect()),
                            ));
                        }
                        entries.push(Entry::action(
                            "Recent items and app actions",
                            Command::AppActions(Box::new((item.clone(), anchor, monitor.clone()))),
                        ));
                        let tokens = self.bars[index].tokens.menu(&ui.text);
                        self.open_menu(ui, entries, point, &tokens);
                    }
                    _ => {}
                }
            }
            Target::Extra(Extra::Desktop) if input == Trigger::Left => system::show_desktop(),
            Target::Extra(Extra::Trash) if input == Trigger::Left => {
                system::open("shell:RecycleBinFolder".into())
            }
            Target::Extra(Extra::Trash) if input == Trigger::Right => self.open_menu(
                ui,
                vec![
                    Entry::action(
                        "Open Recycle Bin",
                        Command::Launch("shell:RecycleBinFolder".into()),
                    ),
                    Entry::action("Empty Recycle Bin", Command::EmptyTrash),
                ],
                point,
                &self.bars[index].tokens.menu(&ui.text),
            ),
            _ => {}
        }
    }

    fn action(&mut self, action: Action, anchor: Rect, monitor: &Monitor) {
        match action {
            Action::Exec { exec, args } => providers::custom::exec(&exec, &args),
            Action::Open { open } => system::open(open),
            Action::Builtin(action) => {
                use BuiltinAction::*;
                let kind = builtin_flyout(action);
                if let Some(kind) = kind {
                    flyouts::open(kind, anchor, monitor);
                    return;
                }
                match action {
                    NativeStart => system::start_menu(),
                    NativeNotifications => {
                        system::action_center(providers::actions::Panel::Notifications)
                    }
                    NativeQuickSettings => system::action_center(providers::actions::Panel::Quick),
                    Search => system::search(),
                    TaskManager => system::open("taskmgr.exe".into()),
                    Settings => system::open("ms-settings:".into()),
                    ShowDesktop => system::show_desktop(),
                    MediaToggle => self.services.media.control(MediaAction::Toggle),
                    MediaNext => self.services.media.control(MediaAction::Next),
                    MediaPrevious => self.services.media.control(MediaAction::Previous),
                    VolumeUp => self.volume(Some(0.04), false),
                    VolumeDown => self.volume(Some(-0.04), false),
                    VolumeMute => self.volume(Option::None, true),
                    WorkspaceNext => system::switch_desktop(Direction::Right),
                    WorkspacePrevious => system::switch_desktop(Direction::Left),
                    Lock => system::power(PowerAction::Lock),
                    Explorer => system::open("explorer.exe".into()),
                    _ => {}
                }
            }
        }
    }

    fn volume(&mut self, delta: Option<f32>, toggle_mute: bool) {
        if let Some(audio) = &self.services.audio {
            let mute = toggle_mute.then(|| audio.read().is_some_and(|v| !v.muted));
            self.services.model.volume = audio.change(None, mute, delta);
            self.dirty();
        }
    }
    /// Opens the dock's menu at `point`, drawn with the bar's theme (`tokens`).
    fn open_menu(
        &mut self,
        ui: &mut Ui,
        entries: Vec<Entry<Command>>,
        point: (i32, i32),
        tokens: &mypaper_ui::Tokens,
    ) {
        if self.menu.is_none() {
            self.menu = Menu::new("Dock commands").ok();
        }
        if let Some(menu) = &mut self.menu {
            let dock = self.bars.iter().find(|b| {
                b.kind == BarKind::Dock
                    && b.monitor.rect.x <= point.0
                    && point.0 < b.monitor.rect.x + b.monitor.rect.width
                    && b.monitor.rect.y <= point.1
                    && point.1 < b.monitor.rect.y + b.monitor.rect.height
            });
            let config = dock.map_or(&self.config, |b| &b.config);
            let animation = &config.dock.animation;
            let timing = motion::resolve(
                animation,
                true,
                motion::Leaf::Open,
                self.system.animations,
                config.performance.active(mypaper_desktop::on_battery()),
            );
            let mut bar_look = config.dock.look.clone();
            let appearance = config.appearance.over(&self.appearance);
            look::appearance_layer(config, "dock", &mut bar_look, &appearance);
            look::popup_font(config, "dock", &mut bar_look);
            look::glass_layer(config, "dock", &mut bar_look, self.theme_glass);
            let knobs = Config::load()
                .map(|c| crate::glass::knobs(&c.look, crate::glass::Family::Menus))
                .unwrap_or_default();
            let mut popup = crate::flyouts::look::Look::resolve_with(
                &bar_look,
                &appearance,
                &self.system,
                &knobs,
            )
            .menu_tokens();
            if config.performance.active(mypaper_desktop::on_battery()) {
                popup.glass = None;
            }
            let performance = config.performance.active(mypaper_desktop::on_battery());
            menu.set_interaction_timings(
                motion::resolve(
                    animation,
                    true,
                    motion::Leaf::Hover,
                    self.system.animations,
                    performance,
                ),
                motion::resolve(
                    animation,
                    true,
                    motion::Leaf::Press,
                    self.system.animations,
                    performance,
                ),
            );
            menu.set_presentation(
                popup,
                mypaper_ui::menu::MenuMotion {
                    timing,
                    style: motion::reveal(motion::presentation(
                        animation,
                        true,
                        motion::Leaf::Open,
                    )),
                },
            );
            let _ = menu.open(ui, entries, point, screen_at(point.0, point.1), tokens);
        }
    }
    fn command(&mut self, command: Command) {
        match command {
            Command::Activate(w) => {
                system::activate(system::hwnd(w));
            }
            Command::Close(w) => system::close(system::hwnd(w)),
            Command::CloseAll(windows) => {
                for w in windows {
                    system::close(system::hwnd(w));
                }
            }
            Command::Launch(path) => system::open(path),
            Command::AppActions(data) => crate::integrations::open_app(&data.0, data.1, &data.2),
            Command::EmptyTrash => trash::empty(),
            Command::Pin(pin) => self.edit_pins(|pins| {
                if !pins.iter().any(|p| p.path.eq_ignore_ascii_case(&pin.path)) {
                    pins.push(pin);
                }
            }),
            Command::Unpin(path) => {
                self.edit_pins(|pins| pins.retain(|p| !p.path.eq_ignore_ascii_case(&path)))
            }
        }
    }
    fn edit_pins(&mut self, change: impl FnOnce(&mut Vec<Pinned>)) {
        let mut pins = self.config.dock.pinned.clone().unwrap_or_default();
        change(&mut pins);
        let edited = config::path().and_then(|p| {
            config::edit(&p, |root| {
                root.entry("dock")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .unwrap()
                    .insert("pinned".into(), serde_json::to_value(pins).unwrap());
            })
        });
        if let Err(e) = edited {
            log::warn!("saving dock pins: {e:#}");
        }
        self.config_changed();
    }
    fn drop_item(&mut self, index: usize, target: Target, at: Point) {
        let Target::Dock(item) = target else {
            return;
        };
        if item.key.contains('#') {
            return;
        }
        let b = &self.bars[index];
        let far = at.x < -64.0
            || at.y < -64.0
            || at.x > b.frame.size.0 as f32 + 64.0
            || at.y > b.frame.size.1 as f32 + 64.0;
        let coordinate = if b.horizontal() { at.x } else { at.y };
        let before = b.frame.list.hits.iter().filter(|h| matches!(b.frame.targets.get(h.id as usize-1),Some(Target::Dock(app)) if app.pinned && app.path != item.path)).filter(|h| coordinate > if b.horizontal() { (h.rect.left+h.rect.right)/2.0 } else { (h.rect.top+h.rect.bottom)/2.0 }).count();
        self.edit_pins(|pins| {
            pins.retain(|p| !p.path.eq_ignore_ascii_case(&item.path));
            if !far {
                pins.insert(
                    before.min(pins.len()),
                    Pinned {
                        path: item.path,
                        name: Some(item.name),
                    },
                );
            }
        });
    }

    /// Builds the bars the config and monitors call for. A bar whose window and reservation stay
    /// the same ([`Bar::geometry`]: monitor, edge, band, auto-hide) is kept and restyled in
    /// place, so a style, look or material change only repaints it and fades its plates: no new
    /// windows and no new work area, which would make maximized windows jump and the screen
    /// flash. Only bars whose geometry changed are dropped (first, freeing their band) and built
    /// again, and only then do the bars reserve again.
    fn reconcile(&mut self, ui: &mut Ui) -> Result<()> {
        plugin_modules::place(&self.config);
        self.press = None;
        self.preview = None;
        self.preview_due = None;
        self.preview_close = None;
        let mut old = std::mem::take(&mut self.bars);
        if let Some(menu) = &mut self.menu {
            menu.close();
        }
        let monitors = monitor_infos();
        startup::ready("shell", "monitors_ms");
        for old in &self.monitors {
            if !monitors.iter().any(|m| m.device == old.device) {
                glass::publish(glass::Source::Bars, old, &[]);
                self.published.remove(&old.device);
            }
        }
        self.monitors = monitors.clone();
        let mut wanted = Vec::new();
        for monitor in monitors {
            let mut config = self.config.for_monitor(&monitor.device, monitor.primary)?;
            config.resolve(services::default_pins);
            // Plugin heights are geometry: the bar rebuilds for them, unlike the other tokens.
            if let Some(h) = plugin_style::number("top-bar", "top", self.config.raw(), "height") {
                config.top.height = f64::from(h.clamp(16.0, 96.0));
            }
            let base_appearance = config.appearance.over(&self.appearance);
            for kind in [BarKind::Top, BarKind::Dock] {
                let (surface, bar_key) = match kind {
                    BarKind::Top => ("top-bar", "top"),
                    BarKind::Dock => ("dock", "dock"),
                };
                let mut appearance = base_appearance.clone();
                let (enabled, includes, look) = match kind {
                    BarKind::Top => (
                        config.top.enabled,
                        config
                            .top
                            .monitors
                            .includes(&monitor.device, monitor.primary),
                        &config.top.look,
                    ),
                    BarKind::Dock => (
                        config.dock.enabled,
                        config
                            .dock
                            .monitors
                            .includes(&monitor.device, monitor.primary),
                        &config.dock.look,
                    ),
                };
                if enabled && includes {
                    let performance = config.performance == config::Performance::On
                        || (config.performance == config::Performance::Auto
                            && mypaper_desktop::on_battery());
                    let mut look = look.clone();
                    look::appearance_layer(&config, bar_key, &mut look, &appearance);
                    look::glass_layer(&config, bar_key, &mut look, self.theme_glass);
                    let colors = plugin_style::patch_look(
                        surface,
                        bar_key,
                        self.config.raw(),
                        &mut look,
                        &mut appearance,
                    );
                    let mut tokens = look::Tokens::resolve(&look, &appearance, &self.system);
                    plugin_style::colors(&colors, self.config.raw(), &mut tokens.theme.color);
                    tokens.cache_config(&look, &config);
                    // No glass on battery saver: the theme's own surface draws instead.
                    if performance {
                        tokens.material = None;
                    }
                    let animate = self.system.animations;
                    wanted.push((kind, config.clone(), monitor.clone(), tokens, animate));
                }
            }
        }
        // Keep each bar whose geometry stays; drop the others before any new one reserves.
        let kept: Vec<Option<Bar>> = wanted
            .iter()
            .map(|(kind, config, monitor, tokens, ..)| {
                let geometry = Bar::geometry(*kind, config, monitor);
                old.iter()
                    .position(|b| {
                        Bar::geometry(b.kind, &b.config, &b.monitor) == geometry
                            && b.tokens.scale == tokens.scale
                    })
                    .map(|i| old.swap_remove(i))
            })
            .collect();
        let rebuilt = !old.is_empty() || kept.iter().any(Option::is_none);
        drop(old);
        for ((kind, config, monitor, tokens, animate), kept) in wanted.into_iter().zip(kept) {
            let mut bar = match kept {
                Some(mut bar) => {
                    bar.restyle(config, tokens, animate);
                    bar
                }
                None => {
                    let mut bar = Bar::new(kind, config, monitor, tokens, animate)?;
                    bar.hidden =
                        self.asleep || self.fullscreen.as_deref() == Some(&bar.monitor.device);
                    if kind == BarKind::Dock {
                        bar.items = self.services.dock_items(
                            &bar.config,
                            providers::apps::window_monitor(bar.surface.hwnd()),
                        );
                    }
                    // A rebuild can spend time presenting several monitors. Re-establish each
                    // band before its new surface appears, so maximized apps cannot enter it.
                    if self.journal.is_some() {
                        bar.reserve();
                    }
                    bar
                }
            };
            // Present immediately: creating the remaining windows is not part of this
            // surface's first-frame dependency chain.
            bar.paint(
                ui,
                &self.services.model,
                &mut self.images,
                Instant::now(),
                false,
            )?;
            self.bars.push(bar);
        }
        // Startup defers reservations until core hands over its taskbar journal. A rebuild
        // dropped old appbars, so every bar settles its band again before returning to the loop;
        // a restyle keeps every band as it was.
        if self.journal.is_some() {
            self.apply_taskbar();
            if rebuilt {
                for bar in &mut self.bars {
                    bar.reserve();
                }
            }
            self.schedule_reservations();
        }
        self.update_items();
        Ok(())
    }
    fn schedule_reservations(&mut self) {
        if self.bars.iter().any(|bar| !bar.reserved) {
            self.reservation_due
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(1));
        } else {
            self.reservation_due = None;
        }
    }
    fn update_items(&mut self) {
        for b in &mut self.bars {
            if b.kind == BarKind::Dock {
                let monitor = providers::apps::window_monitor(b.surface.hwnd());
                let items = self.services.dock_items(&b.config, monitor);
                if items != b.items {
                    b.items = items;
                    b.dirty = true;
                }
            }
        }
    }
    fn dirty(&mut self) {
        for b in &mut self.bars {
            b.dirty = true;
        }
    }
    pub fn integration_changed(&mut self) {
        self.subscribe();
        self.dirty();
    }
    fn subscribe(&mut self) {
        let topics = self
            .bars
            .iter()
            .filter(|b| b.shows_content())
            .flat_map(Bar::topics)
            .filter(|t| self.fullscreen.is_none() || !t.periodic())
            .collect();
        let configs: Vec<_> = self
            .bars
            .iter()
            .filter(|b| b.shows_content())
            .map(|b| &b.config)
            .collect();
        let seconds = self.bars.iter().any(|b| {
            if b.kind == BarKind::Top {
                b.config.top.clock.seconds
            } else {
                b.config.dock.clock.seconds
            }
        });
        let tray = self
            .bars
            .iter()
            .any(|b| b.topics().contains(&providers::actions::Topic::Tray))
            && (self.config.tray.host || self.config.taskbar.mode == TaskbarMode::Hide);
        self.services.subscribe(topics, &configs, seconds, tray);
        crate::integrations::bar_subscriptions(
            self.bars
                .iter()
                .filter(|b| b.shows_content())
                .flat_map(|b| providers::actions::bar_modules(b.kind, &b.config)),
            self.bars
                .iter()
                .any(|b| b.shows_content() && b.kind == BarKind::Dock),
        );
        let trash = self.bars.iter().any(|b| {
            b.shows_content()
                && b.kind == BarKind::Dock
                && b.config.dock.extras.contains(&Extra::Trash)
        });
        if trash != self.trash.is_some() {
            self.trash = trash
                .then(|| {
                    self.broadcast
                        .as_ref()
                        .and_then(|b| TrashWatch::new(b.hwnd()))
                })
                .flatten();
            if trash {
                self.services.model.trash_full = providers::trash::full();
            }
        }
    }
    fn apply_taskbar(&mut self) {
        let wanted = Wanted {
            mode: if self.bars.is_empty() {
                TaskbarMode::Keep
            } else {
                self.config.taskbar.mode
            },
            alignment: self.config.taskbar.alignment,
        };
        providers::hooks::watch_taskbar(wanted.mode == TaskbarMode::Hide);
        if let Err(e) = self.taskbar.apply(wanted) {
            log::warn!("native taskbar: {e:#}");
        }
    }
    fn read_theme(&mut self) {
        let loaded = Config::load()
            .and_then(|c| mypaper_theme::locate(Some(&c.theme)))
            .and_then(LoadedTheme::load)
            .ok();
        self.images.configure(
            self.config.icons.as_deref(),
            loaded
                .as_ref()
                .map_or_else(Default::default, |t| t.dir.clone()),
        );
        self.appearance = look::theme_appearance(loaded.as_ref());
        self.theme_glass = loaded
            .as_ref()
            .and_then(|t| t.theme.bars.as_ref())
            .and_then(|s| s.glass);
        self.theme_watch = None;
        if let Some(t) = &loaded
            && let Ok(text) = std::fs::read_to_string(t.dir.join("theme.json"))
            && let Ok(value) = serde_json::from_str(&text)
        {
            look::warn_dropped(&value);
        }
        if let Some(theme) = &loaded {
            self.theme_watch =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.is_ok_and(|e| {
                        !matches!(e.kind, notify::EventKind::Access(_))
                            && e.paths.iter().any(|p| {
                                p.file_name()
                                    .is_some_and(|f| f.eq_ignore_ascii_case("theme.json"))
                            })
                    }) {
                        crate::inbox::post(crate::inbox::WM_CONFIG, 0);
                    }
                })
                .ok()
                .and_then(|mut watcher| {
                    watcher
                        .watch(&theme.dir, notify::RecursiveMode::NonRecursive)
                        .ok()?;
                    Some(watcher)
                });
        }
    }
    fn watch(&mut self) {
        let Ok(dir) = mypaper_config::dir() else {
            return;
        };
        let waker = self.services.waker.clone();
        if let Ok(mut watcher) =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                if let Ok(event) = result
                    && (event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove())
                    && event
                        .paths
                        .iter()
                        .any(|p| p.file_name().is_some_and(|f| f == "bars.json"))
                {
                    waker.send(providers::wake::Msg::Config);
                }
            })
            && watcher
                .watch(&dir, notify::RecursiveMode::NonRecursive)
                .is_ok()
        {
            self.watches.push(watcher);
        }
    }
}

impl Drop for Bars {
    fn drop(&mut self) {
        // Construction can fail before the post-paint handoff from core.
        if self.journal.is_none() {
            self.journal = JournalLease::acquire().ok();
        }
        providers::hooks::watch_taskbar(false);
        self.services.tray_host.set_active(false);
        for monitor in monitor_infos() {
            glass::publish(glass::Source::Bars, &monitor, &[]);
        }
        self.bars.clear();
        if let Err(e) = self.taskbar.restore() {
            log::error!("restoring native taskbar: {e:#}");
        }
    }
}

fn trigger(button: Button) -> Trigger {
    match button {
        Button::Left => Trigger::Left,
        Button::Middle => Trigger::Middle,
        Button::Right => Trigger::Right,
    }
}
fn load_config() -> Result<BarsConfig> {
    let mut config = config::path().and_then(|p| BarsConfig::load(&p))?;
    if let Ok(value) = serde_json::to_value(&config) {
        look::warn_dropped(&value);
    }
    config.resolve(services::default_pins);
    Ok(config)
}

/// Only flyout modules speculate a capture, never ordinary app/volume-wheel clicks.
fn opens_flyout(target: &Target, config: &BarsConfig) -> bool {
    let module = match target {
        Target::Module(module) => Some(module.clone()),
        Target::Tray(_) => Some(ModuleRef::Builtin(Module::Tray)),
        _ => None,
    };
    if let Some(action) = module
        .as_ref()
        .and_then(|m| config.modules.get(m))
        .and_then(|m| m.click.left.as_ref())
    {
        match action {
            Action::Builtin(BuiltinAction::Default) => {}
            Action::Builtin(action) => return builtin_flyout(*action).is_some(),
            _ => return false,
        }
    }
    matches!(
        target,
        Target::Module(ModuleRef::Builtin(
            Module::Start
                | Module::Clock
                | Module::Quick
                | Module::Network
                | Module::Volume
                | Module::Power
                | Module::Language
                | Module::Notifications
        ))
    ) || matches!(target, Target::Tray(id) if id == providers::tray::OVERFLOW)
}

fn builtin_flyout(action: BuiltinAction) -> Option<flyouts::Kind> {
    use BuiltinAction::*;
    match action {
        Start => Some(flyouts::Kind::Launcher),
        Notifications => Some(flyouts::Kind::Notifications),
        QuickSettings => Some(flyouts::Kind::Quick),
        Calendar => Some(flyouts::Kind::Calendar),
        Network => Some(flyouts::Kind::Wifi),
        Mixer => Some(flyouts::Kind::Mixer),
        _ => Option::None,
    }
}

thread_local! {
    static TICK_COSTS: std::cell::RefCell<Vec<(&'static str, Duration)>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// Adds the time since `start` to `part`; returns now.
fn tick_spent(part: &'static str, start: Instant) -> Instant {
    let now = Instant::now();
    TICK_COSTS.with_borrow_mut(|costs| match costs.iter_mut().find(|c| c.0 == part) {
        Some(c) => c.1 += now - start,
        None => costs.push((part, now - start)),
    });
    now
}
