//! The bars host's state and main loop (docs/bars.md): bars per monitor, the native taskbar, the
//! hold, every page request, and the data sources behind the modules.
//!
//! One thread. It sleeps in `MsgWaitForMultipleObjectsEx` until a window message, a [`Msg`] from
//! a worker, the virtual-desktop registry event, or the earliest timer (debounces, the clock, the
//! stats sample). Sources run only while some bar on screen shows their module.
//!
//! Each bar keeps `bars.json` as its monitor sees it (with `overrides`). The look a bar gets
//! besides its config is in `looks`; attention, custom modules, the trash and previews in
//! `extras`.

mod extras;
mod looks;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use mypaper_bars::config::{self, BarsConfig, DockPosition, Pinned, TaskbarMode, TopPosition};
use mypaper_bars::taskbar::{NativeShell, Taskbar, Wanted};
use mypaper_config::Config;
use mypaper_desktop::hold::{Fact, Holds, Reason};
use mypaper_desktop::{
    ControlWindow, Monitor, WM_FACT, fullscreen_app_monitor, monitor_infos, startup,
};
use mypaper_theme::Color;
use notify::Watcher;
use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE, ICoreWebView2Environment,
};
use windows::Win32::Foundation::{HANDLE, HWND, POINT, WAIT_OBJECT_0};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows::Win32::UI::Shell::{ABN_FULLSCREENAPP, ABN_POSCHANGED, ABN_STATECHANGE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetCursorPos, GetForegroundWindow, GetShellWindow, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetForegroundWindow, TranslateMessage, WM_CLOSE, WM_QUIT,
};
use windows::core::{PCWSTR, w};

use crate::appbar::Edge;
use crate::apps::{self, Apps, Pin, Running};
use crate::assets::ASSETS_HOST;
use crate::audio::Audio;
use crate::bar::{
    self, Bar, Geometry, WM_BARS_ACTIVATED, WM_BARS_APPBAR, WM_BARS_DISPLAY, WM_BARS_EDGE,
    WM_BARS_POWER, WM_BARS_SETTINGS, WM_BARS_SHELL, WM_BARS_TASKBAR, WM_BARS_TIME, WM_BARS_TRASH,
};
use crate::bridge::{
    Activate, BarError, BarKind, Button, Envelope, Method, Topic, TrashAction, bar_topics,
};
use crate::desktops::{Desktops, Workspaces};
use crate::first_paint::FirstPaint;
use crate::flyouts::language::{self as lang, LanguageWatch};
use crate::flyouts::winkey::WinKey;
use crate::flyouts::{Done, Flyouts, Request, Watched};
use crate::glass::BACKDROP_HOST;
use crate::hooks::{self, Hooks};
use crate::icons::ICON_HOST;
use crate::icons::{Icons, Source};
use crate::look::PACKS_HOST;
use crate::look::{self, ThemeSkin};
use crate::media::{Media, MediaInfo};
use crate::network::{self, NetworkInfo, NetworkWatch};
use crate::system::{self, Sampler, Stats};
use crate::trash;
use crate::tray::{Tray, TrayItem};
use crate::tray_data::Click;
use crate::tray_host::{self, TrayHost};
use crate::wake::{Msg, WM_WAKE, Waker};

/// The flyouts' and the tray host's methods of `App`.
#[path = "flyouts/glue.rs"]
mod glue;
use crate::web::{self, Events, Folders, PAGE_HOST, Web, WebEvent};

/// The class of the host's message-only window: core finds it to stop the bars (`WM_CLOSE`).
pub const HOST_CLASS: PCWSTR = w!("mypaper.bars.host");

const SETTLE_WINDOWS: Duration = Duration::from_millis(50);
const SETTLE_FULLSCREEN: Duration = Duration::from_millis(100);
const SETTLE_CONFIG: Duration = Duration::from_millis(100);
const SETTLE_PAGE: Duration = Duration::from_millis(500);
const SETTLE_DISPLAY: Duration = Duration::from_millis(300);
const SETTLE_APPBAR: Duration = Duration::from_millis(50);
const SETTLE_NETWORK: Duration = Duration::from_millis(500);
const SETTLE_SETTINGS: Duration = Duration::from_millis(250);
/// Explorer needs a moment after `TaskbarCreated` before its taskbar takes settings.
const SETTLE_EXPLORER: Duration = Duration::from_secs(1);
const STATS_EVERY: Duration = Duration::from_secs(2);
const RETRY_WEBVIEW: Duration = Duration::from_secs(30);
/// Re-hiding a taskbar that Explorer keeps showing happens at most this often.
const REHIDE_SPACING: Duration = Duration::from_millis(50);

enum Env {
    None,
    Creating(u64),
    Ready(ICoreWebView2Environment),
}

/// Deadlines.
#[derive(Default)]
struct Timers {
    windows: Option<Instant>,
    fullscreen: Option<Instant>,
    config: Option<Instant>,
    theme: Option<Instant>,
    page: Option<Instant>,
    display: Option<Instant>,
    appbar: Option<Instant>,
    explorer: Option<Instant>,
    rehide: Option<Instant>,
    network: Option<Instant>,
    clock: Option<Instant>,
    stats: Option<Instant>,
    webview: Option<Instant>,
    settings: Option<Instant>,
    /// The next custom module due to run.
    custom: Option<Instant>,
}

