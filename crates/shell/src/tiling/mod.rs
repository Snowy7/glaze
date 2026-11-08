//! Opt-in scrolling window management. No object, subscription, worker or timer exists while off.
pub mod animation;
pub(crate) mod indicator;
mod learned;
mod overview;
mod parking;
mod platform;
use parking::{bounds, monitor_rect, park};
mod rules;
use anyhow::Result;
use glaze_desktop::{
    tiling_recovery::{self, Journal},
    win_events::{self, Subscription, TilingHooks},
};
use glaze_tiling::{Command, Constraints, Layout, Rect, Settings, WindowId};
use platform::hwnd;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::{
            Shell::{IVirtualDesktopManager, VirtualDesktopManager},
            WindowsAndMessaging::*,
        },
    },
    core::GUID,
};
pub fn current_integrity() -> Result<u32> {
    platform::integrity(std::process::id())
}
static COMMANDS: Mutex<VecDeque<Command>> = Mutex::new(VecDeque::new());
static DOCK_FOCUS: Mutex<VecDeque<WindowId>> = Mutex::new(VecDeque::new());
/// A dock click reveals its column even if Windows refuses foreground activation.
pub fn dock_click(raw: isize) {
    if glaze_bars::flyouts::winkey::tiling_enabled() {
        DOCK_FOCUS.lock().unwrap().push_back(WindowId(raw as u64));
        crate::inbox::post(crate::inbox::WM_CONFIG + 24, 0);
    }
}
/// Only config reload installs the callback. The disabled keyboard path reads one atomic.
pub fn configure_keys(settings: Option<&Settings>) {
    glaze_bars::flyouts::winkey::configure_tiling(settings.map(|s| {
        (
            s.keybindings.clone(),
            Arc::new(|command| {
                let mut queue = COMMANDS.lock().unwrap();
                if queue.len() < 64 {
                    queue.push_back(command);
                    crate::inbox::post(crate::inbox::WM_CONFIG + 20, 0);
                }
            }) as Arc<dyn Fn(Command) + Send + Sync>,
        )
    }));
}
pub fn process_commands(tiler: &mut Tiler) {
    let focus = std::mem::take(&mut *DOCK_FOCUS.lock().unwrap());
    for window in focus {
        if let Some(w) = tiler.windows.get(&window) {
            tiler.current = w.desktop.clone();
            if let Some(l) = tiler.layouts.get_mut(&w.desktop) {
                l.focus(window);
            }
            let _ = tiler.commit(true);
        }
    }
    let commands = std::mem::take(&mut *COMMANDS.lock().unwrap());
    for command in commands {
        tiler.command(command);
    }
}

