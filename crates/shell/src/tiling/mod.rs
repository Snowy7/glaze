//! Opt-in scrolling window management. No object, subscription, worker or timer exists while off.
pub mod animation;
pub(crate) mod indicator;
mod learned;
mod overview;
mod platform;
mod parking;
use parking::{bounds, monitor_rect, park};
mod rules;
use anyhow::Result;
use mypaper_desktop::{
    tiling_recovery::{self, Journal},
    win_events::{self, Subscription, TilingHooks},
};
use mypaper_tiling::{Command, Constraints, Layout, Rect, Settings, WindowId};
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
    suspended: Option<mypaper_tiling::Suspended>,
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
    displays: Vec<mypaper_desktop::Monitor>,
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
        let queue = Arc::new(Mutex::new(Queue::default()));
        let events = queue.clone();
        let event_scope = scope.clone();
        let subscription = Subscription::new(move |event| {
            let mut q = events.lock().unwrap();
            if event_scope.is_some()
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
        let paths = mypaper_desktop::display_paths();
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
        let displays = mypaper_desktop::monitor_infos();
        if displays == self.displays {
            return;
        }
        self.displays = displays;
        self.changed_at = Instant::now();
        let paths = mypaper_desktop::display_paths();
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
                    queue.events.push_back(win_events::Event { event: EVENT_OBJECT_LOCATIONCHANGE, window });
                }
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
                            mypaper_tiling::Axis::Horizontal => width,
                            mypaper_tiling::Axis::Vertical => height,
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
                                mypaper_tiling::Axis::Horizontal => {
                                    self.learned.record(&w.admission.exe, width, 0.0, false)
                                }
                                mypaper_tiling::Axis::Vertical => {
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
                        self.overview = None;
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
                EVENT_OBJECT_LOCATIONCHANGE => {
                    if animation::busy() {
                        self.queue.lock().unwrap().deferred.insert(event.window);
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
                                        mypaper_tiling::Axis::Horizontal => {
                                            actual.w / l.monitors[mi].scale
                                        }
                                        mypaper_tiling::Axis::Vertical => {
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
        }
    }
    pub fn displays_changed(&mut self) {
        self.cancel_projection();
        self.refresh_displays();
        if let Err(e) = self.commit(false) {
            log::error!("tiling displays: {e:#}");
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
        mypaper_bars::flyouts::winkey::set_overview(false);
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
