//! The widget host's state and main loop: surfaces per monitor, the layout,
//! edit mode, the hold, and every widget call.
//!
//! One thread. It sleeps in `MsgWaitForMultipleObjectsEx` until a window
//! message, a [`Msg`] from a worker, or the earliest timer (debounces, the
//! stats tick, plugin timeouts). With nothing scheduled it waits forever, so
//! an idle or held host costs nothing.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use mypaper_config::Config;
use mypaper_desktop::hold::{Fact, HoldReasons, Holds, Reason};
use mypaper_desktop::{
    ControlWindow, DesktopHost, Monitor, Rect, WM_FACT, WindowEvents, covered_monitors,
    monitor_infos,
};
use mypaper_theme::{Color, Surface as SurfaceKind, WidgetStyle};
use notify::Watcher;
use serde_json::{Map, Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE, ICoreWebView2Environment,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, RegisterWindowMessageW, SW_SHOWNORMAL, TranslateMessage, WM_HOTKEY,
    WM_QUIT,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::appicon::Icons;
use crate::audio::{Audio, Wasapi};
use crate::bridge::{ErrorCode, FromShell, Method, Out, Reply, Target, Topic, WidgetError};
use crate::display::{DisplayWatcher, WM_DISPLAY};
use crate::layout::{self, Grid, Layout, Placed, RectF};
use crate::look;
use crate::manifest::{Preset, WidgetManifest};
use crate::media::Media;
use crate::net::WidgetNet;
use crate::overlay::{self, Layer};
use crate::pick;
use crate::plugins::Plugins;
use crate::registry::{self, Registry};
use crate::stats::{Delivery, Stats};
use crate::storage::{self, Storage};
use crate::surface::{self, Events, NetPolicy, Setup, Surface, WebEvent};
use crate::theme::{self, ThemeInfo};
use crate::userinfo;
use crate::wake::{Msg, WM_WAKE, Waker};

/// The class of the host's message-only window (how `--edit` finds it).
pub const HOST_CLASS: PCWSTR = w!("mypaper.widgets.host");
/// Posted to the host window: `wParam` 0 = off, 1 = on, 2 = toggle.
pub const EDIT_MESSAGE: PCWSTR = w!("mypaper.widgets.edit");

const EDIT_HOTKEY: i32 = 1;
/// Ctrl+Alt+Shift+W.
const EDIT_KEY: u32 = b'W' as u32;
const OVERLAY_HOTKEY: i32 = 2;

/// One window of one monitor: the desktop layer's or the overlay's.
type LayerKey = (String, Layer);

const RECHECK: Duration = Duration::from_millis(100);
const SETTLE_CONFIG: Duration = Duration::from_millis(100);
const SETTLE_LAYOUT: Duration = Duration::from_millis(150);
const SETTLE_WIDGETS: Duration = Duration::from_millis(250);
const SETTLE_SHELL: Duration = Duration::from_millis(500);
/// App bars announce a work-area change in bursts (one per bar, per edge).
const SETTLE_DISPLAY: Duration = Duration::from_millis(200);
const RETRY_WEBVIEW: Duration = Duration::from_secs(30);
/// After a monitor's layer has been hidden this long (a game or maximized window
/// over it, a locked PC), that layer and its WebView are released entirely: a
/// suspended WebView still holds hundreds of MB. It is rebuilt (about a second)
/// when the monitor shows again. Layers on other monitors stay live.
const PARK_AFTER: Duration = Duration::from_secs(30);

/// Nothing is on screen: layers hide and sleep. Any other hold (paused, on battery) leaves the
/// desktop visible, so layers stay up showing their last frame.
fn nothing_visible(reasons: HoldReasons) -> bool {
    reasons.contains(Reason::DisplayOff) || reasons.contains(Reason::Locked)
}

/// What one monitor's widget layer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerState {
    /// Shown and running.
    Live,
    /// Shown, but widgets are told to stop (a global hold with the desktop on screen).
    Frozen,
    /// Hidden and suspended (and released later): nothing to see.
    Hidden,
}

/// The rule for a monitor's layer. Only a window covering *that* monitor (a maximized or
/// fullscreen app) hides it, so a game on another monitor changes nothing here. Edit mode
/// shows a covered monitor regardless.
fn layer_state(reasons: HoldReasons, covered: bool, editing: bool) -> LayerState {
    if nothing_visible(reasons) || (covered && !editing) {
        LayerState::Hidden
    } else if !reasons.is_empty() {
        LayerState::Frozen
    } else {
        LayerState::Live
    }
}

/// The built shell: its folder and the hardening script injected into every frame.
struct Shell {
    dir: PathBuf,
    harden: String,
}

fn load_shell() -> Option<Shell> {
    let Some(dir) = crate::files::shell_dist() else {
        log::error!("widgets/shell/dist not found; build it with `pnpm -C widgets/shell build`");
        return None;
    };
    match std::fs::read_to_string(dir.join("harden.js")) {
        Ok(harden) => Some(Shell { dir, harden }),
        Err(e) => {
            log::error!("{}: {e}", dir.join("harden.js").display());
            None
        }
    }
}

/// Moves instances of replaced widgets onto their replacements (`Layout::migrate`) and saves
/// when that changed anything, so the file, and Studio which reads it, names installed widgets.
/// Never call it while the file doesn't parse: it would overwrite the user's file.
fn migrate_layout(registry: &Registry, path: &Path, layout: &mut Layout) {
    if layout.migrate(|old| registry.replacement(old))
        && let Err(e) = layout.save(path)
    {
        log::warn!("saving the migrated widgets.json: {e:#}");
    }
}

enum Env {
    None,
    Creating(u64),
    Ready(ICoreWebView2Environment),
}

/// Debounce deadlines.
#[derive(Default)]
struct Timers {
    recheck: Option<Instant>,
    config: Option<Instant>,
    layout: Option<Instant>,
    widgets: Option<Instant>,
    shell: Option<Instant>,
    webview: Option<Instant>,
    park: Option<Instant>,
    display: Option<Instant>,
}