impl Timers {
    fn earliest(&self) -> Option<Instant> {
        [
            self.windows,
            self.fullscreen,
            self.config,
            self.theme,
            self.page,
            self.display,
            self.appbar,
            self.explorer,
            self.rehide,
            self.network,
            self.clock,
            self.stats,
            self.webview,
            self.settings,
            self.custom,
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

/// Takes a timer if it's due.
fn due(timer: &mut Option<Instant>, now: Instant) -> bool {
    timer.take_if(|at| *at <= now).is_some()
}

/// Sets a debounce timer unless one is already pending.
fn soon(timer: &mut Option<Instant>, after: Duration) {
    timer.get_or_insert_with(|| Instant::now() + after);
}

/// The built page: `bars/shell/dist` beside the exe or any parent, else under the current folder
/// or any parent (a dev build run from the repo).
fn page_dir() -> Option<PathBuf> {
    let found = |start: &Path| {
        start
            .ancestors()
            .map(|dir| dir.join("bars").join("shell").join("dist"))
            .find(|dir| dir.join("index.html").is_file())
    };
    let exe = std::env::current_exe().ok();
    let from_exe = exe.as_deref().and_then(Path::parent).and_then(found);
    from_exe.or_else(|| found(&std::env::current_dir().ok()?))
}

fn built_at(page: &Path) -> Option<SystemTime> {
    std::fs::metadata(page.join("index.html"))
        .and_then(|m| m.modified())
        .ok()
}

/// The native taskbar's pinned shortcuts, in name order: the dock's pins until the user picks
/// their own. Without any, File Explorer.
fn taskbar_pins() -> Vec<Pinned> {
    let folder = std::env::var_os("APPDATA").map(|appdata| {
        PathBuf::from(appdata).join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
    });
    let mut links: Vec<PathBuf> = folder
        .and_then(|folder| std::fs::read_dir(folder).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        })
        .collect();
    links.sort();
    let pins: Vec<Pinned> = links
        .into_iter()
        .map(|path| Pinned {
            path: path.display().to_string(),
            name: None,
        })
        .collect();
    if pins.is_empty() {
        vec![Pinned {
            path: r"C:\Windows\explorer.exe".into(),
            name: Some("File Explorer".into()),
        }]
    } else {
        pins
    }
}

/// What a pin's own path says about its app, before the icon worker has looked at it.
fn direct_identity(path: &str) -> (Option<String>, Option<String>) {
    let lower = path.to_lowercase();
    if lower.starts_with(r"shell:appsfolder\") {
        (Some(path[r"shell:AppsFolder\".len()..].to_owned()), None)
    } else if lower.ends_with(".exe") {
        (None, Some(lower))
    } else {
        (None, None)
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// When the clock should tick next: the next whole minute (or second), a hair after it.
fn next_tick(now_ms: u64, seconds: bool) -> Duration {
    let unit = if seconds { 1_000 } else { 60_000 };
    Duration::from_millis(unit - now_ms % unit + 5)
}

pub struct App {
    control: ControlWindow,
    /// Hears broadcasts and the shell hook; the Recycle Bin's notifications go to it.
    broadcast: HWND,
    rx: Receiver<Msg>,
    waker: Waker,
    events: Events,
    env: Env,
    env_generation: u64,
    bars: Vec<Bar>,
    next_bar: u32,
    monitors: Vec<Monitor>,
    page: Option<PathBuf>,
    /// When the page's `index.html` was built: a rebuild changes it, reading it doesn't.
    page_built: Option<SystemTime>,
    devtools: bool,

    config_path: PathBuf,
    /// `bars.json`, resolved (the last good one if the file is broken).
    config: BarsConfig,
    taskbar: Taskbar<NativeShell>,
    last_rehide: Instant,
    skin: ThemeSkin,
    palette: Option<Color>,

    holds: Holds,
    /// Display off or session locked: every bar is suspended.
    asleep: bool,
    /// The device of the monitor a fullscreen app covers.
    fullscreen: Option<String>,

    hooks: Option<Hooks>,
    apps: Apps,
    /// The last window in front that isn't ours (clicking a bar takes the foreground).
    foreground: Option<isize>,
    icons: Icons,
    media: Media,
    media_state: Option<MediaInfo>,
    audio: Option<Audio>,
    network: Option<NetworkWatch>,
    network_state: Option<NetworkInfo>,
    tray: Tray,
    /// The notification area's own host (docs/bars.md, "Tray host"); `tray` is its fallback.
    tray_host: TrayHost,
    tray_items: Vec<TrayItem>,
    desktops: Option<Desktops>,
    workspaces: Workspaces,
    sampler: Option<Sampler>,
    stats: Option<Stats>,
    /// The flyouts' providers, watches and the launcher's index (docs/bars.md, "Flyouts").
    flyouts: Flyouts,
    /// The Windows key hook, while the launcher replaces Start.
    winkey: Option<WinKey>,
    language_watch: Option<LanguageWatch>,
    /// The installed keyboard layouts, read when the `language` topic first needs them.
    layouts: Option<Vec<lang::Layout>>,
    /// The notification count, once the lane has counted.
    notification_count: Option<Option<usize>>,
    /// The bar that took the keyboard for a flyout (the launcher): it keeps the foreground.
    keyboard: Option<u32>,
    looks: looks::Looks,
    extras: extras::Extras,

    /// Topics being produced right now.
    active: BTreeSet<Topic>,
    /// What each bar last got per topic, so unchanged values aren't re-sent.
    sent: HashMap<(u32, Topic), String>,
    timers: Timers,
    watchers: Vec<notify::RecommendedWatcher>,
    theme_watch: Option<(PathBuf, notify::RecommendedWatcher)>,
    _palette_watch: Option<mypaper_desktop::ConfigWatcher>,
    /// The native stand-ins shown until the pages paint, and the late topics' last values.
    first_paint: FirstPaint,
}

impl App {
    pub fn new() -> Result<Self> {
        // SAFETY: COM for the UI thread, which WebView2 requires to be an STA.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .context("CoInitializeEx")?;
        let control = ControlWindow::new(HOST_CLASS)?;
        bar::register_classes()?;
        let broadcast = bar::broadcast_window()?;
        let (tx, rx) = mpsc::channel();
        // SAFETY: no preconditions.
        let waker = Waker::new(tx, unsafe { GetCurrentThreadId() });

        let settings = Config::load().unwrap_or_else(|e| {
            log::error!("{e:#}\n(using the default settings)");
            Config::default()
        });
        let mut holds = Holds::default();
        holds.apply(&settings);

        let config_path = config::path()?;
        let mut config = BarsConfig::load(&config_path).unwrap_or_else(|e| {
            log::error!("{e:#}\n(using the defaults)");
            BarsConfig::default()
        });
        config.resolve(taskbar_pins);
        let page = page_dir();
        if page.is_none() {
            log::error!("bars/shell/dist not found; build it with `pnpm -C bars/shell build`");
        }
        let icons_dir = mypaper_bars::files::local("icons")?;
        let monitors = monitor_infos();
        let mut app = Self {
            control,
            broadcast,
            rx,
            events: Events::default(),
            env: Env::None,
            env_generation: 0,
            bars: Vec::new(),
            next_bar: 0,
            monitors,
            page_built: page.as_deref().and_then(built_at),
            page,
            devtools: std::env::var_os("MYPAPER_BARS_DEVTOOLS").is_some_and(|v| v == "1"),
            config_path,
            config,
            taskbar: Taskbar::native()?,
            last_rehide: Instant::now(),
            skin: ThemeSkin::load(),
            palette: look::palette_accent(),
            holds,
            asleep: false,
            fullscreen: None,
            hooks: None,
            apps: Apps::default(),
            foreground: None,
            icons: Icons::new(waker.clone(), icons_dir.clone()),
            media: Media::new(waker.clone()),
            media_state: None,
            audio: None,
            network: None,
            network_state: None,
            tray: Tray::new(waker.clone(), icons_dir.clone()),
            tray_host: TrayHost::new(waker.clone()),
            tray_items: Vec::new(),
            desktops: None,
            workspaces: Workspaces {
                count: 1,
                current: 0,
            },
            sampler: None,
            stats: None,
            flyouts: Flyouts::new(waker.clone(), icons_dir.clone()),
            winkey: None,
            language_watch: None,
            layouts: None,
            notification_count: None,
            keyboard: None,
            looks: looks::Looks::new(waker.clone())?,
            extras: extras::Extras::new(waker.clone()),
            active: BTreeSet::new(),
            sent: HashMap::new(),
            timers: Timers::default(),
            watchers: Vec::new(),
            theme_watch: None,
            _palette_watch: None,
            first_paint: FirstPaint::load(),
            waker,
        };
        app.holds
            .set(Fact::OnBattery, mypaper_desktop::on_battery());
        // The native stand-ins first: they're on screen while the bars reserve their space and
        // WebView2 starts (docs/performance.md, "Native first paint").
        app.refresh_fullscreen();
        let looks = [BarKind::Top, BarKind::Dock].map(|kind| app.look(kind));
        let look = |kind| looks[usize::from(kind == BarKind::Dock)].clone();
        let covered = app.fullscreen.as_deref();
        app.first_paint
            .show_early(&app.monitors, &app.config, covered, look);
        log::info!(
            "taskbar {:?}, top bar {}, dock {}",
            app.config.taskbar.mode,
            if app.config.top.enabled { "on" } else { "off" },
            if app.config.dock.enabled { "on" } else { "off" },
        );
        app.flyouts.switch_output = app.config.audio.switch_output;
        app.apply_taskbar();
        app.watch();
        app.reconcile();
        app.refresh_look();
        app.refresh_fullscreen();
        app.sync_hold();
        Ok(app)
    }

    /// Runs until `WM_QUIT` (or core's `WM_CLOSE`); returns the exit code.
    pub fn run(&mut self) -> u8 {
        loop {
            if let Some(code) = self.pump() {
                return code;
            }
            self.handle_web_events();
            while let Ok(msg) = self.rx.try_recv() {
                self.handle(msg);
            }
            self.handle_web_events();
            self.tick(Instant::now());
            self.wait();
        }
    }

    fn wait(&mut self) {
        let earliest = [self.timers.earliest(), self.flyouts.deadline()]
            .into_iter()
            .flatten()
            .min();
        let timeout = earliest.map_or(INFINITE, |at| {
            let left = at.saturating_duration_since(Instant::now());
            left.as_millis().min(u128::from(INFINITE - 1)) as u32
                + u32::from(left.subsec_micros() % 1000 != 0)
        });
        if timeout == 0 {
            return;
        }
        let handles: Vec<HANDLE> = self.desktops.iter().map(Desktops::event).collect();
        // SAFETY: waits on our own queue and handles we own.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if !handles.is_empty() && woke == WAIT_OBJECT_0 {
            if let Some(desktops) = &self.desktops {
                self.workspaces = desktops.changed();
            }
            self.emit(Topic::Workspaces);
        }
    }

    fn pump(&mut self) -> Option<u8> {
        let mut msg = MSG::default();
        // SAFETY: the standard message pump on the thread that owns our windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                match msg.message {
                    WM_QUIT => return Some(msg.wParam.0 as u8),
                    // Core asks us to stop; Drop restores the taskbar.
                    WM_CLOSE if msg.hwnd == self.control.hwnd() => {
                        log::info!("asked to stop");
                        return Some(0);
                    }
                    WM_FACT => {
                        if let Some(fact) = Fact::from_wparam(msg.wParam.0) {
                            self.holds.set(fact, msg.lParam.0 != 0);
                            self.sync_hold();
                        }
                    }
                    WM_WAKE => {}
                    WM_BARS_APPBAR => match msg.wParam.0 as u32 {
                        ABN_POSCHANGED | ABN_STATECHANGE => {
                            soon(&mut self.timers.appbar, SETTLE_APPBAR)
                        }
                        ABN_FULLSCREENAPP => soon(&mut self.timers.fullscreen, Duration::ZERO),
                        _ => {}
                    },
                    WM_BARS_EDGE => self.reveal(msg.lParam.0),
                    WM_BARS_DISPLAY => soon(&mut self.timers.display, SETTLE_DISPLAY),
                    // The tray host's own broadcast (asking apps for their icons) isn't a restart.
                    WM_BARS_TASKBAR if tray_host::own_broadcast() => {}
                    WM_BARS_TASKBAR => {
                        log::info!("Explorer (re)started");
                        soon(&mut self.timers.explorer, SETTLE_EXPLORER);
                        self.tray_host.explorer_restarted();
                    }
                    WM_BARS_POWER => {
                        let on_battery = mypaper_desktop::on_battery();
                        self.holds.set(Fact::OnBattery, on_battery);
                        self.emit(Topic::Battery);
                        self.power_changed(on_battery);
                    }
                    WM_BARS_SHELL => self.on_shell(msg.wParam.0, msg.lParam.0),
                    WM_BARS_SETTINGS => soon(&mut self.timers.settings, SETTLE_SETTINGS),
                    WM_BARS_TRASH => self.trash_changed(),
                    WM_BARS_TIME => {
                        self.timers.clock = None;
                        self.sync_clock();
                    }
                    // A flyout that took the keyboard (the launcher) keeps the foreground.
                    WM_BARS_ACTIVATED if self.keyboard.is_some() => {}
                    WM_BARS_ACTIVATED => {
                        log::debug!("a bar was activated without a click; handing it back");
                        self.hand_back(None);
                    }
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
        None
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Config => soon(&mut self.timers.config, SETTLE_CONFIG),
            Msg::Theme => soon(&mut self.timers.theme, SETTLE_CONFIG),
            Msg::Page => soon(&mut self.timers.page, SETTLE_PAGE),
            Msg::Described(source, described) => {
                self.icons.on_described(source, described);
                self.emit(Topic::Apps);
                self.emit(Topic::Foreground);
            }
            Msg::Media(state) => {
                self.media_state = state;
                self.first_paint.reported(Topic::Media);
                self.emit(Topic::Media);
            }
            Msg::Volume => {
                if let Some(audio) = &mut self.audio {
                    audio.follow_default();
                }
                self.emit(Topic::Volume);
            }
            Msg::Network => soon(&mut self.timers.network, SETTLE_NETWORK),
            Msg::Tray(items) => {
                self.tray_items = items;
                self.first_paint.reported(Topic::Tray);
                self.emit(Topic::Tray);
            }
            Msg::Flyout(done) => self.on_flyout(done),
            Msg::Backdrop => self.backdrop_changed(),
            Msg::Custom(id, output) => {
                if self.extras.custom.finished(id, output) {
                    self.emit(Topic::Custom);
                }
            }
            Msg::WatchError(path, error) => {
                log::warn!("stopped watching {}: {error}", path.display())
            }
        }
    }

    fn tick(&mut self, now: Instant) {
        self.first_paint.expire(now);
        if let Some(hooks) = &self.hooks {
            let happened = hooks.take();
            if happened & hooks::WINDOWS != 0 {
                soon(&mut self.timers.windows, SETTLE_WINDOWS);
            }
            if happened & hooks::FOREGROUND != 0 {
                self.on_foreground();
            }
            if happened & (hooks::FOREGROUND | hooks::MOVED) != 0 {
                soon(&mut self.timers.fullscreen, SETTLE_FULLSCREEN);
            }
            if happened & hooks::TASKBAR_SHOWN != 0 {
                self.rehide_taskbar(now); // at once: it must never be seen
            }
        }
        if due(&mut self.timers.windows, now) && self.apps.refresh() {
            self.emit(Topic::Apps);
        }
        if due(&mut self.timers.fullscreen, now) {
            self.refresh_fullscreen();
            self.sync_hold();
        }
        if due(&mut self.timers.config, now) {
            self.reload_config();
        }
        if due(&mut self.timers.theme, now) {
            self.reload_theme();
        }
        if due(&mut self.timers.page, now) {
            self.reload_page();
        }
        if due(&mut self.timers.display, now) {
            self.displays_changed();
        }
        if due(&mut self.timers.appbar, now) {
            for bar in &mut self.bars {
                bar.appbar_moved();
            }
            self.push_looks();
        }
        if due(&mut self.timers.explorer, now) {
            self.explorer_restarted();
        }
        if due(&mut self.timers.rehide, now) {
            self.rehide_taskbar(now);
        }
        if due(&mut self.timers.network, now) {
            self.network_state = Some(network::read(
                self.network.as_ref().and_then(NetworkWatch::wlan),
            ));
            self.emit(Topic::Network);
        }
        if due(&mut self.timers.clock, now) {
            self.emit(Topic::Clock);
            self.sync_clock();
        }
        if due(&mut self.timers.stats, now) {
            if let Some(sampler) = &mut self.sampler {
                self.stats = Some(sampler.sample());
                self.first_paint.reported(Topic::Stats);
                self.timers.stats = Some(now + STATS_EVERY);
            }
            self.emit(Topic::Stats);
        }
        if due(&mut self.timers.webview, now) {
            self.ensure_webviews();
        }
        self.flyouts.tick(now);
        if due(&mut self.timers.settings, now) {
            self.settings_changed();
        }
        if due(&mut self.timers.custom, now) {
            self.timers.custom = self.extras.custom.tick(now);
        }
    }

    // --- Bars --------------------------------------------------------------------------------

    fn geometry(kind: BarKind, config: &BarsConfig) -> Geometry {
        match kind {
            BarKind::Top => Geometry {
                edge: match config.top.position {
                    TopPosition::Top => Edge::Top,
                    TopPosition::Bottom => Edge::Bottom,
                },
                thickness: config.top.band(),
                reserve: true,
            },
            BarKind::Dock => Geometry {
                edge: match config.dock.position {
                    DockPosition::Bottom => Edge::Bottom,
                    DockPosition::Top => Edge::Top,
                    DockPosition::Left => Edge::Left,
                    DockPosition::Right => Edge::Right,
                },
                thickness: config.dock.band(),
                reserve: !config.dock.autohide,
            },
        }
    }

    /// `bars.json` as `monitor`'s bars see it (its `overrides` applied).
    fn monitor_config(&self, monitor: &Monitor) -> BarsConfig {
        self.config
            .for_monitor(&monitor.device, monitor.primary)
            .unwrap_or_else(|e| {
                log::error!("overrides for {}: {e:#}", monitor.device);
                self.config.clone()
            })
    }

    /// Makes the bars match `bars.json` and the monitors: one window per bar per monitor.
    fn reconcile(&mut self) {
        let wanted: Vec<(BarKind, Monitor, BarsConfig)> = if self.page.is_none() {
            Vec::new()
        } else {
            self.monitors
                .iter()
                .flat_map(|monitor| {
                    let config = self.monitor_config(monitor);
                    let shows = |enabled: bool, monitors: &config::Monitors| {
                        enabled && monitors.includes(&monitor.device, monitor.primary)
                    };
                    let top = shows(config.top.enabled, &config.top.monitors);
                    let dock = shows(config.dock.enabled, &config.dock.monitors);
                    [(BarKind::Top, top), (BarKind::Dock, dock)]
                        .into_iter()
                        .filter(|(_, on)| *on)
                        .map(|(kind, _)| (kind, monitor.clone(), config.clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let before = self.bars.len();
        self.bars.retain(|bar| {
            wanted
                .iter()
                .any(|(kind, m, _)| *kind == bar.kind && m.device == bar.monitor.device)
        });
        let mut changed = self.bars.len() != before;
        // Top bars first: a dock on the same monitor then reserves past them.
        for (kind, monitor, config) in wanted {
            let geometry = Self::geometry(kind, &config);
            match self
                .bars
                .iter_mut()
                .find(|b| b.kind == kind && b.monitor.device == monitor.device)
            {
                Some(bar) => {
                    bar.config = config;
                    changed |= bar.update(monitor, geometry);
                }
                None => {
                    self.next_bar += 1;
                    match Bar::new(self.next_bar, kind, monitor, config, geometry) {
                        Ok(bar) => {
                            log::info!("{kind:?} bar on {}", bar.monitor.device);
                            self.bars.push(bar);
                            changed = true;
                        }
                        Err(e) => log::error!("creating a bar: {e:#}"),
                    }
                }
            }
        }
        if changed {
            // App bars moved the work areas: look again once they settle.
            soon(&mut self.timers.display, SETTLE_DISPLAY);
        }
        self.place_tray();
        if self.bars.is_empty() {
            if !matches!(self.env, Env::None) {
                log::info!("no bars on screen; releasing WebView2");
            }
            self.env = Env::None;
        } else {
            self.ensure_webviews();
        }
        self.sync_hooks();
        self.apply_visibility();
        self.sync_topics();
        // Windows moved: wallpaper glass lines up again.
        self.push_looks();
    }

    /// Hooks run while a bar exists (window lists, fullscreen apps) or the taskbar is hidden
    /// (Explorer re-showing it).
    fn sync_hooks(&mut self) {
        let wanted = !self.bars.is_empty() || self.config.taskbar.mode == TaskbarMode::Hide;
        match (wanted, self.hooks.is_some()) {
            (true, false) => {
                self.hooks = Some(Hooks::install());
                self.on_foreground();
            }
            (false, true) => self.hooks = None,
            _ => {}
        }
    }

    fn displays_changed(&mut self) {
        let monitors = monitor_infos();
        if monitors == self.monitors {
            return;
        }
        self.monitors = monitors;
        self.reconcile();
        // A monitor that came back has a fresh secondary taskbar.
        self.apply_taskbar();
    }

    fn explorer_restarted(&mut self) {
        for bar in &mut self.bars {
            bar.reregister();
        }
        self.apply_taskbar();
        self.tray.refresh();
        soon(&mut self.timers.display, SETTLE_DISPLAY);
    }

    fn ensure_webviews(&mut self) {
        match &self.env {
            Env::None => {
                if self.timers.webview.is_some() {
                    return; // waiting to retry
                }
                self.env_generation += 1;
                match web::create_environment(self.env_generation, &self.events) {
                    Ok(()) => self.env = Env::Creating(self.env_generation),
                    Err(e) => {
                        log::error!("{e:#}");
                        self.timers.webview = Some(Instant::now() + RETRY_WEBVIEW);
                    }
                }
            }
            Env::Creating(_) => {}
            Env::Ready(env) => {
                let env = env.clone();
                for bar in self
                    .bars
                    .iter_mut()
                    .filter(|b| b.web.is_none() && !b.creating)
                {
                    match web::create_controller(&env, bar.hwnd(), bar.id, &self.events) {
                        Ok(()) => bar.creating = true,
                        Err(e) => log::error!("creating a WebView: {e:#}"),
                    }
                }
            }
        }
    }

    fn handle_web_events(&mut self) {
        loop {
            let event = self.events.borrow_mut().pop_front();
            let Some(event) = event else { break };
            match event {
                WebEvent::Environment { generation, result } => {
                    if !matches!(self.env, Env::Creating(g) if g == generation) {
                        continue;
                    }
                    match result {
                        Ok(env) => {
                            self.env = Env::Ready(env);
                            self.ensure_webviews();
                        }
                        Err(e) => {
                            log::error!("WebView2 is unavailable: {e}");
                            self.env = Env::None;
                            self.timers.webview = Some(Instant::now() + RETRY_WEBVIEW);
                        }
                    }
                }
                WebEvent::Controller { bar, result } => self.attach(bar, result),
                WebEvent::Message { bar, source, json } => {
                    if let Some(index) = self.bars.iter().position(|b| b.id == bar) {
                        self.on_page_message(index, &source, &json);
                    }
                }
                WebEvent::ProcessFailed { bar, kind } => {
                    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                        log::warn!("the WebView2 browser process exited; starting over");
                        for bar in &mut self.bars {
                            bar.drop_web();
                        }
                        self.env = Env::None;
                        self.reconcile();
                    } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                        || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
                    {
                        log::warn!("a bar's renderer failed ({kind:?}); reloading");
                        if let Some(bar) = self.bars.iter_mut().find(|b| b.id == bar) {
                            bar.ready = false;
                            if let Some(web) = &bar.web {
                                web.reload();
                            }
                        }
                    }
                }
                WebEvent::Painted { bar } => {
                    startup::ready("bars", "first_paint_ms");
                    self.first_paint.painted(bar);
                }
            }
        }
    }

    fn attach(
        &mut self,
        id: u32,
        result: windows::core::Result<
            webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
        >,
    ) {
        let Some(page) = self.page.clone() else {
            return;
        };
        let Some(index) = self.bars.iter().position(|b| b.id == id) else {
            if let Ok(controller) = result {
                // SAFETY: COM call on a controller nobody else holds.
                let _ = unsafe { controller.Close() };
            }
            return;
        };
        let bar = &mut self.bars[index];
        bar.creating = false;
        let controller = match result {
            Ok(controller) => controller,
            Err(e) => return log::error!("creating a WebView: {e}"),
        };
        let packs = look::packs_dir().unwrap_or_else(|| self.looks.assets.dir().to_owned());
        let images = [
            (ICON_HOST, self.icons.dir()),
            (PACKS_HOST, packs.as_path()),
            (BACKDROP_HOST, self.looks.glass.dir()),
            (ASSETS_HOST, self.looks.assets.dir()),
        ];
        let folders = Folders {
            page: &page,
            images: &images,
        };
        match Web::attach(
            controller,
            bar.id,
            bar.scale(),
            bar.size(),
            &folders,
            self.devtools,
            &self.events,
        ) {
            Ok(web) => {
                if bar.hidden {
                    web.set_hidden(true);
                }
                bar.web = Some(web);
            }
            Err(e) => log::error!("setting up a WebView: {e:#}"),
        }
    }

    // --- Hold ---------------------------------------------------------------------------------

    fn refresh_fullscreen(&mut self) {
        let hold = self.holds.wants_fullscreen();
        // The Windows key hook needs it whatever the hold says (and reads it from an atomic).
        let rect = (hold || self.winkey.is_some())
            .then(fullscreen_app_monitor)
            .flatten();
        WinKey::set_fullscreen(rect.is_some());
        self.fullscreen = rect.filter(|_| hold).and_then(|rect| {
            self.monitors
                .iter()
                .find(|m| m.rect == rect)
                .map(|m| m.device.clone())
        });
    }

    /// Display off or locked: every bar suspended. A fullscreen app: that monitor's bars hidden,
    /// the others idle (only their periodic work stops). Pause and battery don't touch the bars:
    /// they're the user's taskbar.
    fn sync_hold(&mut self) {
        let reasons = self.holds.reasons();
        let asleep = reasons.contains(Reason::DisplayOff) || reasons.contains(Reason::Locked);
        if asleep != self.asleep {
            // Locked or unlocked: a Windows key press the hook saw may never come up.
            WinKey::reset();
            log::info!(
                "{}",
                if asleep {
                    "asleep: bars suspended"
                } else {
                    "awake"
                }
            );
            self.asleep = asleep;
        }
        self.apply_visibility();
        self.sync_topics();
    }

    fn apply_visibility(&mut self) {
        let mut changed = false;
        for index in 0..self.bars.len() {
            let bar = &self.bars[index];
            let hidden =
                self.asleep || self.fullscreen.as_deref() == Some(bar.monitor.device.as_str());
            if hidden == bar.hidden {
                continue;
            }
            if hidden {
                log::info!("hiding the {:?} bar on {}", bar.kind, bar.monitor.device);
                self.post_event(index, "visibility", &json!(false));
                self.bars[index].previews.close();
                self.bars[index].set_hidden(true);
            } else {
                self.bars[index].set_hidden(false);
                self.post_event(index, "visibility", &json!(true));
                self.push_all(index);
            }
            changed = true;
        }
        if changed {
            self.sync_glass();
        }
        let looks = [BarKind::Top, BarKind::Dock].map(|kind| self.look(kind));
        self.first_paint.sync(&self.bars, |kind| {
            looks[usize::from(kind == BarKind::Dock)].clone()
        });
        self.sync_winkey();
    }

    // --- Topics --------------------------------------------------------------------------------

    fn shown_topics(&self) -> BTreeSet<Topic> {
        if self.asleep {
            return BTreeSet::new();
        }
        let idle = self.fullscreen.is_some();
        self.bars
            .iter()
            .filter(|b| !b.hidden)
            .flat_map(|b| bar_topics(b.kind, &b.config))
            .chain(self.flyouts.watches.core())
            .filter(|t| !(idle && t.periodic()))
            .collect()
    }

    /// Starts the sources of topics that came on screen and stops the rest.
    fn sync_topics(&mut self) {
        let wanted = self.shown_topics();
        if wanted == self.active {
            // Which custom modules show can change without the topic set changing.
            self.sync_extras();
            return;
        }
        let on = |topic| wanted.contains(&topic);
        self.media.set_active(on(Topic::Media));
        match (on(Topic::Volume), self.audio.is_some()) {
            (true, false) => self.audio = Audio::new(self.waker.clone()),
            (false, true) => self.audio = None,
            _ => {}
        }
        match (on(Topic::Network), self.network.is_some()) {
            (true, false) => {
                let watch = NetworkWatch::start(self.waker.clone());
                self.network_state = Some(network::read(watch.wlan()));
                self.network = Some(watch);
            }
            (false, true) => self.network = None,
            _ => {}
        }
        match (on(Topic::Workspaces), self.desktops.is_some()) {
            (true, false) => {
                self.desktops = Desktops::watch();
                if let Some(desktops) = &self.desktops {
                    self.workspaces = desktops.read();
                }
            }
            (false, true) => self.desktops = None,
            _ => {}
        }
        match (on(Topic::Stats), self.sampler.is_some()) {
            (true, false) => {
                self.sampler = Some(Sampler::new());
                self.timers.stats = Some(Instant::now() + Duration::from_millis(500));
            }
            (false, true) => {
                self.sampler = None;
                self.timers.stats = None;
            }
            _ => {}
        }
        match (on(Topic::Language), self.language_watch.is_some()) {
            (true, false) => self.language_watch = LanguageWatch::start(self.waker.clone()),
            (false, true) => self.language_watch = None,
            _ => {}
        }
        self.flyouts.set_counting(on(Topic::Notifications));
        let started: Vec<Topic> = wanted.difference(&self.active).copied().collect();
        self.active = wanted;
        self.sync_tray();
        self.sync_clock();
        self.sync_extras();
        for topic in started {
            self.emit(topic);
        }
    }

    fn sync_clock(&mut self) {
        if !self.active.contains(&Topic::Clock) {
            self.timers.clock = None;
        } else if self.timers.clock.is_none() {
            let seconds = self.bars.iter().any(|b| b.config.top.clock.seconds);
            let wait = next_tick(epoch_ms(), seconds);
            self.timers.clock = Some(Instant::now() + wait);
        }
    }

    /// The current value of a topic, as bar `index` sees it (only the dock's apps differ by bar).
    fn value(&mut self, topic: Topic, index: usize) -> Option<Value> {
        Some(match topic {
            Topic::Apps => json!(self.dock_items(index)),
            Topic::Foreground => self.foreground_info(),
            Topic::Clock => json!({ "now": epoch_ms() }),
            Topic::Media => json!(self.media_state),
            Topic::Volume => json!(self.audio.as_ref().and_then(Audio::read)),
            Topic::Network => json!(
                self.network_state
                    .get_or_insert_with(|| network::read(None))
            ),
            Topic::Battery => json!(system::battery()),
            Topic::Tray => json!(self.tray_items),
            Topic::Workspaces => json!(self.workspaces),
            Topic::Stats => json!(self.stats?),
            Topic::Language => {
                let layouts = self.layouts.get_or_insert_with(lang::layouts).clone();
                json!(lang::Languages {
                    current: lang::current(self.foreground),
                    layouts,
                })
            }
            Topic::Notifications => json!({ "count": self.notification_count? }),
            Topic::Custom => self.custom_value(),
            Topic::Trash => self.trash_value(),
        })
    }

    /// Sends a topic's value to every ready, visible bar that shows it, unless unchanged.
    fn emit(&mut self, topic: Topic) {
        if !self.active.contains(&topic) {
            return;
        }
        let mut shared: Option<String> = None;
        for index in 0..self.bars.len() {
            let bar = &self.bars[index];
            if !bar.ready || bar.hidden || !self.shows(bar, topic) {
                continue;
            }
            let message = match &shared {
                Some(message) if topic != Topic::Apps => message.clone(),
                _ => {
                    let Some(value) = self.value(topic, index) else {
                        return;
                    };
                    let message = json!({ "event": topic.event(), "data": value }).to_string();
                    self.first_paint.sent(topic, &message);
                    shared = Some(message.clone());
                    message
                }
            };
            let bar = &self.bars[index];
            if self.sent.get(&(bar.id, topic)) != Some(&message) {
                bar.post(&message);
                self.sent.insert((bar.id, topic), message);
            }
        }
    }

    /// A bar just said hello or came back: everything it shows, now. A late topic whose
    /// provider hasn't reported yet gets its last value from the previous run.
    fn push_all(&mut self, index: usize) {
        let id = self.bars[index].id;
        self.sent.retain(|(bar, _), _| *bar != id);
        let topics: Vec<Topic> = self
            .active
            .iter()
            .copied()
            .filter(|&topic| self.shows(&self.bars[index], topic))
            .collect();
        for topic in topics {
            match self.first_paint.stand_in(topic).map(str::to_owned) {
                Some(cached) => {
                    self.bars[index].post(&cached);
                    self.sent.insert((id, topic), cached);
                }
                None => self.emit(topic),
            }
        }
    }

    fn post_event(&self, index: usize, event: &str, data: &Value) {
        if self.bars[index].ready {
            self.bars[index].post(&json!({ "event": event, "data": data }).to_string());
        }
    }

    fn broadcast_event(&self, event: &str, data: impl Fn(&Bar) -> Value) {
        for bar in self.bars.iter().filter(|b| b.ready) {
            bar.post(&json!({ "event": event, "data": data(bar) }).to_string());
        }
    }

    // --- Apps -----------------------------------------------------------------------------------

    fn on_foreground(&mut self) {
        // SAFETY: read-only queries.
        let (hwnd, pid) = unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            (hwnd, pid)
        };
        if hwnd.is_invalid() || pid == std::process::id() {
            return; // a click on a bar: the app before it stays "in front"
        }
        let root = apps::root_owner(hwnd.0 as isize);
        if self.foreground != Some(root) {
            self.foreground = Some(root);
            soon(&mut self.timers.windows, SETTLE_WINDOWS);
            self.attended(root);
            self.emit(Topic::Apps);
            self.emit(Topic::Foreground);
            self.emit(Topic::Language); // a layout can be per window
        }
    }

    fn running(&mut self) -> Vec<Running> {
        let groups = self.apps.groups.clone();
        groups
            .into_iter()
            .map(|group| {
                let source = Source::App {
                    aumid: group.aumid.clone(),
                    exe: group.exe.clone(),
                };
                let described = self.icons.get(&source).cloned().unwrap_or_default();
                let fallback_name = group
                    .exe
                    .as_deref()
                    .map(apps::file_stem)
                    .or_else(|| group.windows.first().map(|w| w.title.clone()))
                    .unwrap_or_default();
                Running {
                    name: described.name.unwrap_or(fallback_name),
                    path: described
                        .path
                        .or_else(|| group.exe.clone())
                        .unwrap_or_default(),
                    icon: described.icon,
                    group,
                }
            })
            .collect()
    }

    fn pins(&mut self) -> Vec<Pin> {
        let pinned = self.config.dock.pinned.clone().unwrap_or_default();
        pinned
            .into_iter()
            .map(|p| {
                let described = self
                    .icons
                    .get(&Source::Path(p.path.clone()))
                    .cloned()
                    .unwrap_or_default();
                let (aumid, exe) = direct_identity(&p.path);
                Pin {
                    name: p
                        .name
                        .or(described.name)
                        .unwrap_or_else(|| apps::file_stem(&p.path)),
                    icon: described.icon,
                    aumid: described.aumid.or(aumid),
                    exe: described.exe.or(exe),
                    path: p.path,
                }
            })
            .collect()
    }

    /// What dock `index` shows (its config decides which running apps).
    fn dock_items(&mut self, index: usize) -> Vec<apps::Item> {
        let pins = self.pins();
        let running = self.running();
        let bar = &self.bars[index];
        let view = apps::View {
            running: bar.config.dock.running,
            monitor: apps::window_monitor(bar.hwnd()),
            active: self.foreground,
            attention: Some(&self.extras.attention),
        };
        apps::dock_items(&pins, &running, &self.apps.order, view)
    }

    fn foreground_info(&mut self) -> Value {
        let Some(group) = self
            .foreground
            .and_then(|hwnd| self.apps.group_of(hwnd))
            .cloned()
        else {
            return Value::Null;
        };
        let source = Source::App {
            aumid: group.aumid.clone(),
            exe: group.exe.clone(),
        };
        let described = self.icons.get(&source).cloned().unwrap_or_default();
        let name = described
            .name
            .or_else(|| group.exe.as_deref().map(apps::file_stem))
            .unwrap_or_default();
        json!({ "name": name, "icon": described.icon })
    }

    /// A dock click: launch it if it isn't running, minimize it if it's in front, else bring its
    /// most recent window to the front (restoring it).
    fn activate_app(&mut self, index: usize, key: &str) -> Result<Value, BarError> {
        let items = self.dock_items(index);
        let item = items
            .iter()
            .find(|i| i.key == key)
            .ok_or_else(|| BarError::not_found(format!("no dock item {key:?}")))?;
        if item.windows.is_empty() {
            system::open(item.path.clone());
            return Ok(Value::Null);
        }
        // Asked now, not from the window list, which trails the hooks by a few milliseconds.
        // SAFETY: a read-only query; a stale handle reads as not minimized.
        let minimized = |hwnd: isize| unsafe { IsIconic(system::hwnd(hwnd)) }.as_bool();
        let front = item
            .windows
            .iter()
            .map(|w| w.hwnd)
            .find(|&hwnd| Some(hwnd) == self.foreground && !minimized(hwnd));
        log::debug!(
            "activate {key}: in front {:?}, the app's windows {:?}",
            self.foreground,
            item.windows.iter().map(|w| w.hwnd).collect::<Vec<_>>()
        );
        match front {
            Some(window) => {
                system::minimize(system::hwnd(window));
                // Like the taskbar: the next window comes to the front, not the bar.
                self.hand_back(Some(window));
            }
            None => {
                let window = item.windows[0].hwnd;
                if system::activate(system::hwnd(window)) {
                    self.moved_foreground(Some(window));
                }
            }
        }
        Ok(Value::Null)
    }

    /// We moved the foreground ourselves. The hooks skip our own process's events, so this is
    /// how the dock learns which app is in front now.
    fn moved_foreground(&mut self, to: Option<isize>) {
        if self.foreground != to {
            self.foreground = to;
            self.emit(Topic::Apps);
            self.emit(Topic::Foreground);
        }
    }

    /// Gives the foreground back to the app window that had it (or the top one, or the desktop).
    /// The bars only hold it right after a click; WebView2 taking focus as it starts, or
    /// Windows activating our topmost window when the active app minimized or closed, must not
    /// leave it with us. `except` is a window that is going away.
    fn hand_back(&mut self, except: Option<isize>) {
        let usable = |hwnd: isize| {
            let window = system::hwnd(hwnd);
            // SAFETY: read-only queries; a stale handle just isn't usable.
            unsafe {
                IsWindow(Some(window)).as_bool()
                    && IsWindowVisible(window).as_bool()
                    && !IsIconic(window).as_bool()
            }
        };
        let target = self
            .foreground
            .filter(|&hwnd| Some(hwnd) != except && usable(hwnd))
            .or_else(|| apps::front_window(except));
        // SAFETY: plain calls; we hold the foreground, so Windows lets us pass it on.
        let moved = unsafe {
            let window = target.map_or_else(|| GetShellWindow(), system::hwnd);
            SetForegroundWindow(window).as_bool()
        };
        if moved {
            self.moved_foreground(target);
        }
    }

    /// Edits the pinned list and saves it to `bars.json`.
    fn edit_pins(&mut self, change: impl FnOnce(&mut Vec<Pinned>)) -> Result<Value, BarError> {
        let mut pins = self.config.dock.pinned.clone().unwrap_or_default();
        change(&mut pins);
        let value = serde_json::to_value(&pins).map_err(|e| BarError::internal(e.to_string()))?;
        config::edit(&self.config_path, |root| {
            config::set(root, "dock", "pinned", value)
        })
        .map_err(|e| BarError::internal(format!("{e:#}")))?;
        self.config.dock.pinned = Some(pins);
        self.emit(Topic::Apps);
        Ok(Value::Null)
    }

    // --- The page ---------------------------------------------------------------------------------

    fn on_page_message(&mut self, index: usize, source: &str, json: &str) {
        if !source.starts_with(&format!("https://{PAGE_HOST}/")) {
            return log::warn!("ignored a message from {source}");
        }
        let envelope: Envelope = match serde_json::from_str(json) {
            Ok(envelope) => envelope,
            Err(e) => return log::warn!("bad message from a bar ({e}): {json}"),
        };
        let hello = envelope.method == "hello";
        log::debug!(
            "bar {}: {} {}",
            self.bars[index].id,
            envelope.method,
            envelope.params
        );
        let reply = match Method::parse(&envelope.method, envelope.params) {
            Ok(Method::Flyout(request)) => self.flyout(index, envelope.id, request),
            Ok(method) => Some(self.call(index, method)),
            Err(error) => Some(Err(error)),
        };
        // A flyout provider answers later (`Done::Reply`).
        let Some(reply) = reply else { return };
        let message = match reply {
            Ok(result) => json!({ "id": envelope.id, "result": result }),
            Err(error) => {
                log::info!("{}: {}", envelope.method, error.message);
                json!({ "id": envelope.id, "error": error })
            }
        };
        if let Some(bar) = self.bars.get(index) {
            bar.post(&message.to_string());
        }
        if hello && index < self.bars.len() {
            // A new page: its flyouts are gone.
            let id = self.bars[index].id;
            self.flyouts.watches.forget(id);
            self.flyouts.page_loaded(id);
            self.bars[index].set_room(0.0);
            if self.keyboard == Some(id) {
                self.keyboard = None;
            }
            self.bars[index].ready = true;
            self.sync_winkey();
            self.looks.forget(self.bars[index].id);
            self.first_paint.hello(self.bars[index].id);
            self.push_all(index);
            self.push_looks();
            if let Some(web) = &self.bars[index].web {
                web.probe_paint();
            }
        }
    }

    fn look(&self, kind: BarKind) -> look::Look {
        look::resolve(
            kind,
            self.skin.appearance.as_ref(),
            &self.config.appearance,
            self.palette,
        )
    }

    fn call(&mut self, index: usize, method: Method) -> Result<Value, BarError> {
        let done = Ok(Value::Null);
        match method {
            Method::Hello => {
                let bar = &self.bars[index];
                let scale = bar.scale();
                Ok(json!({
                    "bar": bar.kind,
                    "monitor": {
                        "device": bar.monitor.device,
                        "primary": bar.monitor.primary,
                        "width": f64::from(bar.monitor.rect.width) / scale,
                        "height": f64::from(bar.monitor.rect.height) / scale,
                    },
                    "config": bar.config,
                    "look": self.look_payload(bar),
                    "mode": self.mode_payload(bar),
                    "backdrop": self.backdrop_payload(bar),
                    "dpi": bar.monitor.dpi,
                }))
            }
            Method::Activate(Activate::App { app }) => self.activate_app(index, &app),
            Method::Activate(Activate::Window { hwnd }) => {
                if system::activate(system::hwnd(hwnd)) {
                    self.moved_foreground(Some(hwnd));
                }
                done
            }
            Method::Launch { path, .. } => {
                system::open(path);
                done
            }
            Method::Minimize { hwnd } => {
                system::minimize(system::hwnd(hwnd));
                done
            }
            Method::Close { hwnd } => {
                system::close(system::hwnd(hwnd));
                done
            }
            Method::Pin { path, name, index } => self.edit_pins(|pins| {
                let pinned = Pinned { path, name };
                let at = pins
                    .iter()
                    .position(|p| p.path.eq_ignore_ascii_case(&pinned.path));
                let pinned = match at {
                    Some(at) => pins.remove(at),
                    None => pinned,
                };
                let index = index.unwrap_or(pins.len()).min(pins.len());
                pins.insert(index, pinned);
            }),
            Method::Unpin { path } => {
                self.edit_pins(|pins| pins.retain(|p| !p.path.eq_ignore_ascii_case(&path)))
            }
            Method::Reorder { paths } => self.edit_pins(|pins| reorder(pins, &paths)),
            Method::Start => {
                system::start_menu();
                done
            }
            Method::ActionCenter { panel } => {
                system::action_center(panel);
                done
            }
            Method::Volume { set, mute, delta } => {
                let audio = self
                    .audio
                    .as_ref()
                    .ok_or_else(|| BarError::unavailable("no volume module on screen"))?;
                let volume = audio
                    .change(set, mute, delta)
                    .ok_or_else(|| BarError::unavailable("no output device"))?;
                Ok(json!(volume))
            }
            Method::Media { action } => {
                self.media.control(action);
                done
            }
            Method::Tray { id, button, rect } => {
                self.click_tray(index, id, button, rect);
                done
            }
            Method::Power { action } => {
                system::power(action);
                done
            }
            Method::Workspace { direction } => {
                system::switch_desktop(direction);
                done
            }
            Method::OpenSettings { uri } => {
                system::open(uri);
                done
            }
            Method::Region { rects } => {
                self.first_paint.region(&self.bars[index], &rects);
                self.bars[index].set_region(rects);
                done
            }
            // Routed to `flyout` before `call`.
            Method::Flyout(_) => Err(BarError::internal("a flyout request reached call()")),
            Method::Run { module, trigger } => self.run_action(index, &module, trigger),
            Method::Search => {
                system::search();
                done
            }
            Method::ShowDesktop => {
                system::show_desktop();
                done
            }
            Method::Trash { action } => {
                match action {
                    TrashAction::Open => system::open(r"shell:RecycleBinFolder".into()),
                    TrashAction::Empty => trash::empty(),
                }
                done
            }
            Method::PreviewOpen { hwnds } => Ok(self.preview_open(index, &hwnds)),
            Method::PreviewPlace { windows } => {
                self.preview_place(index, &windows);
                done
            }
            Method::PreviewClose => {
                self.bars[index].previews.close();
                done
            }
        }
    }

    /// An auto-hiding dock's edge was touched.
    fn reveal(&self, edge: isize) {
        if let Some(index) = self
            .bars
            .iter()
            .position(|b| b.edge_hwnd().is_some_and(|e| e.0 as isize == edge))
            && !self.bars[index].hidden
        {
            self.post_event(index, "reveal", &Value::Null);
        }
    }

    // --- The native taskbar -----------------------------------------------------------------------

    fn wanted_taskbar(&self) -> Wanted {
        // Without the page there's nothing to replace the taskbar with: keep it.
        let mode = if self.page.is_some() {
            self.config.taskbar.mode
        } else {
            TaskbarMode::Keep
        };
        Wanted {
            mode,
            alignment: self.config.taskbar.alignment,
        }
    }

    fn apply_taskbar(&mut self) {
        let wanted = self.wanted_taskbar();
        hooks::watch_taskbar(wanted.mode == TaskbarMode::Hide);
        if let Err(e) = self.taskbar.apply(wanted) {
            log::warn!("{e:#}");
        }
    }

    /// Explorer showed (or slid in) the taskbar while it should be hidden: hide it at once, and
    /// set auto-hide again if Explorer reset it. Never gives up; [`REHIDE_SPACING`] only keeps a
    /// fight with Explorer cheap.
    fn rehide_taskbar(&mut self, now: Instant) {
        if self.wanted_taskbar().mode != TaskbarMode::Hide {
            return;
        }
        let next = self.last_rehide + REHIDE_SPACING;
        if now < next {
            self.timers.rehide.get_or_insert(next);
            return;
        }
        self.last_rehide = now;
        let (shown, autohide) = mypaper_bars::taskbar::hide_visible();
        if shown > 0 {
            log::debug!("Explorer showed the taskbar; hid it again");
        }
        if !autohide {
            log::info!("Explorer reset the taskbar's auto-hide; setting it again");
            self.apply_taskbar();
        }
    }

    // --- Live reload ------------------------------------------------------------------------------

    fn reload_config(&mut self) {
        match Config::load() {
            Ok(settings) => {
                self.holds.apply(&settings);
                self.refresh_fullscreen();
                self.sync_hold();
            }
            Err(e) => log::error!("{e:#}\n(keeping the current settings)"),
        }
        let skin = ThemeSkin::load();
        if skin != self.skin {
            self.skin = skin;
            self.watch_theme();
            self.refresh_look();
        }
        let mut config = match BarsConfig::load(&self.config_path) {
            Ok(config) => config,
            Err(e) => return log::error!("{e:#}\n(keeping the last good bars.json)"),
        };
        config.resolve(taskbar_pins);
        if config == self.config {
            return;
        }
        log::info!("bars.json changed");
        let taskbar_changed = config.taskbar != self.config.taskbar;
        let clock_changed = config.top.clock != self.config.top.clock;
        self.config = config;
        if taskbar_changed {
            self.apply_taskbar();
        }
        self.reconcile();
        self.sync_hooks();
        if clock_changed {
            self.timers.clock = None;
            self.sync_clock();
        }
        self.sync_tray();
        self.flyouts.switch_output = self.config.audio.switch_output;
        self.sync_winkey();
        self.broadcast_event("config", |bar| json!(bar.config));
        self.refresh_look();
        self.sent.clear();
        for index in 0..self.bars.len() {
            if self.bars[index].ready && !self.bars[index].hidden {
                self.push_all(index);
            }
        }
    }

    fn reload_theme(&mut self) {
        let skin = ThemeSkin::load();
        let palette = look::palette_accent();
        if skin == self.skin && palette == self.palette {
            return;
        }
        if skin.dir != self.skin.dir {
            self.skin = skin;
            self.watch_theme();
        } else {
            self.skin = skin;
        }
        self.palette = palette;
        self.refresh_look();
    }

    fn reload_page(&mut self) {
        let first = self.page.is_none();
        let page = page_dir();
        let built = page.as_deref().and_then(built_at);
        if !first && built == self.page_built {
            return; // the WebView reading the files, not a rebuild
        }
        self.page = page;
        self.page_built = built;
        if first && self.page.is_some() {
            log::info!("the page was built; starting the bars");
            self.apply_taskbar();
            self.reconcile();
            return;
        }
        log::info!("the page was rebuilt; reloading the bars");
        for bar in &mut self.bars {
            bar.ready = false;
            if let Some(web) = &bar.web {
                web.reload();
            }
        }
    }

    /// File watches: `%APPDATA%\mypaper` (bars.json, config.json, user themes), the active
    /// theme's folder, the palette, and the page's build folder.
    fn watch(&mut self) {
        if let Ok(appdata) = mypaper_config::dir() {
            let root = appdata.clone();
            let themes = appdata.join("themes");
            let classify = move |path: &Path| -> Option<Msg> {
                if path.parent() == Some(root.as_path()) {
                    return match path.file_name()?.to_str()? {
                        "bars.json" | "config.json" => Some(Msg::Config),
                        _ => None,
                    };
                }
                path.starts_with(&themes).then_some(Msg::Theme)
            };
            let _ = std::fs::create_dir_all(&appdata);
            self.watch_dir(&appdata, classify);
        }
        if let Some(page) = self.page.clone().or_else(|| {
            // Not built yet: watch for the build to appear beside the exe or cwd.
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join("bars").join("shell"))
                .filter(|dir| dir.is_dir())
        }) {
            self.watch_dir(&page, |_| Some(Msg::Page));
        }
        if let Ok(palette) = mypaper_bars::files::local("palette.json") {
            let waker = self.waker.clone();
            self._palette_watch =
                mypaper_desktop::watch_file(&palette, move || waker.send(Msg::Theme))
                    .inspect_err(|e| log::warn!("not watching the palette: {e:#}"))
                    .ok();
        }
        self.watch_theme();
    }

    /// Watches the active theme's folder when it's outside `%APPDATA%\mypaper` (bundled).
    fn watch_theme(&mut self) {
        let Some(dir) = self.skin.dir.clone() else {
            self.theme_watch = None;
            return;
        };
        if self.theme_watch.as_ref().is_some_and(|(d, _)| *d == dir)
            || mypaper_config::dir().is_ok_and(|appdata| dir.starts_with(appdata))
        {
            return;
        }
        let waker = self.waker.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|e| !e.kind.is_access()) {
                waker.send(Msg::Theme);
            }
        });
        self.theme_watch = watcher
            .and_then(|mut w| w.watch(&dir, notify::RecursiveMode::Recursive).map(|()| w))
            .inspect_err(|e| log::warn!("not watching {}: {e}", dir.display()))
            .ok()
            .map(|w| (dir, w));
    }

    fn watch_dir(&mut self, dir: &Path, classify: impl Fn(&Path) -> Option<Msg> + Send + 'static) {
        let (path, waker) = (dir.to_owned(), self.waker.clone());
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if !event.kind.is_access() => {
                    if let Some(msg) = event.paths.iter().find_map(|p| classify(p)) {
                        waker.send(msg);
                    }
                }
                Ok(_) => {}
                Err(e) => waker.send(Msg::WatchError(path.clone(), e.to_string())),
            });
        match watcher.and_then(|mut w| w.watch(dir, notify::RecursiveMode::Recursive).map(|()| w)) {
            Ok(watcher) => self.watchers.push(watcher),
            Err(e) => log::warn!("not watching {}: {e}", dir.display()),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let started = Instant::now();
        // First: a lone press must not be swallowed for a launcher that's going away.
        self.winkey = None;
        self.first_paint.save();
        self.media.set_active(false);
        self.tray_host.set_active(false); // apps re-register with Explorer
        self.tray.set_active(false);
        self.bars.clear(); // app bars removed: the work area comes back
        self.env = Env::None;
        match self.taskbar.restore() {
            Ok(true) => log::info!("native taskbar restored"),
            Ok(false) => {}
            Err(e) => log::error!("restoring the taskbar: {e:#} (core replays the journal)"),
        }
        log::info!("stopped in {:?}", started.elapsed());
    }
}

/// Puts `pins` in the order of `paths` (case-insensitive); pins not listed keep their order
/// after the listed ones.
fn reorder(pins: &mut [Pinned], paths: &[String]) {
    let rank = |pin: &Pinned| {
        paths
            .iter()
            .position(|p| p.eq_ignore_ascii_case(&pin.path))
            .unwrap_or(usize::MAX)
    };
    pins.sort_by_key(rank); // stable: unlisted pins keep their order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(path: &str) -> Pinned {
        Pinned {
            path: path.into(),
            name: None,
        }
    }

    #[test]
    fn reorder_follows_the_page_and_keeps_the_rest() {
        let mut pins = vec![pin("a"), pin("b"), pin("c"), pin("d")];
        reorder(&mut pins, &["C".into(), "a".into()]);
        let order: Vec<&str> = pins.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(order, ["c", "a", "b", "d"]);
    }

    #[test]
    fn the_clock_ticks_on_the_minute() {
        // 12:00:59.500 → just after 12:01:00.
        let now = 12 * 3_600_000 + 59_500;
        assert_eq!(next_tick(now, false), Duration::from_millis(505));
        assert_eq!(next_tick(now, true), Duration::from_millis(505));
        assert_eq!(next_tick(60_000, false), Duration::from_millis(60_005));
    }

    #[test]
    fn pin_paths_identify_their_apps() {
        assert_eq!(
            direct_identity(r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
            (
                Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
                None
            )
        );
        assert_eq!(
            direct_identity(r"C:\Windows\Explorer.EXE"),
            (None, Some(r"c:\windows\explorer.exe".into()))
        );
        assert_eq!(direct_identity(r"C:\Users\me\Projects"), (None, None));
    }
}