#[derive(Default)]
struct Queue {
    events: VecDeque<win_events::Event>,
    known: HashSet<isize>,
    deferred: HashSet<isize>,
}
struct Managed {
    admission: platform::Admission,
    desktop: String,
    expected: Option<Rect>,
    moves: VecDeque<Instant>,
    offences: u8,
    minimized: bool,
    fullscreen: bool,
    suspended: Option<glaze_tiling::Suspended>,
    maximized: bool,
    scale: f64,
}
/// UI-thread controller. Events arrive on the existing WinEvent thread and are drained after a wake.
pub struct Tiler {
    settings: Settings,
    layouts: HashMap<String, Layout>,
    windows: HashMap<WindowId, Managed>,
    queue: Arc<Mutex<Queue>>,
    _subscription: Subscription,
    _hooks: TilingHooks,
    journal: Journal,
    rules: rules::Rules,
    scope: Option<HashSet<u32>>,
    desktops: Option<IVirtualDesktopManager>,
    current: String,
    displays: Vec<glaze_desktop::Monitor>,
    changed_at: Instant,
    dragging: Option<WindowId>,
    animation: animation::Animator,
    display_events: platform::DisplayEvents,
    overview: Option<overview::Overview>,
    learned: learned::Learned,
    drag_origin: Option<(f64, f64)>,
    hint: Option<platform::InsertHint>,
    edge_at: Instant,
    recently_hidden: VecDeque<(tiling_recovery::Identity, Instant)>,
    focus_settle: Instant,
}
impl Tiler {
    /// Fixture/recovery diagnostics: expected physical bounds for the current managed set.
    pub fn placements(&self) -> Vec<(tiling_recovery::Identity, Rect)> {
        self.windows
            .values()
            .filter(|w| !w.minimized && !w.fullscreen)
            .filter_map(|w| w.expected.map(|r| (w.admission.identity, r)))
            .collect()
    }
    pub fn start(settings: Settings) -> Result<Self> {
        let scope = platform::scope()?;
        tiling_recovery::restore_pending()?;
        let mut journal = Journal::open(tiling_recovery::path()?)?;
        let rules = rules::Rules::load()?;
        let display_events = platform::DisplayEvents::new()?;
        let learned = learned::Learned::load()?;
        // Scoped tests do not change user-wide Windows preferences.
        if scope.is_none() {
            journal.disable_snap()?;
        }
        let queue = Arc::new(Mutex::new(Queue::default()));
        let events = queue.clone();
        let event_scope = scope.clone();
        let subscription = Subscription::new(move |event| {
            let mut q = events.lock().unwrap();
            if event_scope.is_some()
                // A destroyed HWND no longer reports its PID. Admission already established
                // this window's scope; dropping its destroy event would retain a dead tile.
                && !(event.event == EVENT_OBJECT_DESTROY && q.known.contains(&event.window))
                && !platform::allowed(hwnd(WindowId(event.window as u64)), &event_scope)
            {
                return;
            }
            if !q.known.contains(&event.window)
                && matches!(
                    event.event,
                    EVENT_OBJECT_CREATE
                        | EVENT_OBJECT_SHOW
                        | EVENT_OBJECT_NAMECHANGE
                        | EVENT_OBJECT_UNCLOAKED
                )
                && !platform::desktop_window(hwnd(WindowId(event.window as u64)))
                && !platform::intake_candidate(hwnd(WindowId(event.window as u64)))
            {
                return;
            }
            if !q.known.contains(&event.window)
                && matches!(
                    event.event,
                    EVENT_OBJECT_HIDE | EVENT_OBJECT_DESTROY | EVENT_OBJECT_CLOAKED
                )
            {
                return;
            }
            if matches!(
                event.event,
                EVENT_OBJECT_LOCATIONCHANGE | EVENT_SYSTEM_MOVESIZESTART | EVENT_SYSTEM_MOVESIZEEND
            ) && !q.known.contains(&event.window)
            {
                return;
            }
            if event.event == EVENT_OBJECT_LOCATIONCHANGE && animation::busy() {
                q.deferred.insert(event.window);
                return;
            }
            if q.events
                .iter()
                .any(|e| e.event == event.event && e.window == event.window)
            {
                return;
            }
            if q.events.len() < 512 {
                q.events.push_back(event);
                win_events::notify();
            }
        });
        // SAFETY: public in-process COM service, optional if Explorer has not started.
        let desktops =
            unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_INPROC_SERVER) }.ok();
        let mut tiler = Self {
            settings,
            layouts: HashMap::new(),
            windows: HashMap::new(),
            queue,
            _subscription: subscription,
            _hooks: TilingHooks::enable(),
            journal,
            rules,
            scope,
            desktops,
            current: String::new(),
            displays: Vec::new(),
            changed_at: Instant::now() - Duration::from_secs(11),
            dragging: None,
            animation: animation::Animator::new(),
            display_events,
            overview: None,
            learned,
            drag_origin: None,
            hint: None,
            edge_at: Instant::now(),
            recently_hidden: VecDeque::new(),
            focus_settle: Instant::now() - Duration::from_secs(1),
        };
        tiler.refresh_displays();
        tiler.enumerate();
        if tiler.current.is_empty() {
            tiler.current = tiler
                .desktop(platform::foreground())
                .unwrap_or_else(|| format!("{:?}", GUID::zeroed()));
        }
        if !tiler.layouts.contains_key(&tiler.current) {
            tiler
                .layouts
                .insert(tiler.current.clone(), tiler.make_layout());
        }
        tiler.commit(false)?;
        Ok(tiler)
    }
    pub fn update(&mut self, settings: Settings) {
        if self.settings != settings {
            self.settings = settings.clone();
            for l in self.layouts.values_mut() {
                l.settings = settings.clone();
            }
            if let Err(e) = self.commit(true) {
                log::error!("tiling config: {e:#}");
            }
        }
    }
    fn desktop(&self, h: windows::Win32::Foundation::HWND) -> Option<String> {
        if let Some(manager) = &self.desktops {
            // SAFETY: public read-only desktop query.
            let guid = unsafe { manager.GetWindowDesktopId(h) }.ok()?;
            Some(format!("{guid:?}"))
        } else {
            Some(format!("{:?}", GUID::zeroed()))
        }
    }
    fn current_desktop(&self, h: windows::Win32::Foundation::HWND) -> bool {
        self.desktops.as_ref().is_none_or(|d| {
            /* SAFETY: public read-only query. */
            unsafe { d.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|v| v.as_bool())
        })
    }
    fn enumerate(&mut self) {
        let mut windows: Vec<isize> = Vec::new();
        unsafe extern "system" fn collect(
            h: windows::Win32::Foundation::HWND,
            p: windows::Win32::Foundation::LPARAM,
        ) -> windows::core::BOOL {
            /* SAFETY: stack Vec lives through synchronous enumeration. */
            unsafe { (*(p.0 as *mut Vec<isize>)).push(h.0 as isize) };
            true.into()
        }
        // SAFETY: callback borrows our Vec only for this call.
        let _ = unsafe {
            EnumWindows(
                Some(collect),
                windows::Win32::Foundation::LPARAM(&mut windows as *mut _ as isize),
            )
        };
        for h in windows {
            let window = WindowId(h as u64);
            if self.windows.contains_key(&window) && self.current_desktop(hwnd(window)) {
                if let Some(desktop) = self.desktop(hwnd(window)) {
                    self.current = desktop;
                }
            }
            self.admit(window);
        }
        self.update_process_hooks();
    }
    fn admit(&mut self, window: WindowId) {
        if self.windows.contains_key(&window) || !self.current_desktop(hwnd(window)) {
            return;
        }
        let Some(admission) = platform::admit(hwnd(window), &self.scope, &self.rules) else {
            return;
        };
        self.recently_hidden
            .retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
        if self
            .recently_hidden
            .iter()
            .any(|(id, _)| *id == admission.identity)
        {
            return;
        }
        if platform::fullscreen(hwnd(window)) {
            return;
        }
        let Some(desktop) = self.desktop(hwnd(window)) else {
            return;
        };
        self.current = desktop.clone();
        if !self.layouts.contains_key(&desktop) {
            let layout = self.make_layout();
            self.layouts.insert(desktop.clone(), layout);
        }
        // A journal failure prevents every subsequent window mutation.
        if let Err(e) = self.journal.remember(admission.identity) {
            log::error!("tiling admission: {e:#}");
            return;
        }
        let owner = admission.owner;
        let behavior = self.learned.get(&admission.exe);
        let float = admission.floating || behavior.floating;
        let frame = admission.frame;
        let layout = self.layouts.get_mut(&desktop).unwrap();
        let prior_monitor = layout.focused_monitor.clone();
        let had_focus = layout.focused().is_some();
        if let Some(owner) = owner {
            layout.focus(owner);
        } else if let Some(m) = layout.monitors.iter().find(|m| {
            m.full
                .contains(frame.x + frame.w / 2.0, frame.y + frame.h / 2.0)
        }) {
            layout.focused_monitor = Some(m.id.clone());
        }
        let floating = float.then(|| {
            let parent = owner.and_then(|id| {
                layout
                    .targets()
                    .into_iter()
                    .find(|t| t.window == id)
                    .map(|t| t.rect)
            });
            let mut placed = if let Some(p) = parent {
                Rect {
                    x: p.x + (p.w - frame.w) / 2.0,
                    y: p.y + (p.h - frame.h) / 2.0,
                    ..frame
                }
            } else {
                frame
            };
            if let Some(m) = layout
                .monitors
                .iter()
                .find(|m| {
                    m.area
                        .contains(placed.x + placed.w / 2.0, placed.y + placed.h / 2.0)
                })
                .or_else(|| layout.monitors.first())
            {
                placed.x = placed
                    .x
                    .clamp(m.area.x, (m.area.x + m.area.w - placed.w).max(m.area.x));
                placed.y = placed
                    .y
                    .clamp(m.area.y, (m.area.y + m.area.h - placed.h).max(m.area.y));
            }
            placed
        });
        // Do not change foreground: Windows has already made the launch/focus decision.
        let active = platform::foreground() == hwnd(window);
        layout.insert(
            window,
            Constraints {
                min_w: behavior.min_w,
                min_h: behavior.min_h,
                ..Constraints::default()
            },
            owner,
            floating,
            active,
        );
        if !active && had_focus {
            layout.focused_monitor = prior_monitor;
        }
        let maximized = platform::zoomed(hwnd(window));
        if maximized {
            layout.set_maximized(window, true);
        }
        self.windows.insert(
            window,
            Managed {
                admission,
                desktop,
                expected: None,
                moves: VecDeque::new(),
                offences: 0,
                minimized: false,
                fullscreen: false,
                suspended: None,
                maximized,
                scale: platform::dpi_scale(hwnd(window)),
            },
        );
        self.queue.lock().unwrap().known.insert(window.0 as isize);
    }
    fn make_layout(&self) -> Layout {
        let mut l = Layout::new(self.settings.clone());
        let paths = glaze_desktop::display_paths();
        for m in &self.displays {
            let key = paths
                .iter()
                .find(|(d, _)| *d == m.device)
                .map_or(&m.device, |(_, p)| p);
            l.add_monitor(
                key.clone(),
                monitor_rect(m.work),
                monitor_rect(m.rect),
                f64::from(m.dpi) / 96.0,
            );
        }
        l
    }
    fn refresh_displays(&mut self) {
        let displays = glaze_desktop::monitor_infos();
        if displays == self.displays {
            return;
        }
        self.displays = displays;
        self.changed_at = Instant::now();
        let paths = glaze_desktop::display_paths();
        let ids: HashSet<_> = self
            .displays
            .iter()
            .map(|m| {
                paths
                    .iter()
                    .find(|(d, _)| *d == m.device)
                    .map_or(m.device.clone(), |(_, p)| p.clone())
            })
            .collect();
        for l in self.layouts.values_mut() {
            let removed: Vec<_> = l
                .monitors
                .iter()
                .filter(|m| !ids.contains(&m.id))
                .map(|m| m.id.clone())
                .collect();
            for id in removed {
                l.remove_monitor(&id);
            }
            for m in &self.displays {
                let key = paths
                    .iter()
                    .find(|(d, _)| *d == m.device)
                    .map_or(&m.device, |(_, p)| p);
                l.add_monitor(
                    key.clone(),
                    monitor_rect(m.work),
                    monitor_rect(m.rect),
                    f64::from(m.dpi) / 96.0,
                );
            }
        }
    }
    fn update_process_hooks(&self) {
        let mut processes: Vec<_> = self
            .windows
            .values()
            .map(|w| w.admission.identity.pid)
            .collect();
        processes.sort_unstable();
        processes.dedup();
        TilingHooks::processes(processes);
    }
    /// Process at most one coalesced event batch; empty queues do no Win32 work.
    pub fn process(&mut self) {
        if !animation::busy() {
            let mut queue = self.queue.lock().unwrap();
            let deferred = std::mem::take(&mut queue.deferred);
            for window in deferred {
                if queue.known.contains(&window) {
                    queue.events.push_back(win_events::Event {
                        event: EVENT_OBJECT_LOCATIONCHANGE,
                        window,
                    });
                }
            }
        }
        if self.display_events.drag_frame() {
            if let Some(window) = self.dragging {
                self.drag_hint(window);
            }
        }
        let mut learned_changed = false;
        for result in animation::results() {
            let window = WindowId(result.identity.hwnd as u64);
            let Some(w) = self.windows.get(&window) else {
                continue;
            };
            if w.admission.identity != result.identity || w.fullscreen || w.minimized {
                continue;
            }
            // A previous animation can complete after a newer action. Never learn stale geometry.
            if !w.expected.is_some_and(|r| near(r, result.expected)) {
                continue;
            }
            if result.actual.w + 16.0 < result.expected.w
                || result.actual.h + 16.0 < result.expected.h
            {
                if let Some(l) = self.layouts.get_mut(&w.desktop) {
                    l.float_at(window, result.actual);
                    let _ = self.learned.record(&w.admission.exe, 0.0, 0.0, true);
                    learned_changed = true;
                }
                continue;
            }
            if result.actual.w > result.expected.w + 16.0
                || result.actual.h > result.expected.h + 16.0
            {
                if let Some(l) = self.layouts.get_mut(&w.desktop) {
                    if let Some((mi, _)) = l.locate(window) {
                        let m = &l.monitors[mi];
                        let scale = m.scale;
                        let width = result.actual.w / scale;
                        let height = result.actual.h / scale;
                        let along = match m.axis {
                            glaze_tiling::Axis::Horizontal => width,
                            glaze_tiling::Axis::Vertical => height,
                        };
                        let axis = m.axis;
                        let area = m.area;
                        let before = l
                            .targets()
                            .into_iter()
                            .find(|t| t.window == window)
                            .map(|t| t.rect);
                        l.learn_minimum(window, along);
                        let after = l
                            .targets()
                            .into_iter()
                            .find(|t| t.window == window)
                            .map(|t| t.rect);
                        if before == after {
                            let mut rect = result.actual;
                            rect.x = rect.x.clamp(area.x, (area.x + area.w - rect.w).max(area.x));
                            rect.y = rect.y.clamp(area.y, (area.y + area.h - rect.h).max(area.y));
                            l.float_at(window, rect);
                            let _ = self.learned.record(&w.admission.exe, 0.0, 0.0, true);
                        } else {
                            let _ = match axis {
                                glaze_tiling::Axis::Horizontal => {
                                    self.learned.record(&w.admission.exe, width, 0.0, false)
                                }
                                glaze_tiling::Axis::Vertical => {
                                    self.learned.record(&w.admission.exe, 0.0, height, false)
                                }
                            };
                        }
                        learned_changed = true;
                    }
                }
            }
        }
        if learned_changed {
            if let Err(e) = self.commit(false) {
                log::warn!("learned minimum: {e:#}");
            }
        }
        let overview_events = self
            .overview
            .as_ref()
            .map(|o| o.events())
            .unwrap_or_default();
        for event in overview_events {
            match event {
                overview::Event::Select(id) => {
                    if let Some(l) = self.layouts.get_mut(&self.current) {
                        l.focus(id);
                    }
                    self.close_overview();
                    let _ = self.commit(true);
                    self.activate_focused();
                }
                overview::Event::Drop {
                    window,
                    monitor,
                    index,
                    insert,
                } => {
                    if let Some(l) = self.layouts.get_mut(&self.current) {
                        l.drop_on_workspace(window, &monitor, index, insert);
                    }
                    self.refresh_overview();
                    let _ = self.commit(false);
                }
                overview::Event::Close => {
                    self.close_overview();
                    let _ = self.commit(false);
                    self.activate_focused();
                }
            }
        }
        if self.display_events.changed() {
            self.displays_changed();
            self.enumerate();
        }
        let events = std::mem::take(&mut self.queue.lock().unwrap().events);
        if events.is_empty() {
            return;
        }
        let mut changed = false;
        let mut processes_changed = false;
        for event in events {
            let window = WindowId(event.window as u64);
            let h = hwnd(window);
            match event.event {
                EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE => {
                    if event.event == EVENT_OBJECT_HIDE && platform::visible(h) {
                        continue;
                    }
                    // Display reconfiguration can synthesize both MINIMIZE and HIDE.
                    if event.event == EVENT_OBJECT_HIDE
                        && self.changed_at.elapsed() < Duration::from_secs(10)
                        && platform::iconic(h)
                    {
                        continue;
                    }
                    if let Some(w) = self.windows.remove(&window) {
                        if event.event == EVENT_OBJECT_HIDE {
                            if let Err(e) = self.journal.restore_window(w.admission.identity) {
                                log::warn!("restoring hidden window: {e:#}");
                            }
                        }
                        self.focus_settle = Instant::now();
                        if event.event == EVENT_OBJECT_HIDE
                            && platform::class(h) == "Chrome_WidgetWin_1"
                            && !self
                                .rules
                                .resolve(&w.admission.exe, &platform::class(h), &platform::title(h))
                                .tray
                        {
                            if self.recently_hidden.len() >= 1024 {
                                self.recently_hidden.pop_front();
                            }
                            self.recently_hidden
                                .push_back((w.admission.identity, Instant::now()));
                        }
                        if event.event == EVENT_OBJECT_DESTROY {
                            let _ = self.journal.release(w.admission.identity);
                        }
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            l.remove(window);
                        }
                        self.queue.lock().unwrap().known.remove(&event.window);
                        changed = true;
                        processes_changed = true;
                    }
                }
                EVENT_OBJECT_CREATE
                | EVENT_OBJECT_SHOW
                | EVENT_OBJECT_UNCLOAKED
                | EVENT_OBJECT_NAMECHANGE => {
                    if self.windows.contains_key(&window) {
                        if event.event == EVENT_OBJECT_NAMECHANGE {
                            let mut ignored = None;
                            if let Some(w) = self.windows.get_mut(&window) {
                                let options = self.rules.resolve(
                                    &w.admission.exe,
                                    &platform::class(h),
                                    &platform::title(h),
                                );
                                if options.ignore {
                                    if let Some(l) = self.layouts.get_mut(&w.desktop) {
                                        l.remove(window);
                                    }
                                    ignored = Some(platform::Placement {
                                        identity: w.admission.identity,
                                        rect: w.admission.frame,
                                        border: w.admission.border,
                                        hung: !platform::responsive(h),
                                    });
                                } else if options.float {
                                    if let Some(l) = self.layouts.get_mut(&w.desktop) {
                                        let r = w
                                            .expected
                                            .filter(|r| {
                                                self.displays.iter().any(|m| {
                                                    monitor_rect(m.rect).contains(r.x, r.y)
                                                })
                                            })
                                            .unwrap_or(w.admission.frame);
                                        l.float_at(window, r);
                                        w.expected = None;
                                    }
                                }
                            }
                            if let Some(placement) = ignored {
                                if platform::apply(std::slice::from_ref(&placement)).is_ok()
                                    && !placement.hung
                                {
                                    let _ = self.journal.release(placement.identity);
                                }
                                self.windows.remove(&window);
                                self.queue.lock().unwrap().known.remove(&event.window);
                                changed = true;
                                processes_changed = true;
                                continue;
                            }
                        }
                        if self.current_desktop(h) {
                            if let Some(desktop) = self.desktop(h) {
                                self.current = desktop;
                            }
                            changed = true;
                        }
                    } else {
                        let n = self.windows.len();
                        self.admit(window);
                        changed |= self.windows.len() != n;
                        processes_changed |= self.windows.len() != n;
                    }
                    if platform::desktop_window(h)
                        || matches!(platform::class(h).as_str(), "Progman" | "WorkerW")
                    {
                        self.current = self
                            .desktop(platform::foreground())
                            .unwrap_or_else(|| format!("{:?}", GUID::zeroed()));
                        self.enumerate();
                        if !self.layouts.contains_key(&self.current) {
                            self.layouts
                                .insert(self.current.clone(), self.make_layout());
                        }
                        changed = true;
                    }
                }
                EVENT_SYSTEM_FOREGROUND => {
                    // SHQueryUserNotificationState is checked only on foreground events.
                    let game = platform::notification_game();
                    self.animation.pause(game || platform::fullscreen(h));
                    if game || platform::fullscreen(h) {
                        self.close_overview();
                        self.hint = None;
                    }
                    if let Some(desktop) = self.desktop(h) {
                        self.current = desktop;
                    }
                    if !self.layouts.contains_key(&self.current) {
                        self.layouts
                            .insert(self.current.clone(), self.make_layout());
                    }
                    if let Some(w) = self.windows.get(&window) {
                        // A visible transient foreground after close must not discard the
                        // remembered view. Off-view activation is still revealed immediately.
                        if self.focus_settle.elapsed() < Duration::from_millis(500)
                            && w.expected.is_some_and(|r| {
                                self.displays
                                    .iter()
                                    .any(|m| monitor_rect(m.rect).contains(r.x, r.y))
                            })
                        {
                            continue;
                        }
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            l.focus(window);
                            changed = true;
                        }
                    } else {
                        let before = self.windows.len();
                        self.admit(window);
                        changed |= self.windows.len() != before;
                        processes_changed |= self.windows.len() != before;
                    }
                }
                EVENT_SYSTEM_MINIMIZESTART => {
                    if self.changed_at.elapsed() < Duration::from_secs(10) {
                        continue;
                    }
                    if let Some(w) = self.windows.get_mut(&window) {
                        self.focus_settle = Instant::now();
                        w.minimized = true;
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            l.remove(window);
                            changed = true;
                        }
                    }
                }
                EVENT_SYSTEM_MINIMIZEEND => {
                    if let Some(w) = self.windows.remove(&window) {
                        self.queue.lock().unwrap().known.remove(&event.window);
                        drop(w);
                    }
                    self.admit(window);
                    changed = true;
                    processes_changed = true;
                }
                EVENT_SYSTEM_MOVESIZESTART => {
                    self.display_events.dragging(true);
                    self.dragging = Some(window);
                    self.cancel_projection();
                    if let Some(point) = platform::cursor() {
                        self.drag_origin = Some((f64::from(point.x), f64::from(point.y)));
                    }
                }
                EVENT_SYSTEM_MOVESIZEEND => {
                    self.display_events.dragging(false);
                    self.hint = None;
                    self.dragging = None;
                    let point = platform::cursor().unwrap_or_default();
                    if let Some(w) = self.windows.get_mut(&window) {
                        w.moves.clear();
                        w.expected = None;
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            let (x, y) = (f64::from(point.x), f64::from(point.y));
                            if self
                                .drag_origin
                                .take()
                                .is_some_and(|(ox, oy)| (x - ox).hypot(y - oy) > 256.0)
                            {
                                if l.is_floating(window) {
                                    if let Some(actual) = platform::frame(h) {
                                        l.move_floating(window, actual);
                                    }
                                } else if l.targets().iter().any(|t| {
                                    t.window != window
                                        && t.visible
                                        && !t.floating
                                        && t.rect.contains(x, y)
                                }) {
                                    l.drop_at(window, x, y);
                                } else if let Some(actual) = platform::frame(h) {
                                    l.float_at(window, actual);
                                }
                            } else if let Some(actual) = platform::frame(h) {
                                if let Some((mi, _)) = l.locate(window) {
                                    let m = &l.monitors[mi];
                                    let along = match m.axis {
                                        glaze_tiling::Axis::Horizontal => actual.w / m.scale,
                                        glaze_tiling::Axis::Vertical => actual.h / m.scale,
                                    };
                                    if l.is_floating(window) {
                                        l.move_floating(window, actual);
                                    } else {
                                        l.resize_column(window, along);
                                    }
                                }
                            }
                        }
                    }
                    changed = true;
                }
                EVENT_OBJECT_LOCATIONCHANGE => {
                    if animation::busy() {
                        self.queue.lock().unwrap().deferred.insert(event.window);
                        continue;
                    }
                    if self.dragging == Some(window) {
                        self.drag_hint(window);
                        continue;
                    }
                    if !self.current_desktop(h) {
                        continue;
                    }
                    let Some(w) = self.windows.get_mut(&window) else {
                        continue;
                    };
                    if !w.admission.identity.valid() || w.minimized {
                        continue;
                    }
                    let fullscreen = platform::fullscreen(h) || platform::topmost(h);
                    if fullscreen != w.fullscreen {
                        w.fullscreen = fullscreen;
                        w.expected = None;
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            if fullscreen {
                                w.suspended = l.suspend(window);
                                let _ = self.journal.release(w.admission.identity);
                            } else if let Some(saved) = w.suspended.take() {
                                l.resume(saved, platform::foreground() == h);
                            }
                        }
                        changed = true;
                        continue;
                    }
                    if fullscreen {
                        continue;
                    }
                    let Some(actual) = platform::frame(h) else {
                        continue;
                    };
                    let maximized = platform::zoomed(h);
                    if maximized != w.maximized {
                        w.maximized = maximized;
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            l.set_maximized(window, maximized);
                        }
                        changed = true;
                        continue;
                    }
                    if w.expected.is_some_and(|r| near(r, actual)) {
                        continue;
                    }
                    if let Some(l) = self.layouts.get_mut(&w.desktop) {
                        if l.is_floating(window) {
                            l.move_floating(window, actual);
                            w.expected = Some(actual);
                            continue;
                        }
                    }
                    // An external move must force re-application even if the model is unchanged.
                    w.expected = None;
                    let now = Instant::now();
                    w.moves
                        .retain(|at| now.duration_since(*at) < Duration::from_secs(1));
                    w.moves.push_back(now);
                    if w.moves.len() >= 4 {
                        w.moves.clear();
                        w.offences += 1;
                        if let Some(l) = self.layouts.get_mut(&w.desktop) {
                            if w.offences == 1 {
                                let along = l
                                    .locate(window)
                                    .map(|(mi, _)| match l.monitors[mi].axis {
                                        glaze_tiling::Axis::Horizontal => {
                                            actual.w / l.monitors[mi].scale
                                        }
                                        glaze_tiling::Axis::Vertical => {
                                            actual.h / l.monitors[mi].scale
                                        }
                                    })
                                    .unwrap_or(actual.w);
                                l.resize_column(window, along);
                            } else {
                                l.float_at(window, actual);
                                let _ = self.learned.record(&w.admission.exe, 0.0, 0.0, true);
                            }
                        }
                    }
                    changed = true;
                }
                _ => {}
            }
        }
        if processes_changed {
            self.update_process_hooks();
        }
        if changed {
            if let Err(e) = self.commit(true) {
                log::error!("tiling layout: {e:#}");
            }
            self.refresh_overview();
        }
    }
    pub fn command(&mut self, command: Command) {
        if command == Command::Overview {
            if self.overview.is_some() {
                self.close_overview();
                let _ = self.commit(false);
                self.activate_focused();
            } else {
                self.cancel_projection();
                if let Some(l) = self.layouts.get(&self.current) {
                    match overview::Overview::open(l, self.scope.is_some()) {
                        Ok(o) => {
                            self.overview = Some(o);
                            glaze_bars::flyouts::winkey::set_overview(true);
                        }
                        Err(e) => log::warn!("overview: {e:#}"),
                    }
                }
            }
            return;
        }
        if let Some(l) = self.layouts.get_mut(&self.current) {
            l.command(command);
        }
        if let Err(e) = self.commit(true) {
            log::error!("tiling shortcut: {e:#}");
            return;
        }
        if self.overview.is_some() {
            self.refresh_overview();
            return;
        }
        self.activate_focused();
    }
    /// Only a real user shortcut or overview click can request native foreground rights.
    fn activate_focused(&self) {
        if self.scope.is_none() && !self.animation.paused() {
            if let Some(id) = self.layouts.get(&self.current).and_then(Layout::focused) {
                /* SAFETY: only a real user shortcut asks for foreground, never a fixture. */
                unsafe {
                    let _ = SetForegroundWindow(hwnd(id));
                }
            }
        }
    }
    fn close_overview(&mut self) {
        self.focus_settle = Instant::now();
        self.overview = None;
        glaze_bars::flyouts::winkey::set_overview(false);
    }
    fn drag_hint(&mut self, window: WindowId) {
        let Some(point) = platform::cursor() else {
            return;
        };
        let (x, y) = (f64::from(point.x), f64::from(point.y));
        if !self
            .drag_origin
            .is_some_and(|(ox, oy)| (x - ox).hypot(y - oy) > 256.0)
        {
            return;
        }
        let Some(l) = self.layouts.get_mut(&self.current) else {
            return;
        };
        if l.is_floating(window) {
            self.hint = None;
            return;
        }
        let target = l
            .targets()
            .into_iter()
            .find(|t| t.window != window && t.visible && !t.floating && t.rect.contains(x, y));
        if let Some(t) = target {
            if self.hint.is_none() {
                self.hint = platform::InsertHint::new().ok();
            }
            let horizontal = l
                .locate(t.window)
                .is_none_or(|(mi, _)| l.monitors[mi].axis == glaze_tiling::Axis::Horizontal);
            let fraction = if horizontal {
                (x - t.rect.x) / t.rect.w
            } else {
                (y - t.rect.y) / t.rect.h
            };
            let r = if fraction < 0.3 {
                if horizontal {
                    Rect { w: 4.0, ..t.rect }
                } else {
                    Rect { h: 4.0, ..t.rect }
                }
            } else if fraction > 0.7 {
                if horizontal {
                    Rect {
                        x: t.rect.x + t.rect.w - 4.0,
                        w: 4.0,
                        ..t.rect
                    }
                } else {
                    Rect {
                        y: t.rect.y + t.rect.h - 4.0,
                        h: 4.0,
                        ..t.rect
                    }
                }
            } else {
                Rect {
                    y: t.rect.y + t.rect.h / 2.0,
                    h: 4.0,
                    ..t.rect
                }
            };
            if let Some(hint) = &self.hint {
                hint.place(r);
            }
        } else {
            self.hint = None;
        }
        if self.edge_at.elapsed() > Duration::from_millis(150) {
            if let Some(m) = l.monitors.iter().find(|m| m.area.contains(x, y)) {
                let direction = if m.axis == glaze_tiling::Axis::Horizontal {
                    if x < m.area.x + 30.0 {
                        Some(glaze_tiling::Direction::Left)
                    } else if x > m.area.x + m.area.w - 30.0 {
                        Some(glaze_tiling::Direction::Right)
                    } else {
                        None
                    }
                } else if y < m.area.y + 30.0 {
                    Some(glaze_tiling::Direction::Up)
                } else if y > m.area.y + m.area.h - 30.0 {
                    Some(glaze_tiling::Direction::Down)
                } else {
                    None
                };
                if let Some(direction) = direction {
                    l.command(Command::Focus(direction));
                    self.edge_at = Instant::now();
                    let _ = self.commit(true);
                }
            }
        }
    }
    fn refresh_overview(&mut self) {
        if let (Some(overview), Some(layout)) = (&self.overview, self.layouts.get(&self.current)) {
            if overview.matches(layout) {
                return;
            }
        }
        if self.overview.is_some() {
            self.overview = None;
            if let Some(l) = self.layouts.get(&self.current) {
                self.overview = overview::Overview::open(l, self.scope.is_some()).ok();
            }
            glaze_bars::flyouts::winkey::set_overview(self.overview.is_some());
        }
    }
    pub fn displays_changed(&mut self) {
        self.cancel_projection();
        self.refresh_displays();
        if let Err(e) = self.commit(false) {
            log::error!("tiling displays: {e:#}");
        }
    }
    /// Core's game signal also reaches us while the shared WinEvent source is suspended.
    pub fn set_game_activity(&mut self, on: bool) {
        self.animation.pause(on);
        if on {
            for window in self.windows.values_mut() {
                window.expected = None;
            }
            self.close_overview();
            self.hint = None;
            self.display_events.dragging(false);
            self.dragging = None;
        } else {
            self.refresh_displays();
            self.enumerate();
            let _ = self.commit(false);
        }
    }
    /// Cancelled previews never make their uncommitted targets look like native window state.
    fn cancel_projection(&mut self) {
        self.animation.cancel();
        for window in self.windows.values_mut() {
            window.expected = None;
            window.scale = platform::dpi_scale(window.admission.identity.window());
        }
    }
    fn commit(&mut self, animate: bool) -> Result<()> {
        let Some(layout) = self.layouts.get(&self.current) else {
            return Ok(());
        };
        indicator::publish(layout, &self.displays);
        let Some(bounds) = bounds(&self.displays) else {
            return Ok(());
        };
        let mut changes = Vec::new();
        let mut changed = false;
        for t in layout.targets() {
            if !self.current_desktop(hwnd(t.window)) {
                continue;
            }
            let Some(w) = self.windows.get_mut(&t.window) else {
                continue;
            };
            if w.fullscreen
                || w.minimized
                || self.dragging == Some(t.window)
                || !w.admission.identity.valid()
            {
                continue;
            }
            let target = if t.visible {
                t.rect
            } else {
                park(bounds, t.rect)
            };
            changed |= w.expected.is_none_or(|old| !near(old, target));
            if !animation::busy() {
                changed |=
                    platform::frame(hwnd(t.window)).is_none_or(|actual| !near(actual, target));
            }
            self.journal.remember(w.admission.identity)?;
            let scale = layout
                .locate(t.window)
                .map_or(w.scale, |(mi, _)| layout.monitors[mi].scale);
            let dpi_changed = (scale - w.scale).abs() > 0.001;
            w.scale = scale;
            changes.push(animation::Change {
                identity: w.admission.identity,
                from: w
                    .expected
                    .or_else(|| platform::frame(hwnd(t.window)))
                    .unwrap_or(target),
                to: target,
                clip: layout
                    .locate(t.window)
                    .map_or(bounds, |(mi, _)| layout.monitors[mi].full),
                border: w.admission.border,
                visible: t.visible,
                dpi_changed,
            });
            w.expected = Some(target);
        }
        if changed || changes.is_empty() {
            self.animation
                .apply(changes, animate && self.settings.animations)?;
        }
        Ok(())
    }
}
impl Drop for Tiler {
    fn drop(&mut self) {
        indicator::clear();
        glaze_bars::flyouts::winkey::set_overview(false);
        self.animation.cancel();
        if let Err(e) = self.journal.restore() {
            log::error!("tiling restore: {e:#}");
        }
    }
}
fn near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 2.0
        && (a.y - b.y).abs() < 2.0
        && (a.w - b.w).abs() < 2.0
        && (a.h - b.h).abs() < 2.0
}