impl Timers {
    fn earliest(&self) -> Option<Instant> {
        [
            self.recheck,
            self.config,
            self.layout,
            self.widgets,
            self.shell,
            self.webview,
            self.park,
            self.display,
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

pub struct App {
    host: DesktopHost,
    control: ControlWindow,
    edit_message: u32,
    overlay_message: u32,
    hotkey: bool,
    overlay_hotkey: bool,
    events: Events,
    rx: Receiver<Msg>,
    waker: Waker,
    env: Env,
    env_generation: u64,
    surfaces: Vec<Surface>,
    next_surface: u32,
    monitors: Vec<Monitor>,
    icon_cell: [f64; 2],
    layout_path: PathBuf,
    layout: Layout,
    /// Set while `widgets.json` doesn't parse: shown in edit mode, blocks saving.
    layout_error: Option<String>,
    roots: Vec<PathBuf>,
    registry: Registry,
    policy: NetPolicy,
    shell: Option<Shell>,
    holds: Holds,
    reasons: HoldReasons,
    covered: HashMap<String, bool>,
    editing: bool,
    /// Layers (by monitor device) that were hidden long enough to be released.
    parked: BTreeSet<LayerKey>,
    /// Since when each layer has been hidden.
    hidden_since: HashMap<LayerKey, Instant>,
    /// How long a layer stays hidden before it is parked ([`PARK_AFTER`], or
    /// `MYPAPER_WIDGETS_PARK_SECS` for tests).
    park_after: Duration,
    devtools: bool,
    /// `MYPAPER_WIDGETS_IGNORE_COVERED=1`: keep covered monitors awake (for
    /// capturing them in tests).
    ignore_covered: bool,
    storage: Storage,
    /// `%APPDATA%\mypaper\widget-data`: storage files, and the files widgets picked.
    data_dir: PathBuf,
    /// A file dialog is open (one at a time).
    picking: bool,
    stats: Stats,
    media: Media,
    audio: Audio<Wasapi>,
    icons: Icons,
    plugins: Plugins,
    theme: ThemeInfo,
    /// The active theme's folder, and the widget skin read from it.
    theme_dir: Option<PathBuf>,
    skin: look::Skin,
    /// `(instance, sub)` of every `theme` subscription.
    theme_subs: BTreeSet<(String, String)>,
    /// Window-change hooks: only while a widget layer exists (they wake us on
    /// every window move anywhere).
    window_events: Option<WindowEvents>,
    timers: Timers,
    changed_widgets: BTreeSet<String>,
    watchers: Vec<notify::RecommendedWatcher>,
    /// Watches the active theme's `theme.json` and style sheets; replaced when the theme changes.
    theme_watch: Option<notify::RecommendedWatcher>,
    palette_watch: Option<mypaper_desktop::ConfigWatcher>,
    /// Hears about work-area and display changes (see `display.rs`); held for its window.
    _display_watch: Option<DisplayWatcher>,
}

impl App {
    pub fn new() -> Result<Self> {
        // SAFETY: COM for this (UI) thread, which WebView2 requires to be an STA.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .context("CoInitializeEx")?;
        let host = DesktopHost::find()?;
        log::info!("desktop host: {host:?}");
        let control = ControlWindow::new(HOST_CLASS)?;
        // SAFETY: a static name.
        let (edit_message, overlay_message) = unsafe {
            (
                RegisterWindowMessageW(EDIT_MESSAGE),
                RegisterWindowMessageW(overlay::OVERLAY_MESSAGE),
            )
        };
        surface::register_class()?;
        // SAFETY: thread hotkey; WM_HOTKEY arrives in our queue.
        let hotkey = unsafe {
            RegisterHotKey(
                None,
                EDIT_HOTKEY,
                MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                EDIT_KEY,
            )
        }
        .inspect_err(|e| log::warn!("edit hotkey Ctrl+Alt+Shift+W unavailable: {e}"))
        .is_ok();
        // SAFETY: as above.
        let overlay_hotkey = unsafe {
            RegisterHotKey(
                None,
                OVERLAY_HOTKEY,
                MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                overlay::OVERLAY_KEY,
            )
        }
        .inspect_err(|e| log::warn!("overlay hotkey Ctrl+Alt+Shift+O unavailable: {e}"))
        .is_ok();

        let (tx, rx) = mpsc::channel();
        // SAFETY: no preconditions.
        let waker = Waker::new(tx, unsafe { GetCurrentThreadId() });
        let config = Config::load().unwrap_or_else(|e| {
            log::error!("{e:#}\n(using the default settings)");
            Config::default()
        });
        let mut holds = Holds::default();
        holds.apply(&config);
        holds.set(Fact::OnBattery, mypaper_desktop::on_battery());

        let layout_path = crate::files::layout()?;
        let layout_missing = !layout_path.exists();
        let (layout, layout_error) = match Layout::load(&layout_path) {
            Ok(layout) => (layout, None),
            Err(e) => {
                log::error!("{e:#}");
                (Layout::default(), Some(format!("{e:#}")))
            }
        };
        let roots = registry::widget_roots();
        let registry = Registry::scan(&roots);
        let monitors = monitor_infos();
        for monitor in &monitors {
            log::info!(
                "monitor {} {:?} at {} dpi{}",
                monitor.device,
                monitor.rect,
                monitor.dpi,
                if monitor.primary { " (primary)" } else { "" }
            );
        }
        log::info!(
            "{} widgets installed, {} instances",
            registry.widgets.len(),
            layout.instances.len()
        );
        let devtools = std::env::var_os("MYPAPER_WIDGETS_DEVTOOLS").is_some_and(|v| v == "1");
        let theme_dir = theme::dir();
        let skin = look::load_skin(theme_dir.as_deref());
        let display_watch = DisplayWatcher::new()
            .inspect_err(|e| log::warn!("work-area changes won't be noticed: {e:#}"))
            .ok();
        let data_dir = crate::files::user("widget-data")?;
        let mut app = Self {
            host,
            control,
            edit_message,
            overlay_message,
            hotkey,
            overlay_hotkey,
            events: Events::default(),
            rx,
            env: Env::None,
            env_generation: 0,
            surfaces: Vec::new(),
            next_surface: 0,
            monitors,
            icon_cell: icon_cell(),
            layout_path,
            layout,
            layout_error,
            roots,
            registry,
            policy: NetPolicy::default(),
            shell: load_shell(),
            holds,
            reasons: HoldReasons::default(),
            covered: HashMap::new(),
            editing: false,
            parked: BTreeSet::new(),
            hidden_since: HashMap::new(),
            devtools,
            ignore_covered: std::env::var_os("MYPAPER_WIDGETS_IGNORE_COVERED")
                .is_some_and(|v| v == "1"),
            park_after: std::env::var("MYPAPER_WIDGETS_PARK_SECS")
                .ok()
                .and_then(|secs| secs.parse().ok())
                .map_or(PARK_AFTER, Duration::from_secs),
            storage: Storage::new(data_dir.clone()),
            data_dir,
            picking: false,
            stats: Stats::default(),
            media: Media::new(waker.clone()),
            audio: Audio::new(Wasapi::new(waker.clone())),
            icons: Icons::new(waker.clone()),
            plugins: Plugins::new(waker.clone()),
            theme: theme::current(),
            theme_dir,
            skin,
            theme_subs: BTreeSet::new(),
            window_events: None,
            timers: Timers::default(),
            changed_widgets: BTreeSet::new(),
            watchers: Vec::new(),
            theme_watch: None,
            palette_watch: None,
            _display_watch: display_watch,
            waker,
        };
        if layout_missing {
            app.seed_starter_layout();
        }
        if app.layout_error.is_none() {
            migrate_layout(&app.registry, &app.layout_path, &mut app.layout);
        }
        app.watch();
        // Retire a previous host's engine plates: the shell now draws the visible surface.
        crate::plates::retire();
        app.update_policy();
        app.sync_hold();
        app.reconcile();
        Ok(app)
    }

    /// Runs until `WM_QUIT`; returns the exit code.
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

    fn wait(&self) {
        let deadline = [
            self.timers.earliest(),
            self.stats.deadline(),
            self.plugins.deadline(),
        ]
        .into_iter()
        .flatten()
        .min();
        let timeout = deadline.map_or(INFINITE, |at| {
            let left = at.saturating_duration_since(Instant::now());
            left.as_millis().min(u128::from(INFINITE - 1)) as u32
                + u32::from(left.subsec_micros() % 1000 != 0)
        });
        if timeout == 0 {
            return;
        }
        // SAFETY: waits on our own queue only.
        unsafe { MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
    }

    fn pump(&mut self) -> Option<u8> {
        let mut msg = MSG::default();
        // SAFETY: standard message pump on the thread that owns our windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                match msg.message {
                    WM_QUIT => return Some(msg.wParam.0 as u8),
                    WM_HOTKEY if msg.wParam.0 == EDIT_HOTKEY as usize => {
                        self.set_editing(!self.editing)
                    }
                    WM_HOTKEY if msg.wParam.0 == OVERLAY_HOTKEY as usize => {
                        self.set_overlay(!self.layout.overlay)
                    }
                    WM_FACT => {
                        if let Some(fact) = Fact::from_wparam(msg.wParam.0) {
                            self.holds.set(fact, msg.lParam.0 != 0);
                            self.sync_hold();
                        }
                    }
                    WM_WAKE => {}
                    WM_DISPLAY => {
                        self.timers
                            .display
                            .get_or_insert(Instant::now() + SETTLE_DISPLAY);
                    }
                    m if m == self.edit_message && msg.hwnd == self.control.hwnd() => {
                        match msg.wParam.0 {
                            0 => self.set_editing(false),
                            1 => self.set_editing(true),
                            2 => self.set_editing(!self.editing),
                            other => log::warn!("{EDIT_MESSAGE:?}: unknown wParam {other}"),
                        }
                    }
                    m if m == self.overlay_message && msg.hwnd == self.control.hwnd() => {
                        match overlay::requested(self.layout.overlay, msg.wParam.0) {
                            Some(on) => self.set_overlay(on),
                            None => log::warn!(
                                "{:?}: unknown wParam {}",
                                overlay::OVERLAY_MESSAGE,
                                msg.wParam.0
                            ),
                        }
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
        let now = Instant::now();
        match msg {
            Msg::Backdrop => self.push_all(),
            Msg::Config | Msg::Palette | Msg::Theme => {
                self.timers.config.get_or_insert(now + SETTLE_CONFIG);
            }
            Msg::Layout => {
                self.timers.layout.get_or_insert(now + SETTLE_LAYOUT);
            }
            Msg::Widgets(ids) => {
                self.changed_widgets.extend(ids);
                self.timers.widgets.get_or_insert(now + SETTLE_WIDGETS);
            }
            Msg::Shell => {
                self.timers.shell.get_or_insert(now + SETTLE_SHELL);
            }
            Msg::Plugin {
                id,
                generation,
                line,
            } => {
                let out = self.plugins.on_line(&id, generation, line, now);
                self.deliver(out);
            }
            Msg::Media { token, result } => {
                let out = self.media.on_result(token, result);
                self.deliver(out);
            }
            Msg::Answered(target, reply) => self.reply(target, reply),
            Msg::Audio { generation, frame } => {
                let out = self.audio.on_frame(generation, &frame);
                self.deliver(out);
            }
            Msg::Picked(done) => self.on_picked(done),
            Msg::WatchError(path, error) => {
                log::warn!("stopped watching {}: {error}", path.display())
            }
        }
    }

    fn tick(&mut self, now: Instant) {
        if self
            .window_events
            .as_ref()
            .is_some_and(WindowEvents::take_changed)
        {
            self.timers.recheck.get_or_insert(now + RECHECK);
        }
        if due(&mut self.timers.recheck, now) {
            self.recheck();
        }
        if due(&mut self.timers.display, now) {
            self.relayout_for_display();
        }
        if due(&mut self.timers.config, now) {
            self.reload_config();
        }
        if due(&mut self.timers.layout, now) {
            self.reload_layout();
        }
        if due(&mut self.timers.widgets, now) {
            self.rescan_widgets();
        }
        if due(&mut self.timers.shell, now) {
            self.reload_shell();
        }
        if due(&mut self.timers.webview, now) {
            self.reconcile();
        }
        if due(&mut self.timers.park, now) {
            self.park_hidden(now);
        }
        for delivery in self.stats.tick(now) {
            match delivery {
                Delivery::Reply(target, stats) => self.reply(target, Ok(json!(stats))),
                Delivery::Event {
                    instance,
                    sub,
                    stats,
                } => {
                    self.emit_sub(&instance, &sub, json!(stats));
                }
            }
        }
        let out = self.plugins.tick(now);
        self.deliver(out);
    }

    /// A global hold: the user paused, or we're on battery with that setting, or the display
    /// is off or the session locked. (A fullscreen app is not one: it only pauses the monitor
    /// it covers, see [`hides`](Self::hides).)
    fn held(&self) -> bool {
        !self.reasons.is_empty()
    }

    /// Nothing is on screen (display off, locked).
    fn nothing_visible(&self) -> bool {
        nothing_visible(self.reasons)
    }

    /// Held, but the desktop is visible: widgets are told they're not visible (so they stop
    /// animating) while their layers keep showing what they last drew.
    fn frozen(&self) -> bool {
        self.held() && !self.nothing_visible()
    }

    /// Whether `layer` on `device` is hidden (see [`layer_state`]). The overlay is never
    /// shown over a covered monitor, edit mode or not (it doesn't exist in edit mode anyway).
    fn hides(&self, device: &str, layer: Layer) -> bool {
        let covered = self.covered.get(device).copied().unwrap_or(false);
        let editing = self.editing && layer == Layer::Desktop;
        layer_state(self.reasons, covered, editing) == LayerState::Hidden
    }

    // --- Hold -------------------------------------------------------------

    /// Combines every global hold reason, logging changes, and puts what needs it to
    /// sleep (or wakes it).
    fn sync_hold(&mut self) {
        let reasons = self.holds.reasons();
        if reasons == self.reasons {
            return;
        }
        if reasons.is_empty() {
            log::info!("released (was held: {})", self.reasons);
        } else {
            log::info!("held: {reasons}");
        }
        self.reasons = reasons;
        let held = !reasons.is_empty();
        self.plugins.set_held(held);
        self.stats.set_held(held, Instant::now());
        self.media.set_held(held);
        self.audio.set_held(held);
        if held && self.editing {
            log::info!("leaving edit mode");
            self.editing = false;
        }
        // Un-parks what may show again, hides or freezes the rest, and tells every shell.
        self.reconcile();
    }

    fn reload_config(&mut self) {
        match Config::load() {
            Ok(config) => {
                self.holds.apply(&config);
                self.sync_hold();
            }
            Err(e) => log::error!("{e:#}\n(keeping the current settings)"),
        }
        let theme = theme::current();
        let theme_changed = theme != self.theme;
        if theme_changed {
            self.theme = theme;
            for (instance, sub) in &self.theme_subs {
                self.emit_sub(instance, sub, json!(self.theme));
            }
        }
        // The theme's widget skin (appearance and style sheet), and so what `accent: auto`
        // means, may have changed with the theme, its files or the palette: the state carries it.
        let dir = theme::dir();
        let skin = look::load_skin(dir.as_deref());
        if dir != self.theme_dir {
            self.theme_dir = dir;
            self.watch_theme();
        }
        if skin != self.skin || theme_changed {
            self.skin = skin;
            self.push_all();
        }
    }

    /// Re-reads the monitors (position, size, DPI and work area) and re-lays out if any changed:
    /// the surfaces follow, the grid is recomputed (it stretches to the work area) and every
    /// widget is placed again, clamped so none sits under an app bar.
    fn refresh_monitors(&mut self) {
        let monitors = monitor_infos();
        if monitors != self.monitors {
            log::info!("monitors changed");
            self.monitors = monitors;
            self.icon_cell = icon_cell();
            self.reconcile();
        }
    }

    /// After `WM_DISPLAYCHANGE` or `WM_SETTINGCHANGE` for the work area (see `display.rs`).
    /// Unlike window events these arrive even when no window moved, e.g. an app bar that
    /// reserves screen space.
    fn relayout_for_display(&mut self) {
        log::debug!("display or work-area change announced");
        let icon_cell = icon_cell();
        if icon_cell != self.icon_cell {
            self.icon_cell = icon_cell; // icon spacing sets the grid's cell size
            self.reconcile();
        }
        self.refresh_monitors();
    }

    /// After window events: monitors, which are covered by a maximized or fullscreen window,
    /// z-order.
    fn recheck(&mut self) {
        self.refresh_monitors();
        if !self.ignore_covered {
            let rects: Vec<Rect> = self.monitors.iter().map(|m| m.rect).collect();
            self.covered = self
                .monitors
                .iter()
                .zip(covered_monitors(&rects))
                .map(|(monitor, covered)| (monitor.device.clone(), covered))
                .collect();
        }
        for surface in self.surfaces.iter().filter(|s| s.layer == Layer::Desktop) {
            self.host.keep_above_icons(surface.hwnd());
        }
        self.sync_hold();
        if self
            .parked
            .iter()
            .any(|(device, layer)| !self.hides(device, *layer))
        {
            self.reconcile(); // a covered monitor was uncovered: rebuild its layer
        } else {
            self.apply_visibility();
        }
    }

    /// Releases the layers (and so their WebViews) of monitors that have been hidden for
    /// [`PARK_AFTER`]. When that was every monitor with widgets, WebView2 itself goes.
    fn park_hidden(&mut self, now: Instant) {
        let due: Vec<LayerKey> = self
            .hidden_since
            .iter()
            .filter(|(key, since)| {
                let (device, layer) = *key;
                now >= **since + self.park_after
                    && !self.parked.contains(*key)
                    && self.hides(device, *layer)
            })
            .map(|(key, _)| key.clone())
            .collect();
        if due.is_empty() {
            return self.arm_park();
        }
        log::info!(
            "hidden for {:?}: releasing the widget layer and WebView on {}",
            self.park_after,
            due.iter()
                .map(|(device, layer)| format!("{device} ({layer:?})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.parked.extend(due);
        self.reconcile();
    }

    /// Wakes us when the next hidden layer is due to be parked.
    fn arm_park(&mut self) {
        self.timers.park = self
            .hidden_since
            .iter()
            .filter(|(device, _)| !self.parked.contains(*device))
            .map(|(_, since)| *since + self.park_after)
            .min();
    }

    /// Hides (and suspends) layers of monitors that are covered, or everywhere while nothing
    /// is on screen; shows the rest. Widgets hear `visibility:false` before the suspend and
    /// `true` after the resume. Notes since when each layer is hidden, for parking.
    fn apply_visibility(&mut self) {
        let now = Instant::now();
        for index in 0..self.surfaces.len() {
            let key: LayerKey = (
                self.surfaces[index].monitor.device.clone(),
                self.surfaces[index].layer,
            );
            let hidden = self.hides(&key.0, key.1);
            if hidden {
                self.hidden_since.entry(key).or_insert(now);
            } else {
                self.hidden_since.remove(&key);
            }
            let surface = &self.surfaces[index];
            if hidden == surface.hidden {
                continue;
            }
            if hidden {
                self.surfaces[index].hidden = true; // so the state says visible: false
                self.push_state(index);
                self.surfaces[index].hidden = false;
                self.surfaces[index].set_hidden(true);
            } else {
                self.surfaces[index].set_hidden(false);
                self.push_state(index);
                // `system` subscribers get a sample as soon as they're visible again.
                let now = Instant::now();
                let shown: Vec<String> = self
                    .instances_on(index)
                    .map(|instance| instance.instance.clone())
                    .collect();
                for instance in shown {
                    self.stats.refresh(&instance, now);
                }
            }
        }
        // Layers that no longer exist (parked, or gone) aren't timed.
        let live: BTreeSet<LayerKey> = self
            .surfaces
            .iter()
            .map(|s| (s.monitor.device.clone(), s.layer))
            .collect();
        self.hidden_since.retain(|key, _| live.contains(key));
        self.arm_park();
        self.sync_audio();
    }

    /// Tells the audio capture which instances are on screen and running: those on a layer
    /// that is shown, while the host isn't held. Capture runs only for those.
    fn sync_audio(&mut self) {
        let visible = if self.held() {
            BTreeSet::new()
        } else {
            (0..self.surfaces.len())
                .filter(|&index| !self.surfaces[index].hidden)
                .flat_map(|index| self.instances_on(index))
                .map(|instance| instance.instance.clone())
                .collect()
        };
        self.audio.set_visible(visible);
    }

    /// Which layer shows `instance` right now (None: nowhere, or its widget isn't installed).
    fn layer_of(&self, instance: &layout::Instance) -> Option<Layer> {
        let preset = self.registry.get(&instance.widget)?.manifest.preset;
        overlay::layer_for(preset, self.editing, self.layout.overlay)
    }

    /// The instances the surface at `index` shows.
    fn instances_on(&self, index: usize) -> impl Iterator<Item = &layout::Instance> {
        let surface = &self.surfaces[index];
        self.layout.instances.iter().filter(move |instance| {
            self.layer_of(instance) == Some(surface.layer)
                && self.shown_on(instance).as_deref() == Some(surface.monitor.device.as_str())
        })
    }

    /// Refuses adding (or duplicating) an instance of `manifest`'s widget onto `device`
    /// when its `instances` rule is used up, or when it is a popup (the bars host's).
    fn ensure_room(&self, layout: &Layout, manifest: &WidgetManifest, device: &str) -> Result<()> {
        ensure!(
            manifest.preset != Preset::Popup,
            "{:?} is a popup widget; the bars host places it",
            manifest.id
        );
        let (total, here) = layout.count_of(&manifest.id, |instance| {
            self.shown_on(instance).as_deref() == Some(device)
        });
        ensure!(
            manifest.instances.allows(total, here),
            "{:?} is {:?}: it has {total} instance(s), {here} on {device}",
            manifest.id,
            manifest.instances
        );
        Ok(())
    }

    /// A `file`/`image` setting asked for the file dialog: shows it, and sets the setting to
    /// the copy's URL when it closes (see [`on_picked`](Self::on_picked)).
    fn pick_setting(&mut self, index: usize, instance: String, key: String) {
        if self.layout_error.is_some() || self.picking || self.surface_of(&instance) != Some(index)
        {
            return self.push_all();
        }
        let setting = self.layout.get(&instance).and_then(|saved| {
            let manifest = self.registry.manifest_on_disk(&saved.widget)?;
            Some((
                saved.widget.clone(),
                manifest.setting(&key).filter(|s| s.is_file())?.extensions(),
            ))
        });
        let Some((widget, extensions)) = setting else {
            log::warn!("{instance}: {key:?} isn't a file or image setting");
            return self.push_all();
        };
        let request = pick::Request {
            origin: pick::Origin::Setting {
                instance: instance.clone(),
                key,
            },
            widget,
            instance,
            extensions,
            title: None,
            root: self.data_dir.clone(),
        };
        match pick::spawn(request, self.waker.clone()) {
            Ok(()) => self.picking = true,
            Err(e) => log::warn!("couldn't open the file dialog: {e}"),
        }
    }

    /// The file dialog closed: answers the widget's call, or stores the picked file's URL in
    /// the setting that asked.
    fn on_picked(&mut self, done: pick::Done) {
        self.picking = false;
        let pick::Done { origin, result } = done;
        match origin {
            pick::Origin::Call(target) => {
                let reply = result.map(|picked| picked.map_or(Value::Null, |p| json!(p)));
                self.reply(target, reply);
            }
            pick::Origin::Setting { instance, key } => match result {
                Ok(Some(picked)) => {
                    // A settings edit like any other: validated, saved, answered with a state.
                    let values = Map::from_iter([(key, Value::String(picked.url))]);
                    if let Some(index) = self.surface_of(&instance) {
                        self.edit(index, FromShell::Settings { instance, values });
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("{instance}: picking a file failed: {}", e.message),
            },
        }
    }

    // --- Edit mode and surfaces ---------------------------------------------

    fn set_editing(&mut self, on: bool) {
        if on && self.held() {
            log::info!("not entering edit mode while held ({})", self.reasons);
            return;
        }
        if self.editing == on {
            return;
        }
        self.editing = on;
        log::info!("edit mode {}", if on { "on" } else { "off" });
        self.reconcile();
    }

    fn devices(&self) -> (Vec<String>, usize) {
        let devices = self.monitors.iter().map(|m| m.device.clone()).collect();
        let primary = self.monitors.iter().position(|m| m.primary).unwrap_or(0);
        (devices, primary)
    }

    /// The device an instance shows on (its own monitor, or the primary).
    fn shown_on(&self, instance: &layout::Instance) -> Option<String> {
        let (devices, primary) = self.devices();
        layout::show_on(&instance.monitor, &devices, primary).map(str::to_owned)
    }

    /// The surface that shows `instance` (its monitor's window on the layer that has it).
    fn surface_of(&self, instance: &str) -> Option<usize> {
        let instance = self.layout.get(instance)?;
        let (device, layer) = (self.shown_on(instance)?, self.layer_of(instance)?);
        self.surfaces
            .iter()
            .position(|s| s.monitor.device == device && s.layer == layer)
    }

    /// Switches the overlay layer on or off, and remembers it in `widgets.json`.
    fn set_overlay(&mut self, on: bool) {
        if self.layout.overlay == on {
            return;
        }
        self.layout.overlay = on;
        log::info!("overlay widgets {}", if on { "on" } else { "off" });
        match &self.layout_error {
            None => {
                if let Err(e) = self.layout.save(&self.layout_path) {
                    log::warn!("couldn't save the overlay setting: {e:#}");
                }
            }
            Some(error) => {
                log::warn!("not saving the overlay setting while widgets.json is broken: {error}")
            }
        }
        self.reconcile();
    }

    /// Makes the surfaces match what should be on screen: one per monitor
    /// with instances, or every monitor while editing. Then pushes the state.
    fn reconcile(&mut self) {
        if self.shell.is_none() {
            self.surfaces.clear();
            self.env = Env::None;
            self.sync_hooks();
            self.sync_audio();
            return;
        }
        if self.window_events.is_none() {
            // Unwatched while no layer was up: the monitors may have changed.
            self.monitors = monitor_infos();
        }
        // The network filter serves picked files only for instances that exist.
        self.update_policy();
        // A parked layer stays parked while it is still hidden, and not otherwise. Monitors
        // that went away are forgotten.
        let hidden: BTreeSet<LayerKey> = self
            .monitors
            .iter()
            .flat_map(|m| [Layer::Desktop, Layer::Overlay].map(|layer| (m.device.clone(), layer)))
            .filter(|(device, layer)| self.hides(device, *layer))
            .collect();
        self.parked.retain(|key| hidden.contains(key));
        let mut wanted: Vec<(Monitor, Layer)> = Vec::new();
        for monitor in &self.monitors {
            for layer in [Layer::Desktop, Layer::Overlay] {
                let has_instances = self.layout.instances.iter().any(|instance| {
                    self.layer_of(instance) == Some(layer)
                        && self.shown_on(instance).as_deref() == Some(monitor.device.as_str())
                });
                if !self.parked.contains(&(monitor.device.clone(), layer))
                    && overlay::layer_wanted(
                        layer,
                        self.editing,
                        self.layout.overlay,
                        has_instances,
                    )
                {
                    wanted.push((monitor.clone(), layer));
                }
            }
        }
        self.surfaces.retain(|s| {
            wanted
                .iter()
                .any(|(m, layer)| m.device == s.monitor.device && *layer == s.layer)
        });
        for (monitor, layer) in wanted {
            match self
                .surfaces
                .iter_mut()
                .find(|s| s.monitor.device == monitor.device && s.layer == layer)
            {
                Some(surface) if surface.monitor != monitor => {
                    if let Err(e) = surface.set_monitor(&self.host, monitor) {
                        log::warn!("moving the widget layer: {e:#}");
                    }
                }
                Some(_) => {}
                None => {
                    self.next_surface += 1;
                    let created = match layer {
                        Layer::Desktop => Surface::new(self.next_surface, &self.host, monitor),
                        Layer::Overlay => Surface::new_overlay(self.next_surface, monitor),
                    };
                    match created {
                        Ok(surface) => self.surfaces.push(surface),
                        Err(e) => log::error!("creating the widget layer: {e:#}"),
                    }
                }
            }
        }
        if self.surfaces.is_empty() {
            if !matches!(self.env, Env::None) {
                log::info!("no widgets on screen; releasing WebView2");
            }
            self.env = Env::None;
        } else {
            self.ensure_webviews();
        }
        self.sync_hooks();
        self.push_all();
        self.apply_visibility();
    }

    /// The window-change hooks wake us on every window move anywhere, so they
    /// run only while they matter: a layer is up (or parked, and must see the
    /// window over its monitor leave) and the desktop is on screen. Pause,
    /// battery, lock and display arrive as notifications without them.
    fn sync_hooks(&mut self) {
        let layer = !self.surfaces.is_empty() || !self.parked.is_empty();
        let wanted = layer && !self.nothing_visible();
        match (wanted, self.window_events.is_some()) {
            (true, false) => {
                self.window_events = Some(WindowEvents::install());
                // What's covered or fullscreen right now, on the next turn.
                self.timers.recheck = Some(Instant::now());
            }
            (false, true) => self.window_events = None,
            _ => {}
        }
    }

    /// Creates the environment if needed, and a WebView for every surface without one.
    fn ensure_webviews(&mut self) {
        match &self.env {
            Env::None => {
                if self.timers.webview.is_some() {
                    return; // waiting to retry
                }
                self.env_generation += 1;
                match surface::create_environment(self.env_generation, &self.events) {
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
                for surface in self
                    .surfaces
                    .iter_mut()
                    .filter(|s| !s.has_web() && !s.creating)
                {
                    if let Err(e) = surface.create_web(&env, &self.events) {
                        log::error!("creating a WebView: {e:#}");
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
                        continue; // released meanwhile
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
                WebEvent::Controller { surface, result } => self.attach(surface, result),
                WebEvent::ScriptAdded { surface, id } => {
                    if let Some(surface) = self.surfaces.iter_mut().find(|s| s.id == surface) {
                        surface.on_script_added(id);
                    }
                }
                WebEvent::Message {
                    surface,
                    source,
                    json,
                } => {
                    if let Some(index) = self.surfaces.iter().position(|s| s.id == surface) {
                        self.on_shell_message(index, &source, &json);
                    }
                }
                WebEvent::ProcessFailed { surface, kind } => {
                    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                        log::warn!("the WebView2 browser process exited; starting over");
                        for surface in &mut self.surfaces {
                            surface.drop_web();
                        }
                        self.env = Env::None;
                        self.reconcile();
                    } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                        || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
                    {
                        log::warn!("a widget renderer failed ({kind:?}); reloading");
                        if let Some(surface) = self.surfaces.iter_mut().find(|s| s.id == surface) {
                            surface.reload();
                        }
                    }
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
        let (Env::Ready(env), Some(shell)) = (&self.env, &self.shell) else {
            return;
        };
        let Some(index) = self.surfaces.iter().position(|s| s.id == id) else {
            if let Ok(controller) = result {
                // SAFETY: COM call on a controller nobody else holds.
                let _ = unsafe { controller.Close() };
            }
            return;
        };
        let controller = match result {
            Ok(controller) => controller,
            Err(e) => {
                self.surfaces[index].creating = false;
                return log::error!("creating a WebView: {e}");
            }
        };
        let setup = Setup {
            env,
            shell_dir: &shell.dir,
            harden: &shell.harden,
            devtools: self.devtools,
            policy: &self.policy,
            events: &self.events,
        };
        if let Err(e) = self.surfaces[index].attach(controller, &setup) {
            log::error!("setting up a WebView: {e:#}");
        }
    }

    /// First run only (no `widgets.json` yet): place a few bundled widgets on
    /// the primary monitor and save, so the desktop is not empty. Once the
    /// file exists (even with every widget removed) this never runs again.
    fn seed_starter_layout(&mut self) {
        const STARTER: [&str; 3] = ["clock", "system-monitor", "now-playing"];
        let Some(monitor) = self
            .monitors
            .iter()
            .find(|m| m.primary)
            .or(self.monitors.first())
        else {
            return;
        };
        let grid = self.grid_for(monitor);
        let widgets: Vec<(&str, [u32; 2])> = STARTER
            .iter()
            .filter_map(|id| Some((*id, self.registry.get(id)?.manifest.size.default)))
            .collect();
        let starter = Layout::starter(&monitor.device, grid.cols, grid.rows, &widgets);
        if starter.instances.is_empty() {
            return;
        }
        match starter.save(&self.layout_path) {
            Ok(()) => {
                log::info!(
                    "first run: placed {} starter widgets",
                    starter.instances.len()
                );
                self.layout = starter;
            }
            Err(e) => log::warn!("could not save the starter layout: {e:#}"),
        }
    }

    // --- State for the shell --------------------------------------------------

    fn grid_for(&self, monitor: &Monitor) -> Grid {
        let scale = f64::from(monitor.dpi) / 96.0;
        let work = RectF {
            x: f64::from(monitor.work.x - monitor.rect.x) / scale,
            y: f64::from(monitor.work.y - monitor.rect.y) / scale,
            width: f64::from(monitor.work.width) / scale,
            height: f64::from(monitor.work.height) / scale,
        };
        Grid::new(&self.layout.grid, self.icon_cell, work)
    }

    /// What `accent: auto` means right now: the wallpaper palette's accent, else white.
    fn auto_accent(&self) -> Color {
        look::accent(self.theme.palette.as_ref().map(|p| p.accent.as_str()))
    }

    /// The instances `layer` shows on `monitor`, placed on its grid, each with its look merged
    /// from the layers (defaults < theme < user < instance).
    fn placed_on(&self, monitor: &Monitor, grid: &Grid, layer: Layer) -> Vec<Placed> {
        let base = look::base(&self.skin);
        let accent = self.auto_accent();
        self.layout
            .instances
            .iter()
            .filter(|instance| {
                self.layer_of(instance) == Some(layer)
                    && self.shown_on(instance).as_deref() == Some(monitor.device.as_str())
            })
            .filter_map(|instance| {
                let installed = self.registry.get(&instance.widget)?;
                let look =
                    look::resolve(&instance.appearance, &self.layout.appearance, &base, accent);
                Some(layout::place(instance, &installed.manifest, grid, &look))
            })
            .collect()
    }

    /// Sends a surface its state and fits its click-through region to it.
    fn push_state(&self, index: usize) {
        let surface = &self.surfaces[index];
        let monitor = &surface.monitor;
        let scale = f64::from(monitor.dpi) / 96.0;
        let grid = self.grid_for(monitor);
        let placed = self.placed_on(monitor, &grid, surface.layer);
        if self.editing && surface.layer == Layer::Desktop {
            surface.set_region(&[surface.full_region()]);
        } else {
            let rects: Vec<[i32; 4]> = placed
                .iter()
                .map(|p| grid.rect(p.cell, p.size).to_pixels(scale))
                .collect();
            surface.set_region(&rects);
        }
        if !surface.ready {
            return;
        }
        // The palette: popups are the bars host's, never offered here.
        let widgets: Vec<&WidgetManifest> = self
            .registry
            .widgets
            .values()
            .map(|w| &w.manifest)
            .filter(|manifest| manifest.preset != Preset::Popup)
            .collect();
        let to_dip = |px: i32| f64::from(px) / scale;
        let state = json!({
            "type": "state",
            "monitor": {
                "device": monitor.device,
                "dpi": monitor.dpi,
                "width": to_dip(monitor.rect.width),
                "height": to_dip(monitor.rect.height),
            },
            "work": {
                "x": to_dip(monitor.work.x - monitor.rect.x),
                "y": to_dip(monitor.work.y - monitor.rect.y),
                "width": to_dip(monitor.work.width),
                "height": to_dip(monitor.work.height),
            },
            "grid": grid,
            "instances": placed,
            "widgets": widgets,
            "layer": surface.layer,
            "backdrop": if surface.layer == Layer::Desktop { crate::backdrop::payload(monitor) } else { Value::Null },
            "glass": look::liquid(&self.skin),
            "counts": self.layout.counts(),
            // The look: the layers below an instance's own (for the Appearance panel), the
            // color `auto` stands for, and the theme's style sheet.
            "appearance": {
                "base": look::json_of(&look::base(&self.skin)),
                "user": look::json_of(&self.layout.appearance),
                "accent": self.auto_accent().hex(),
            },
            "themeCss": self.skin.css,
            "editing": self.editing,
            // Frozen (paused, on battery) layers stay on screen but tell widgets to stop.
            "visible": !surface.hidden && !self.frozen(),
            "error": self.layout_error,
        });
        surface.post(&state.to_string());
    }

    fn push_all(&mut self) {
        let base = look::base(&self.skin);
        let accent = self.auto_accent();
        crate::backdrop::request(self.layout.instances.iter().any(|instance| {
            let look = look::resolve(&instance.appearance, &self.layout.appearance, &base, accent);
            look.style == Some(WidgetStyle::Liquid) && look.surface == Some(SurfaceKind::Glass)
        }));
        for index in 0..self.surfaces.len() {
            self.push_state(index);
        }
    }

    // --- Messages from the shell ----------------------------------------------

    fn on_shell_message(&mut self, index: usize, source: &str, json: &str) {
        if !source.starts_with("https://shell.mypaper/") {
            return log::warn!("ignored a message from {source}");
        }
        let message: FromShell = match serde_json::from_str(json) {
            Ok(message) => message,
            Err(e) => return log::warn!("bad message from the shell ({e}): {json}"),
        };
        match message {
            FromShell::Ready => {
                self.surfaces[index].ready = true;
                self.push_state(index);
                self.surfaces[index].probe_paint();
            }
            FromShell::Hello { instance } => self.forget(&instance),
            FromShell::Call {
                instance,
                id,
                method,
                params,
            } => {
                let target = Target { instance, id };
                if let Some(reply) = self.call(index, &target, &method, params) {
                    self.reply(target, reply);
                }
            }
            FromShell::Edit { on } => self.set_editing(on),
            FromShell::PickFile { instance, key, .. } => self.pick_setting(index, instance, key),
            FromShell::Plates { plates } => {
                log::debug!("legacy shell reported {} plates", plates.len())
            }
            FromShell::Glass { drawn } => log::info!(
                "widget glass {} {:?}: {drawn} snapshot plates{}",
                self.surfaces[index].monitor.device,
                self.surfaces[index].layer,
                if drawn == 0 { " (CSS fallback)" } else { "" }
            ),
            edit => self.edit(index, edit),
        }
    }

    /// Runs a widget call. `None` means the answer comes later.
    fn call(
        &mut self,
        index: usize,
        target: &Target,
        method: &str,
        params: Value,
    ) -> Option<Reply> {
        let fail = |code: ErrorCode, message: String| -> Option<Reply> {
            Some(Err(WidgetError::new(code, message)))
        };
        let instance = target.instance.as_str();
        let Some(saved) = self.layout.get(instance).cloned() else {
            return fail(ErrorCode::NotFound, format!("no instance {instance:?}"));
        };
        if self.surface_of(instance) != Some(index) {
            return fail(
                ErrorCode::PermissionDenied,
                format!("{instance:?} isn't on this monitor"),
            );
        }
        // Permissions come from the manifest on disk, never from the message.
        let Some(manifest) = self.registry.manifest_on_disk(&saved.widget) else {
            return fail(
                ErrorCode::Internal,
                format!("can't read the manifest of {:?}", saved.widget),
            );
        };
        let method = match Method::parse(method, params) {
            Ok(method) => method,
            Err(e) => return Some(Err(e)),
        };
        if let Err(e) = method.check(&manifest) {
            log::info!("{instance}: {}", e.message);
            return Some(Err(e));
        }
        let now = Instant::now();
        let internal = |e: anyhow::Error| WidgetError::new(ErrorCode::Internal, format!("{e:#}"));
        let done = Some(Ok(Value::Null));
        match method {
            Method::StorageGet { key, shared } => {
                let scope = shared
                    .as_deref()
                    .map_or_else(|| instance.to_owned(), storage::shared);
                Some(self.storage.get(&scope, &key).map_err(internal))
            }
            Method::StorageSet { key, value, shared } => {
                let scope = shared
                    .as_deref()
                    .map_or_else(|| instance.to_owned(), storage::shared);
                Some(
                    self.storage
                        .set(&scope, &key, value)
                        .map(|()| Value::Null)
                        .map_err(internal),
                )
            }
            Method::SettingsSet { values } => {
                let values = match manifest.check_settings(values) {
                    Ok(values) => values,
                    Err(e) => return fail(ErrorCode::InvalidParams, format!("{e:#}")),
                };
                // Saved like an edit from the form: the new state brings the widget its settings.
                self.edit(
                    index,
                    FromShell::Settings {
                        instance: instance.to_owned(),
                        values,
                    },
                );
                done
            }
            Method::UserInfo => match userinfo::cached() {
                Some(info) => Some(Ok(json!(info))),
                None => {
                    userinfo::read_for(target.clone(), self.waker.clone());
                    None
                }
            },
            Method::AppIcon { target: path, size } => self.icons.get(target.clone(), &path, size),
            Method::PickFile { extensions, title } => {
                if self.picking {
                    return fail(ErrorCode::Internal, "a file dialog is already open".into());
                }
                let request = pick::Request {
                    origin: pick::Origin::Call(target.clone()),
                    widget: saved.widget.clone(),
                    instance: instance.to_owned(),
                    extensions,
                    title,
                    root: self.data_dir.clone(),
                };
                match pick::spawn(request, self.waker.clone()) {
                    Ok(()) => {
                        self.picking = true;
                        None
                    }
                    Err(e) => fail(
                        ErrorCode::Internal,
                        format!("couldn't open the file dialog: {e}"),
                    ),
                }
            }
            Method::SystemStats => {
                self.stats.request(target.clone(), now);
                None
            }
            Method::MediaCurrent => {
                self.media.current(target.clone());
                None
            }
            Method::MediaControl { action } => {
                self.media.control(target.clone(), action);
                None
            }
            Method::Launch { target: what } => {
                launch(target.clone(), what, self.waker.clone());
                None
            }
            Method::ThemeCurrent => Some(Ok(json!(self.theme))),
            Method::Plugin {
                plugin,
                method,
                params,
            } => {
                let out = self
                    .plugins
                    .call(target.clone(), &plugin, method, params, now);
                self.deliver(out.into_iter().collect());
                None
            }
            Method::Subscribe { sub, topic } => {
                self.unsubscribe(instance, &sub);
                match topic {
                    Topic::System { interval_ms } => {
                        let interval = Duration::from_millis(interval_ms);
                        self.stats.subscribe(instance, &sub, interval, now);
                    }
                    Topic::Media => self.media.subscribe(instance, &sub),
                    Topic::Audio { bins } => self.audio.subscribe(instance, &sub, bins),
                    Topic::Theme => {
                        self.theme_subs.insert((instance.to_owned(), sub));
                    }
                    Topic::Plugin { plugin, event } => {
                        let subscribed =
                            self.plugins.subscribe(instance, &sub, &plugin, &event, now);
                        return Some(subscribed.map(|()| Value::Null));
                    }
                }
                done
            }
            Method::Unsubscribe { sub } => {
                self.unsubscribe(instance, &sub);
                done
            }
        }
    }

    /// Ends subscription `sub`, whatever its topic.
    fn unsubscribe(&mut self, instance: &str, sub: &str) {
        self.stats.unsubscribe(instance, sub);
        self.media.unsubscribe(instance, sub);
        self.audio.unsubscribe(instance, sub);
        self.plugins.unsubscribe(instance, sub);
        self.theme_subs
            .remove(&(instance.to_owned(), sub.to_owned()));
    }

    /// Drops every subscription an instance holds.
    fn forget(&mut self, instance: &str) {
        self.stats.forget(instance);
        self.media.forget(instance);
        self.audio.forget(instance);
        self.plugins.forget(instance);
        self.theme_subs.retain(|(i, _)| i != instance);
    }

    fn deliver(&mut self, out: Vec<Out>) {
        for out in out {
            match out {
                Out::Reply(target, reply) => self.reply(target, reply),
                Out::Event {
                    instance,
                    event,
                    data,
                } => self.emit(&instance, event, data),
            }
        }
    }

    fn reply(&self, target: Target, reply: Reply) {
        let Some(index) = self.surface_of(&target.instance) else {
            return;
        };
        let message = match reply {
            Ok(result) => {
                json!({ "type": "reply", "instance": target.instance, "id": target.id, "result": result })
            }
            Err(error) => {
                json!({ "type": "reply", "instance": target.instance, "id": target.id, "error": error })
            }
        };
        self.surfaces[index].post(&message.to_string());
    }

    /// Sends an event to an instance's frame. Hidden monitors get nothing:
    /// their WebView is suspended, and their widgets are told so.
    fn emit(&self, instance: &str, event: &str, data: Value) {
        let Some(index) = self.surface_of(instance) else {
            return;
        };
        if self.surfaces[index].hidden {
            return;
        }
        let message =
            json!({ "type": "event", "instance": instance, "event": event, "data": data });
        self.surfaces[index].post(&message.to_string());
    }

    /// A subscription value: `{ sub, value }` as a `sub` event.
    fn emit_sub(&self, instance: &str, sub: &str, value: Value) {
        self.emit(instance, "sub", json!({ "sub": sub, "value": value }));
    }

    // --- Layout edits -----------------------------------------------------------

    /// Applies an edit from the shell, clamped, and saves it. A refused edit
    /// just gets the current state back.
    fn edit(&mut self, index: usize, edit: FromShell) {
        if let Some(error) = &self.layout_error {
            log::warn!("not saving edits while widgets.json is broken: {error}");
            return self.push_all();
        }
        let monitor = self.surfaces[index].monitor.clone();
        let grid = self.grid_for(&monitor);
        let mut next = self.layout.clone();
        let mut removed = None;
        let mut duplicated = None;
        let applied = (|| -> Result<()> {
            let manifest_of = |widget: &str| -> Result<WidgetManifest> {
                Ok(self
                    .registry
                    .get(widget)
                    .with_context(|| format!("widget {widget:?} isn't installed"))?
                    .manifest
                    .clone())
            };
            match edit {
                FromShell::Layout { changes } => {
                    let clamped = changes
                        .into_iter()
                        .map(|change| {
                            let saved = next
                                .get(&change.instance)
                                .with_context(|| format!("no instance {:?}", change.instance))?;
                            let size = manifest_of(&saved.widget)?.clamp_size(change.size);
                            let (cell, size) = grid.clamp(change.cell, size);
                            Ok((change.instance, cell, size))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    next.apply(&clamped)
                }
                FromShell::Add { widget, cell, size } => {
                    let manifest = manifest_of(&widget)?;
                    self.ensure_room(&next, &manifest, &monitor.device)?;
                    let (cell, size) = grid.clamp(cell, manifest.clamp_size(size));
                    let id = next.add(&widget, &monitor.device, cell, size);
                    log::info!("added {id}");
                    Ok(())
                }
                FromShell::Duplicate { instance, cell } => {
                    let saved = next
                        .get(&instance)
                        .with_context(|| format!("no instance {instance:?}"))?;
                    let manifest = manifest_of(&saved.widget)?;
                    let device = self.shown_on(saved).unwrap_or_default();
                    self.ensure_room(&next, &manifest, &device)?;
                    let (cell, _) = grid.clamp(cell, manifest.clamp_size(saved.size));
                    let id = next.duplicate(&instance, cell)?;
                    // The copy keeps its own pictures: its file settings point at its own folder.
                    let rebased: Map<String, Value> = manifest
                        .settings
                        .iter()
                        .filter(|setting| setting.is_file())
                        .filter_map(|setting| {
                            let saved = next.get(&id)?.settings.get(setting.key())?.as_str()?;
                            Some((setting.key().to_owned(), pick::rebase(saved, &id)?.into()))
                        })
                        .collect();
                    next.set_settings(&id, rebased)?;
                    log::info!("duplicated {instance} as {id}");
                    duplicated = Some((instance, id));
                    Ok(())
                }
                FromShell::Remove { instance } => {
                    next.remove(&instance)?;
                    log::info!("removed {instance}");
                    removed = Some(instance);
                    Ok(())
                }
                FromShell::Settings { instance, values } => {
                    let saved = next
                        .get(&instance)
                        .with_context(|| format!("no instance {instance:?}"))?;
                    let manifest = self
                        .registry
                        .manifest_on_disk(&saved.widget)
                        .context("can't read the widget's manifest")?;
                    let values = manifest.check_settings(values)?;
                    next.set_settings(&instance, values)
                }
                FromShell::Appearance { instance, values } => {
                    next.set_appearance(instance.as_deref(), &values)
                }
                FromShell::Ready
                | FromShell::Hello { .. }
                | FromShell::Call { .. }
                | FromShell::Edit { .. }
                | FromShell::PickFile { .. }
                | FromShell::Plates { .. }
                | FromShell::Glass { .. } => Ok(()),
            }
        })();
        if let Err(e) = applied.and_then(|()| next.save(&self.layout_path)) {
            log::warn!("edit refused: {e:#}");
            return self.push_all();
        }
        self.layout = next;
        if let Some(instance) = removed {
            self.forget(&instance);
            pick::remove_instance(&self.data_dir, &instance);
        }
        if let Some((from, to)) = duplicated {
            pick::copy_instance(&self.data_dir, &from, &to);
        }
        // The new state carries the settings; the shell pushes them to the widget.
        self.reconcile();
    }

    /// `widgets.json` changed on disk (Studio, an editor, or our own save).
    fn reload_layout(&mut self) {
        match Layout::load(&self.layout_path) {
            Ok(mut layout) => {
                migrate_layout(&self.registry, &self.layout_path, &mut layout);
                let was_broken = self.layout_error.take().is_some();
                if layout == self.layout && !was_broken {
                    return; // our own save coming back
                }
                log::info!("widgets.json changed");
                // Instances that vanished from the file (Studio removes them there) take their
                // picked files with them.
                for gone in self
                    .layout
                    .instances
                    .iter()
                    .filter(|old| layout.get(&old.instance).is_none())
                {
                    pick::remove_instance(&self.data_dir, &gone.instance);
                }
                self.layout = layout;
                self.reconcile();
            }
            Err(e) => {
                log::error!("{e:#}\n(keeping the last good layout)");
                self.layout_error = Some(format!("{e:#}"));
                self.push_all();
            }
        }
    }

    // --- Installed widgets, the shell, the network policy -------------------------

    fn update_policy(&self) {
        let policy = self
            .registry
            .widgets
            .iter()
            .map(|(id, installed)| {
                let net = WidgetNet {
                    dir: installed.dir.clone(),
                    origins: installed.manifest.network_origins().cloned().collect(),
                    data_dir: self.data_dir.clone(),
                    instances: self
                        .layout
                        .instances
                        .iter()
                        .filter(|instance| &instance.widget == id)
                        .map(|instance| instance.instance.clone())
                        .collect(),
                };
                (id.clone(), net)
            })
            .collect();
        *self.policy.borrow_mut() = policy;
    }

    fn rescan_widgets(&mut self) {
        self.roots = registry::widget_roots();
        self.registry = Registry::scan(&self.roots);
        if self.layout_error.is_none() {
            migrate_layout(&self.registry, &self.layout_path, &mut self.layout);
        }
        self.update_policy(); // the network filter serves widget files from this
        // Fresh manifests first, so reloaded frames get them.
        self.reconcile();
        for widget in std::mem::take(&mut self.changed_widgets) {
            if self.registry.get(&widget).is_some() {
                log::info!("{widget} changed; reloading it");
                let message = json!({ "type": "reload", "widget": widget }).to_string();
                for surface in &self.surfaces {
                    surface.post(&message);
                }
            }
        }
    }

    fn reload_shell(&mut self) {
        let Some(shell) = load_shell() else { return };
        log::info!("the shell was rebuilt; reloading it");
        for surface in &mut self.surfaces {
            if let Err(e) = surface.replace_script(&shell.harden, &self.events) {
                log::warn!("reloading the shell: {e:#}");
            }
        }
        let first = self.shell.is_none();
        self.shell = Some(shell);
        if first {
            self.reconcile();
        }
    }

    /// File watches: `%APPDATA%\mypaper` (config, layout, user widgets), the
    /// bundled `widgets/` (widgets and the shell build), and the palette.
    fn watch(&mut self) {
        let waker = self.waker.clone();
        if let Ok(appdata) = mypaper_config::dir() {
            let root = appdata.clone();
            let widgets = appdata.join("widgets");
            let classify = move |path: &Path| -> Option<Msg> {
                if path.parent() == Some(root.as_path()) {
                    return match path.file_name()?.to_str()? {
                        "config.json" => Some(Msg::Config),
                        "widgets.json" => Some(Msg::Layout),
                        _ => None,
                    };
                }
                Registry::widget_of(path, std::slice::from_ref(&widgets))
                    .map(|id| Msg::Widgets(vec![id]))
            };
            let _ = std::fs::create_dir_all(&appdata);
            self.watch_dir(&appdata, classify, waker.clone());
        }
        if let Some(bundled) = crate::files::bundled_widgets() {
            let root = bundled.clone();
            let classify = move |path: &Path| -> Option<Msg> {
                let inside = path.strip_prefix(&root).ok()?;
                if inside.starts_with("shell") {
                    return inside.starts_with("shell/dist").then_some(Msg::Shell);
                }
                Registry::widget_of(path, std::slice::from_ref(&root))
                    .map(|id| Msg::Widgets(vec![id]))
            };
            self.watch_dir(&bundled, classify, waker.clone());
        }
        if let Ok(palette) = crate::files::local("palette.json") {
            let waker = waker.clone();
            self.palette_watch =
                mypaper_desktop::watch_file(&palette, move || waker.send(Msg::Palette))
                    .inspect_err(|e| log::warn!("not watching the palette: {e:#}"))
                    .ok();
        }
        if let Ok(dir) = crate::files::local("backdrop") {
            let _ = std::fs::create_dir_all(&dir);
            let root = dir.clone();
            self.watch_dir(
                &dir,
                move |path| {
                    (path.parent() == Some(root.as_path())
                        && path.extension().is_some_and(|e| e == "bmp"))
                    .then_some(Msg::Backdrop)
                },
                waker.clone(),
            );
        }
        self.watch_theme();
    }

    /// (Re)watches the active theme's folder for edits to `theme.json` and style sheets, which
    /// restyle the widgets live.
    fn watch_theme(&mut self) {
        self.theme_watch = None;
        let Some(dir) = self.theme_dir.clone() else {
            return;
        };
        let root = dir.clone();
        let classify = move |path: &Path| -> Option<Msg> {
            let name = path.strip_prefix(&root).ok()?.file_name()?.to_str()?;
            (name == mypaper_theme::MANIFEST || name.ends_with(".css")).then_some(Msg::Theme)
        };
        self.theme_watch = start_watch(&dir, classify, self.waker.clone());
    }

    fn watch_dir(
        &mut self,
        dir: &Path,
        classify: impl Fn(&Path) -> Option<Msg> + Send + 'static,
        waker: Waker,
    ) {
        self.watchers.extend(start_watch(dir, classify, waker));
    }
}

/// Watches `dir` recursively; every changed path `classify` recognizes becomes a [`Msg`].
fn start_watch(
    dir: &Path,
    classify: impl Fn(&Path) -> Option<Msg> + Send + 'static,
    waker: Waker,
) -> Option<notify::RecommendedWatcher> {
    let path = dir.to_owned();
    let watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event) if !event.kind.is_access() => {
                for msg in event.paths.iter().filter_map(|p| classify(p)) {
                    waker.send(msg);
                }
            }
            Ok(_) => {}
            Err(e) => waker.send(Msg::WatchError(path.clone(), e.to_string())),
        });
    match watcher.and_then(|mut w| w.watch(dir, notify::RecursiveMode::Recursive).map(|()| w)) {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            log::warn!("not watching {}: {e}", dir.display());
            None
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        crate::backdrop::request(false);
        self.plugins.shutdown();
        self.surfaces.clear();
        self.env = Env::None;
        for (registered, id) in [
            (self.hotkey, EDIT_HOTKEY),
            (self.overlay_hotkey, OVERLAY_HOTKEY),
        ] {
            if registered {
                // SAFETY: a hotkey we registered on this thread.
                let _ = unsafe { UnregisterHotKey(None, id) };
            }
        }
    }
}

/// Opens a path or URL with its default handler, off the main thread
/// (ShellExecute can block on slow shares and shell extensions).
fn launch(target: Target, what: String, waker: Waker) {
    let spawned = std::thread::Builder::new()
        .name("launch".into())
        .spawn(move || {
            // SAFETY: COM for ShellExecute on this short-lived thread; valid strings.
            let result = unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                ShellExecuteW(
                    None,
                    w!("open"),
                    &HSTRING::from(&what),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                )
            };
            let reply = if result.0 as isize > 32 {
                Ok(Value::Null)
            } else {
                Err(WidgetError::new(
                    ErrorCode::Internal,
                    format!("couldn't open {what:?}"),
                ))
            };
            waker.send(Msg::Answered(target, reply));
        });
    if let Err(e) = spawned {
        log::warn!("launch: {e}");
    }
}

/// The desktop icon spacing as cell size in DIPs (see [`layout::icon_spacing_dip`]).
fn icon_cell() -> [f64; 2] {
    let read = |name: PCWSTR| -> Option<i32> {
        let mut buffer = [0u16; 32];
        let mut bytes = size_of_val(&buffer) as u32;
        // SAFETY: the buffer and its size outlive the call.
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
        String::from_utf16_lossy(&buffer[..len]).trim().parse().ok()
    };
    [
        layout::icon_spacing_dip(read(w!("IconSpacing"))),
        layout::icon_spacing_dip(read(w!("IconVerticalSpacing"))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reasons(paused: bool, on_battery: bool, display_off: bool, locked: bool) -> HoldReasons {
        let mut holds = Holds::default();
        holds.apply(&Config {
            paused,
            pause_on_battery: true,
            pause_on_fullscreen: true,
            ..Config::default()
        });
        holds.set(Fact::OnBattery, on_battery);
        holds.set(Fact::DisplayOff, display_off);
        holds.set(Fact::SessionLocked, locked);
        holds.reasons()
    }

    #[test]
    fn only_the_covered_monitor_pauses() {
        let none = reasons(false, false, false, false);
        // A game on another monitor: this monitor is not covered, so it keeps running.
        assert_eq!(layer_state(none, false, false), LayerState::Live);
        assert_eq!(layer_state(none, true, false), LayerState::Hidden);
        // Edit mode shows a covered monitor anyway.
        assert_eq!(layer_state(none, true, true), LayerState::Live);
    }

    #[test]
    fn a_global_pause_freezes_visible_layers_and_hides_the_rest() {
        let paused = reasons(true, false, false, false);
        assert_eq!(
            layer_state(paused, false, false),
            LayerState::Frozen,
            "still on screen"
        );
        assert_eq!(
            layer_state(paused, true, false),
            LayerState::Hidden,
            "covered: nothing to see"
        );
        let battery = reasons(false, true, false, false);
        assert_eq!(layer_state(battery, false, false), LayerState::Frozen);
        // Nothing is on screen at all: everything hides, covered or not.
        for off in [
            reasons(false, false, true, false),
            reasons(false, false, false, true),
        ] {
            assert_eq!(layer_state(off, false, false), LayerState::Hidden);
        }
    }
}
