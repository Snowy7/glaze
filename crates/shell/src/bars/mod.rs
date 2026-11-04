//! Native top bar and dock. Windows services live in `mypaper-bars`; UI and motion live here.
mod bar;
mod icons;
mod look;
mod motion;
mod preview;
mod services;
#[cfg(test)]
mod tests;
mod view;

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
    Activate(isize),
    Close(isize),
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
    press: Option<Press>,
    watches: Vec<notify::RecommendedWatcher>,
    appearance: Appearance,
    system: providers::accent::System,
    published: BTreeMap<String, Vec<glass::Plate>>,
    config_due: Option<Instant>,
    display_due: Option<Instant>,
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
            press: None,
            watches: vec![],
            appearance: Appearance::default(),
            system: providers::accent::read(),
            published: BTreeMap::new(),
            config_due: None,
            display_due: None,
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
        };
        bars.read_theme();
        startup::ready("shell", "theme_ms");
        bars.reconcile(ui)?;
        startup::ready("shell", "surfaces_ms");
        bars.paint(ui, Instant::now())?;
        startup::ready("shell", "bars_painted_ms");
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
    pub fn timeout(&self) -> u32 {
        [
            self.config_due,
            self.display_due,
            self.explorer_due,
            self.rehide_due,
            self.services.deadline(),
            self.preview_due.as_ref().map(|p| p.0),
            self.preview_close,
            self.bars.iter().filter_map(|b| b.hide_at).min(),
        ]
        .into_iter()
        .flatten()
        .min()
        .map_or(u32::MAX, |at| {
            services::wait_ms(at.saturating_duration_since(Instant::now()))
        })
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
                for b in &mut self.bars {
                    let appearance = b.config.appearance.over(&self.appearance);
                    let look = if b.kind == BarKind::Top {
                        &b.config.top.look
                    } else {
                        &b.config.dock.look
                    };
                    b.tokens = look::Tokens::resolve(look, &appearance, &self.system);
                    b.dirty = true;
                }
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
                self.config_changed()
            }
            _ => {}
        }
        self.asleep = self.display_off || self.locked;
    }

    /// All drawing is gated by data changes or active animation. The ticker cancels at rest.
    pub fn tick(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        let changes = self.services.tick(now);
        if changes.config || changes.theme {
            self.config_changed();
        }
        if services::take_due(&mut self.config_due, now) {
            match load_config() {
                Ok(config) => {
                    self.config = config;
                    self.read_theme();
                    if let Err(e) = self.reconcile(ui) {
                        log::error!("bars: {e:#}");
                    }
                    self.apply_taskbar();
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
        }
        if changes.data {
            self.update_items();
            self.dirty();
        }
        if changes.data || changes.moved {
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
            if let Some(b) = self.bars.get(index)
                && !b.hidden
                && let Some((i, app)) =
                    b.frame
                        .targets
                        .iter()
                        .enumerate()
                        .find_map(|(i, t)| match t {
                            Target::Dock(app) if app.key == key => Some((i, app.clone())),
                            _ => None,
                        })
                && let Some(hit) = b.frame.list.hits.iter().find(|h| h.id == i as u32 + 1)
            {
                let origin = b.surface.origin();
                let anchor = hit.rect.offset(origin.0 as f32, origin.1 as f32);
                self.preview = preview::Preview::open(ui, app, anchor, &b.tokens, b.scale())
                    .inspect_err(|e| log::warn!("dock previews: {e:#}"))
                    .ok();
            }
        }
        if let Err(e) = self.paint(ui, now) {
            log::error!("painting bars: {e:#}");
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
        if let Some(preview) = &self.preview
            && preview.owns(id)
        {
            match event {
                Event::Move(_) => self.preview_close = None,
                Event::Leave => {
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
        let Some(index) = self.bars.iter().position(|b| b.surface.id() == id) else {
            return;
        };
        match event {
            Event::DpiChanged(_) => self.display_due = Some(now),
            Event::Leave => {
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
                }
                b.zoom_to(Some(p.at), now);
                if b.config.dock.preview.enabled
                    && self.press.is_none()
                    && !self.menu.as_ref().is_some_and(Menu::is_open)
                {
                    if let Some((_, Target::Dock(app))) = b.frame.target(p.at) {
                        if !app.windows.is_empty()
                            && self.preview.as_ref().is_none_or(|v| v.key != app.key)
                            && self.preview_due.as_ref().is_none_or(|v| v.2 != app.key)
                        {
                            self.preview_due = Some((
                                now + Duration::from_millis(if self.preview.is_some() {
                                    0
                                } else {
                                    b.config.dock.preview.delay as u64
                                }),
                                index,
                                app.key,
                            ));
                        }
                        self.preview_close = None;
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
                }
            }
            Event::Press(p, Button::Left) => {
                if let Some((_, target)) = self.bars[index].frame.target(p.at) {
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
            Event::CaptureLost => self.press = None,
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
                    let p = self.bars[index].surface.origin();
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
                    (point.0 - b.surface.origin().0) as f32,
                    (point.1 - b.surface.origin().1) as f32,
                ))
            })
            .map_or(Rect::new(point.0 as f32, point.1 as f32, 1.0, 1.0), |h| {
                h.rect
                    .offset(b.surface.origin().0 as f32, b.surface.origin().1 as f32)
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
                let ModuleRef::Builtin(module) = module else {
                    return;
                };
                let grouped = b.tokens.family == Family::Fluent;
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
                        self.open_menu(ui, entries, point);
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
                let kind = match action {
                    Start | NativeStart => Some(flyouts::Kind::Launcher),
                    Notifications | NativeNotifications => Some(flyouts::Kind::Notifications),
                    QuickSettings | NativeQuickSettings => Some(flyouts::Kind::Quick),
                    Calendar => Some(flyouts::Kind::Calendar),
                    Network => Some(flyouts::Kind::Wifi),
                    Mixer => Some(flyouts::Kind::Mixer),
                    _ => Option::None,
                };
                if let Some(kind) = kind {
                    flyouts::open(kind, anchor, monitor);
                    return;
                }
                match action {
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
    fn open_menu(&mut self, ui: &mut Ui, entries: Vec<Entry<Command>>, point: (i32, i32)) {
        if self.menu.is_none() {
            self.menu = Menu::new("Dock commands").ok();
        }
        if let Some(menu) = &mut self.menu {
            let _ = menu.open(
                ui,
                entries,
                point,
                screen_at(point.0, point.1),
                &mypaper_ui::Tokens::default(),
            );
        }
    }
    fn command(&mut self, command: Command) {
        match command {
            Command::Activate(w) => {
                system::activate(system::hwnd(w));
            }
            Command::Close(w) => system::close(system::hwnd(w)),
            Command::Launch(path) => system::open(path),
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

    fn reconcile(&mut self, ui: &mut Ui) -> Result<()> {
        self.press = None;
        self.preview = None;
        self.preview_due = None;
        self.preview_close = None;
        self.bars.clear();
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
        for monitor in monitors {
            let mut config = self.config.for_monitor(&monitor.device, monitor.primary)?;
            config.resolve(services::default_pins);
            let appearance = config.appearance.over(&self.appearance);
            for kind in [BarKind::Top, BarKind::Dock] {
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
                    let mut tokens = look::Tokens::resolve(look, &appearance, &self.system);
                    if performance && let Some(material) = tokens.material.take() {
                        tokens.fill = material.tint.with_alpha(0.88);
                    }
                    let mut bar = Bar::new(
                        kind,
                        config.clone(),
                        monitor.clone(),
                        tokens,
                        self.system.animations && !performance,
                    )?;
                    bar.hidden = self.asleep || self.fullscreen.as_deref() == Some(&monitor.device);
                    if kind == BarKind::Dock {
                        bar.items = self.services.dock_items(
                            &config,
                            providers::apps::window_monitor(bar.surface.hwnd()),
                        );
                    }
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
            }
        }
        self.update_items();
        Ok(())
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
        let tray = self.config.tray.host || self.config.taskbar.mode == TaskbarMode::Hide;
        self.services.subscribe(topics, &configs, seconds, tray);
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
        self.appearance = loaded
            .as_ref()
            .and_then(|t| t.theme.bars.as_ref())
            .and_then(|s| s.appearance.as_ref())
            .cloned()
            .unwrap_or_default();
        if let Some(t) = &loaded
            && matches!(self.appearance.accent, Some(mypaper_theme::Paint::Auto))
            && let Some(snapshot) = mypaper_tweaks::palette::read().filter(|p| p.is_for(&t.dir))
        {
            self.appearance.accent = Some(mypaper_theme::Paint::Color(snapshot.palette.accent));
        }
        if let Some(t) = &loaded
            && let Ok(text) = std::fs::read_to_string(t.dir.join("theme.json"))
            && let Ok(value) = serde_json::from_str(&text)
        {
            look::warn_dropped(&value);
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
