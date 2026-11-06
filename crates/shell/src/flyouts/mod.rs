//! Native flyuts (docs/bars.md, "Flyouts"): calendar, quick settings, Wi-Fi, mixer,
//! notification center, launcher, power, tray overflow and language, each a panel drawn with
//! `mypaper-ui` in one reused popup window, with engine glass under it (crate::glass).
//!
//! - **The seam.** The bars call [`open`] for every panel-opening click; the request is queued
//!   and [`Flyouts::process`] opens it in the same loop pass (content on screen in ms; data
//!   follows from provider lanes). Win, Win+N and Win+A come through `mypaper_bars::flyouts::
//!   winkey` and open the launcher, notifications and quick settings.
//! - **One at a time, never overlapping.** Opening another replaces it; the same one again
//!   closes it. It closes on Escape, on a click elsewhere (deactivation), and when an action
//!   hands over to Windows.
//! - **Motion** is position and alpha only (a spring), on the shared ticker armed only while it
//!   moves: once settled nothing redraws until its data changes.
//! - **Providers** run on lanes (`mypaper_bars::flyouts::Lane`) started on first use and
//!   dropped a few seconds after the flyout closes; nothing polls while closed.

pub(crate) mod calendar;
mod controls;
pub(crate) mod kit;
mod language;
mod launcher;
pub mod look;
mod mixer;
pub(crate) mod notifications;
mod plugin_panel;
mod power;
mod quick;
mod settings_pages;
#[cfg(test)]
mod tests;
mod toggles;
mod tray;
mod wifi;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_bars::actions::Topic;
use mypaper_bars::audio::Audio;
use mypaper_bars::flyouts::index::Builder;
use mypaper_bars::flyouts::mixer::MixerLane;
use mypaper_bars::flyouts::quick::QuickLane;
use mypaper_bars::flyouts::wifi::WifiLane;
use mypaper_bars::flyouts::winkey::{self, WinKey};
use mypaper_bars::flyouts::{self as providers, Done, Lane, Watched};
use mypaper_bars::wake::{Msg, Waker};
use mypaper_desktop::Monitor;
use mypaper_ui::{
    Button, Event, Glass, HitId, Image, Kind as SurfaceKind, Rect, Role, Surface, SurfaceId,
    TextField, Ticker, Ui, focus,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowRect};

use self::kit::Kit;
use self::look::Look;
use crate::bars::Bars;

/// What a bar click (or a Windows key shortcut) opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Integration(mypaper_integrations::Provider),
    AppActions,
    Calendar,
    Quick,
    Wifi,
    Mixer,
    Notifications,
    Launcher,
    Power,
    TrayOverflow,
    Language,
    /// A component plugin's panel, by its `bars::plugin_modules::panel_number`.
    Plugin(u16),
}

/// A request from the bars, handled by [`Flyouts::process`] in the same loop pass.
struct Request {
    kind: Kind,
    anchor: Rect,
    monitor: Monitor,
    /// When it was asked for.
    at: Instant,
}

thread_local! {
    static REQUESTS: RefCell<Vec<Request>> = const { RefCell::new(Vec::new()) };
}

/// Opens flyout `kind` for `anchor_rect` (the clicked module, in physical **screen** pixels) on
/// `monitor`, or closes it when it is the one open. The bars' only entry point.
pub fn open(kind: Kind, anchor_rect: Rect, monitor: &Monitor) {
    // SAFETY: plain queries of this thread's last message time and the tick count.
    let since_input = unsafe {
        windows::Win32::System::SystemInformation::GetTickCount()
            .wrapping_sub(windows::Win32::UI::WindowsAndMessaging::GetMessageTime() as u32)
    };
    log::debug!("flyouts: {kind:?} asked for {since_input} ms after its input");
    REQUESTS.with_borrow_mut(|r| {
        r.push(Request {
            kind,
            anchor: anchor_rect,
            monitor: monitor.clone(),
            at: Instant::now(),
        })
    });
}

/// What a panel's input came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Its state changed: draw it again.
    Redraw,
    /// It is done (an action handed over, or a choice was made).
    Close,
    /// Another flyout replaces it (quick settings' chevrons).
    Switch(Kind),
    Menu,
    /// An app's own menu opens from it (a tray icon's right click): it stays open while that
    /// menu is in front, and closes once something else is.
    HoldForMenu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelCommand {
    PinLauncher(mypaper_bars::config::Pinned),
    UnpinLauncher(String),
    PinDock(mypaper_bars::config::Pinned),
}

/// A provider's answer, from a lane.
pub enum Reply {
    Quick(mypaper_bars::flyouts::quick::Quick),
    QuickControl(controls::State, Option<String>),
    /// Do not disturb as Windows reports it, and why a change failed (`toggles::dnd`).
    Dnd(Option<bool>, Option<String>),
    Mixer(mypaper_bars::flyouts::mixer::MixerState),
    Wifi(mypaper_bars::flyouts::wifi::WifiState),
    NotificationIcons(Vec<(String, Option<String>)>),
    NotificationAccess(Result<(), String>),
    Apps(Vec<mypaper_bars::flyouts::index::App>),
    Files(Vec<mypaper_bars::flyouts::index::RecentFile>),
    IndexedFiles {
        query: String,
        hits: Option<Vec<mypaper_index::Hit>>,
    },
    Clipboard(mypaper_bars::flyouts::clipboard::History),
    /// An action failed; the panel shows why.
    Failed(String),
    /// An action finished; the panel reads its state again.
    Done,
}

/// One panel's behaviour; the controller owns the window, motion and input routing.
pub trait Panel {
    fn swipe(&mut self, _id: HitId, _dx: f32, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    fn menu(&mut self) -> Vec<mypaper_ui::Entry<PanelCommand>> {
        vec![]
    }
    fn command(&mut self, _command: PanelCommand, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    fn middle(&mut self, _id: HitId, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    fn double(&mut self, _id: HitId, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    /// Width in design pixels.
    fn width(&self) -> f32;
    /// Lays out the content into `kit` (its body is added by the controller).
    fn view(&self, kit: &mut Kit);
    /// A click on (or Enter/Space at) hit `id`.
    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect;
    /// A right click on hit `id`.
    fn context(&mut self, _id: HitId, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    /// The window is on screen: slow set-up that can follow a frame later (a COM endpoint, a
    /// registry walk, icons) goes here instead of in its constructor. Whether it changed.
    fn opened(&mut self, _cx: &mut Cx) -> bool {
        false
    }
    /// Follows state the shell already holds (the bars' tray), on every loop pass while open;
    /// whether it changed.
    fn refresh(&mut self, _cx: &mut Cx) -> bool {
        false
    }
    /// Slider `id` at `value` (0..=1); `done` when the drag ends.
    fn slide(&mut self, _id: HitId, _value: f32, _done: bool, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    /// The wheel over hit `id` (or the body): lists scroll.
    fn wheel(&mut self, _id: Option<HitId>, _delta: i16, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    /// A provider answered; whether the panel changed.
    fn reply(&mut self, _reply: &Reply, _cx: &mut Cx) -> bool {
        false
    }
    /// A worker said something changed (the mixer's sessions, Wi-Fi scan results, the volume).
    fn changed(&mut self, _msg: &Msg, _cx: &mut Cx) -> bool {
        false
    }
    /// A key the panel wants before the controller's own (the launcher's).
    fn key(&mut self, _key: u16, _cx: &mut Cx) -> Option<Effect> {
        None
    }
    /// Time passed: whether it changed, and when to call again.
    fn tick(&mut self, _now: Instant, _cx: &mut Cx) -> (bool, Option<Instant>) {
        (false, None)
    }
    /// The panel's text field, which the surface feeds IME and keys.
    fn field(&self) -> Option<&TextField> {
        None
    }
    fn text_changed(&mut self, _cx: &mut Cx) -> Effect {
        Effect::None
    }
    fn submit(&mut self, _cx: &mut Cx) -> Effect {
        Effect::None
    }
}

/// What panels reach while handling input: providers, the bars' tray, images, the look.
pub struct Cx<'a> {
    pub lanes: &'a mut Lanes,
    pub bars: &'a mut Bars,
    pub images: &'a mut Images,
    pub ui: &'a Ui,
    pub look: &'a Look,
    /// The hit being activated, in screen px (where tray clicks anchor their menus).
    pub hit: Rect,
    /// Device pixels per design pixel on the flyout's monitor.
    pub scale: f32,
}

impl Cx<'_> {
    /// A cached app/notification icon, decoded on first use.
    pub fn image(&mut self, path: Option<&str>) -> Option<Arc<Image>> {
        self.images.get(self.ui, path?)
    }
}

/// Decoded icons of the open flyout, dropped when it closes.
#[derive(Default)]
pub struct Images(HashMap<String, Option<Arc<Image>>>);

impl Images {
    fn get(&mut self, ui: &Ui, path: &str) -> Option<Arc<Image>> {
        if self.0.len() > 256 {
            self.0.clear();
        }
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| {
                Image::load(ui, std::path::Path::new(path))
                    .inspect_err(|e| log::debug!("flyouts: icon {path}: {e:#}"))
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    }
}

/// The providers' lanes, started on first use, and the endpoint volume while it is shown.
pub struct Lanes {
    pub waker: Waker,
    replies: Sender<Reply>,
    icons: PathBuf,
    quick: Option<Lane<QuickLane>>,
    mixer: Option<Lane<MixerLane>>,
    wifi: Option<Lane<WifiLane>>,
    apps: Option<Lane<Builder>>,
    clipboard: Option<Lane<mypaper_bars::flyouts::clipboard::ClipboardLane>>,
    /// The default output's volume (quick settings and the mixer), with change notifications.
    pub audio: Option<Audio>,
}

/// Wakes the UI loop after a lane queued a reply (the message itself is ignored).
const POKE: Msg = Msg::Flyout(Done::Changed(Watched::Core(Topic::Apps)));

impl Lanes {
    fn run<S: 'static>(
        slot: &mut Option<Lane<S>>,
        name: &str,
        init: impl FnOnce() -> S + Send + 'static,
        tx: &Sender<Reply>,
        waker: &Waker,
        job: impl FnOnce(&mut S) -> Reply + Send + 'static,
    ) {
        let (tx, waker) = (tx.clone(), waker.clone());
        providers::on_lane(slot, name, init, move |state| {
            if tx.send(job(state)).is_ok() {
                waker.send(POKE);
            }
        });
    }

    pub fn quick(&mut self, job: impl FnOnce(&mut QuickLane) -> Reply + Send + 'static) {
        Self::run(
            &mut self.quick,
            "quick",
            QuickLane::new,
            &self.replies,
            &self.waker,
            job,
        );
    }

    pub fn mixer(&mut self, job: impl FnOnce(&mut MixerLane) -> Reply + Send + 'static) {
        let (waker, icons) = (self.waker.clone(), self.icons.clone());
        let init = move || MixerLane::new(waker, icons);
        Self::run(
            &mut self.mixer,
            "mixer",
            init,
            &self.replies,
            &self.waker,
            job,
        );
    }

    pub fn wifi(&mut self, job: impl FnOnce(&mut WifiLane) -> Reply + Send + 'static) {
        let waker = self.waker.clone();
        let init = move || WifiLane::new(waker);
        Self::run(
            &mut self.wifi,
            "wifi",
            init,
            &self.replies,
            &self.waker,
            job,
        );
    }

    pub fn apps(&mut self, job: impl FnOnce(&mut Builder) -> Reply + Send + 'static) {
        let icons = self.icons.clone();
        let init = move || Builder::new(icons);
        Self::run(
            &mut self.apps,
            "apps",
            init,
            &self.replies,
            &self.waker,
            job,
        );
    }

    /// Resolve toast logos on the existing STA icon lane, without another toast subscription.
    pub fn notification_icons(&mut self, ids: Vec<String>) {
        let dir = self.icons.clone();
        self.apps(move |_| {
            let resolver = mypaper_bars::icons::AppIconResolver::new();
            Reply::NotificationIcons(
                ids.into_iter()
                    .map(|id| {
                        let path = resolver.icon(&id, &dir);
                        (id, path)
                    })
                    .collect(),
            )
        });
    }

    /// The default output's volume, watched while a flyout shows it.
    pub fn clipboard(
        &mut self,
        job: impl FnOnce(&mut mypaper_bars::flyouts::clipboard::ClipboardLane) -> Reply + Send + 'static,
    ) {
        Self::run(
            &mut self.clipboard,
            "clipboard",
            mypaper_bars::flyouts::clipboard::ClipboardLane::new,
            &self.replies,
            &self.waker,
            job,
        );
    }

    pub fn audio(&mut self) -> Option<&Audio> {
        if self.audio.is_none() {
            self.audio = Audio::new(self.waker.clone());
        }
        self.audio.as_ref()
    }

    /// Ends every lane and the volume watch (a few seconds after the flyout closed).
    fn release(&mut self) {
        self.quick = None;
        self.mixer = None;
        self.wifi = None;
        self.apps = None;
        self.clipboard = None;
        self.audio = None;
    }
}

/// How long lanes outlive their flyout (reopening within it reuses their state).
const LINGER: Duration = Duration::from_secs(5);
/// On glass: how long the content waits for the engine's glass (its usual "open to glass"),
/// and the engine's fade-in, which the content's fade follows (wallpaper `overlay::window::FADE`).
const GLASS_DELAY: Duration = Duration::from_millis(90);

/// The open flyout.
struct Open {
    kind: Kind,
    panel: Box<dyn Panel>,
    anchor: Rect,
    monitor: Monitor,
    /// Device pixels per design pixel.
    scale: f32,
    /// Where it rests, and its size.
    origin: (i32, i32),
    size: (u32, u32),
    /// Whether it sits below its anchor (slides down into place) or above (slides up).
    below: bool,
    /// Open progress, 0 to 1, while it moves.
    motion: Option<mypaper_ui::transition::Transition>,
    motion_style: mypaper_bars::style::MotionStyle,
    snapshot: Option<mypaper_ui::reveal::Snapshot>,
    interaction: mypaper_ui::interact::motion::InteractionMotion,
    /// When its motion started (the fade follows the glass from there).
    opened_at: Instant,
    hover: Option<HitId>,
    pressed: Option<HitId>,
    focus: Option<HitId>,
    /// The slider being dragged.
    drag: Option<HitId>,
    swipe: Option<(HitId, mypaper_ui::Point)>,
    /// The hit being activated, in screen px (for `Cx::hit`).
    hit: Rect,
    /// Shown yet, and how often it was drawn: in all, and once its motion had settled.
    shown: bool,
    draws: u32,
    settled_draws: u32,
    /// When the panel asked to be ticked.
    due: Option<Instant>,
    dirty: bool,
    return_to: isize,
    /// An app's menu opened from it: deactivation doesn't close it (see `Effect::HoldForMenu`).
    held: bool,
}

/// The flyouts: one reused popup window and the panel shown in it.
pub struct Flyouts {
    surface: Surface,
    open: Option<Open>,
    look: Look,
    /// `bars::plugin_style::generation` when `look` was read: a newer one reads it again.
    style_generation: u64,
    lanes: Lanes,
    images: Images,
    ticker: Ticker,
    rx: Receiver<Msg>,
    replies: Receiver<Reply>,
    release_due: Option<Instant>,
    winkey: Option<WinKey>,
    /// The foreground window last seen, to follow fullscreen apps for the Windows key.
    foreground: HWND,
    menu: Option<mypaper_ui::Menu<PanelCommand>>,
    index_due: Option<Instant>,
    _index_watch: Option<notify::RecommendedWatcher>,
    /// The flyout a deactivation just closed, and when: a click on its own bar module
    /// deactivates it before that click's request arrives, and the request must close it (it
    /// did), not open it again.
    dismissed: Option<(Kind, Instant)>,
}

/// How soon after a deactivation a request for the same flyout counts as that click's close.
const DISMISSED: Duration = Duration::from_millis(400);

impl Flyouts {
    /// Creates the (hidden) popup window now: making a layered window costs ~20 ms, which must
    /// not land on a click.
    pub fn new(ui: &Ui, main: HWND) -> Result<Self> {
        mypaper_bars::wake::make_queue();
        let (tx, rx) = mpsc::channel();
        // SAFETY: no preconditions; the waker posts to this (the UI) thread.
        let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
        let waker = Waker::new(tx, thread);
        let (replies_tx, replies) = mpsc::channel();
        let look = Look::load(&ui.text);
        look.install();
        let mut flyouts = Self {
            surface: Surface::new_deferred_accessibility(
                SurfaceKind::Popup { activate: true },
                "Flyout",
                Role::Dialog,
            )?,
            open: None,
            lanes: Lanes {
                waker: waker.clone(),
                replies: replies_tx,
                icons: mypaper_bars::files::local("icons")?,
                quick: None,
                mixer: None,
                wifi: None,
                apps: None,
                clipboard: None,
                audio: None,
            },
            look,
            style_generation: crate::bars::plugin_style::generation(),
            images: Images::default(),
            ticker: Ticker::new()?,
            rx,
            replies,
            release_due: None,
            winkey: None,
            foreground: HWND::default(),
            menu: Some(mypaper_ui::Menu::new("App commands")?),
            index_due: Some(Instant::now() + Duration::from_secs(8)),
            _index_watch: None,
            dismissed: None,
        };
        flyouts.follow_win_key(main);
        use notify::Watcher;
        let waker = flyouts.lanes.waker.clone();
        if let Ok(mut watcher) =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
                    waker.send(Msg::Flyout(Done::Changed(Watched::Apps)));
                }
            })
        {
            for dir in providers::index::start_menu_dirs() {
                let _ = watcher.watch(&dir, notify::RecursiveMode::Recursive);
            }
            flyouts._index_watch = Some(watcher);
        }
        Ok(flyouts)
    }

    /// The ticker's handle, for the loop's wait.
    pub fn handle(&self) -> windows::Win32::Foundation::HANDLE {
        self.ticker.handle()
    }

    /// The next deadline (a panel's tick, the lanes' release), for the loop's wait.
    pub fn deadline(&self) -> Option<Instant> {
        [
            self.open.as_ref().and_then(|o| o.due),
            self.release_due,
            self.index_due,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// [`deadline`](Self::deadline) as the loop's wait timeout in ms.
    pub fn timeout_ms(&self) -> u32 {
        self.deadline().map_or(u32::MAX, |at| {
            at.saturating_duration_since(Instant::now())
                .as_nanos()
                .div_ceil(1_000_000)
                .min(u128::from(u32::MAX - 1)) as u32
        })
    }

    /// `bars.json` or the theme changed: the look is read again (and the Windows key).
    pub fn config_changed(&mut self, ui: &Ui, main: HWND) {
        self.look = Look::load(&ui.text);
        self.look.install();
        self.follow_win_key(main);
        if let Some(open) = &mut self.open {
            open.scale = open.monitor.dpi.max(96) as f32 / 96.0 * self.look.scale;
            open.motion_style = self.look.opening_style;
            open.interaction = mypaper_ui::interact::motion::InteractionMotion::new(
                self.look.hover_motion,
                self.look.press_motion,
            );
            if !self.look.opening.enabled {
                open.motion = None;
                open.snapshot = None;
            }
            open.dirty = true;
        }
    }

    fn follow_win_key(&mut self, main: HWND) {
        let hidden = mypaper_bars::config::path()
            .and_then(|p| mypaper_bars::BarsConfig::load(&p))
            .is_ok_and(|c| c.taskbar.mode == mypaper_bars::TaskbarMode::Hide);
        let wanted = self.look.win_key && hidden;
        if wanted != self.winkey.is_some() {
            self.winkey = wanted
                .then(|| WinKey::start(self.lanes.waker.clone(), main))
                .flatten();
        }
        // The hook lets every key through until it is enabled.
        WinKey::set_enabled(self.winkey.is_some());
    }

    /// Whether `id` is the flyout's window.
    pub fn owns(&self, id: SurfaceId) -> bool {
        self.surface.id() == id || self.menu.as_ref().is_some_and(|m| m.owns(id))
    }

    /// Opens queued requests, takes provider results and worker news, ticks the panel and
    /// moves it. Call after each pass over the input.
    pub fn process(&mut self, ui: &mut Ui, bars: &mut Bars) {
        let requests = REQUESTS.with_borrow_mut(std::mem::take);
        for request in requests {
            self.toggle(ui, bars, request);
        }
        self.follow_fullscreen();
        self.follow_held();
        WinKey::set_enabled(self.winkey.is_some() && bars.can_open_flyouts());
        if !bars.can_open_flyouts() && self.open.is_some() {
            self.close();
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Flyout(Done::Changed(Watched::Apps)) => {
                    self.index_due = Some(Instant::now() + Duration::from_secs(3))
                }
                Msg::Flyout(Done::WinKey(at, target)) => {
                    if at.elapsed() <= Duration::from_millis(500) && bars.can_open_flyouts() {
                        self.win_key(ui, bars, target);
                    } else {
                        match target {
                            winkey::Target::Launcher => mypaper_bars::system::start_menu(),
                            winkey::Target::Quick => mypaper_bars::system::action_center(
                                mypaper_bars::actions::Panel::Quick,
                            ),
                            winkey::Target::Notifications => mypaper_bars::system::action_center(
                                mypaper_bars::actions::Panel::Notifications,
                            ),
                        }
                    }
                }
                Msg::Flyout(Done::Changed(Watched::Core(Topic::Apps))) => {}
                msg => self.with_panel(ui, bars, |panel, cx| {
                    if panel.changed(&msg, cx) {
                        Effect::Redraw
                    } else {
                        Effect::None
                    }
                }),
            }
        }
        while let Ok(reply) = self.replies.try_recv() {
            if let Reply::Apps(apps) = &reply {
                if let Ok(path) = mypaper_bars::files::local("apps.json") {
                    let _ = mypaper_bars::files::write_atomic(
                        &path,
                        providers::index::cache_json(apps).as_bytes(),
                    );
                }
                if self.open.is_none() {
                    self.release_due = Some(Instant::now() + LINGER);
                }
            }
            self.with_panel(ui, bars, |panel, cx| {
                if panel.reply(&reply, cx) {
                    Effect::Redraw
                } else {
                    Effect::None
                }
            });
        }
        self.with_panel(ui, bars, |panel, cx| {
            if panel.refresh(cx) {
                Effect::Redraw
            } else {
                Effect::None
            }
        });
        let now = Instant::now();
        if self.index_due.is_some_and(|at| at <= now) {
            self.index_due = None;
            self.lanes.apps(|builder| Reply::Apps(builder.build()));
        }
        if self
            .open
            .as_ref()
            .and_then(|o| o.due)
            .is_some_and(|d| d <= now)
        {
            self.schedule_tick(ui, bars, now);
        }
        if self.release_due.is_some_and(|d| d <= now) {
            self.release_due = None;
            if self.open.is_none() {
                self.lanes.release();
                log::debug!("flyouts: providers released");
            }
        }
        if let Some(open) = &mut self.open {
            let input = mypaper_ui::Interaction {
                hover: open.hover,
                pressed: open.pressed,
                focus: open.focus,
                focus_visible: open.focus.is_some(),
            };
            open.dirty |= open.interaction.step(&input, now);
        }
        if self.open.as_ref().is_some_and(|o| o.dirty) {
            self.render(ui);
        }
        self.animate(ui, now);
    }

    /// Asks the open panel when it next wants a tick.
    fn schedule_tick(&mut self, ui: &mut Ui, bars: &mut Bars, now: Instant) {
        let Some(open) = &mut self.open else { return };
        let mut cx = Cx {
            lanes: &mut self.lanes,
            bars,
            images: &mut self.images,
            ui,
            look: &self.look,
            hit: open.anchor,
            scale: open.scale,
        };
        // A tick at `now` that changes nothing just reports the next deadline.
        let (changed, due) = open.panel.tick(now, &mut cx);
        open.dirty |= changed;
        open.due = due;
    }

    /// Runs `f` on the open panel and applies what it came to.
    fn with_panel(
        &mut self,
        ui: &mut Ui,
        bars: &mut Bars,
        f: impl FnOnce(&mut dyn Panel, &mut Cx) -> Effect,
    ) {
        let Some(open) = &mut self.open else { return };
        let mut cx = Cx {
            lanes: &mut self.lanes,
            bars,
            images: &mut self.images,
            ui,
            look: &self.look,
            hit: open.hit,
            scale: open.scale,
        };
        let effect = f(open.panel.as_mut(), &mut cx);
        self.apply(ui, bars, effect);
    }

    fn apply(&mut self, ui: &mut Ui, bars: &mut Bars, effect: Effect) {
        match effect {
            Effect::Menu => {
                if let Some(open) = &mut self.open {
                    let entries = open.panel.menu();
                    let point = (open.hit.left as i32, open.hit.bottom as i32);
                    if self.menu.is_none() {
                        self.menu = mypaper_ui::Menu::new("App commands").ok();
                    }
                    if let Some(menu) = &mut self.menu {
                        let _ = menu.open(
                            ui,
                            entries,
                            point,
                            mypaper_ui::screen_at(point.0, point.1),
                            &mypaper_ui::Tokens::default(),
                        );
                    }
                }
            }
            Effect::None => {}
            Effect::HoldForMenu => {
                if let Some(open) = &mut self.open {
                    open.held = true;
                }
            }
            Effect::Redraw => {
                if let Some(open) = &mut self.open {
                    open.dirty = true;
                }
            }
            Effect::Close => self.close(),
            Effect::Switch(kind) => {
                if let Some(open) = &self.open {
                    let request = Request {
                        kind,
                        anchor: open.anchor,
                        monitor: open.monitor.clone(),
                        at: Instant::now(),
                    };
                    self.show(ui, bars, request);
                }
            }
        }
    }

    fn toggle(&mut self, ui: &mut Ui, bars: &mut Bars, request: Request) {
        if self.open.as_ref().is_some_and(|o| o.kind == request.kind) {
            self.close();
            return;
        }
        // The click on its module already closed it (by deactivating it): that was the toggle.
        if self
            .dismissed
            .take()
            .is_some_and(|(kind, at)| kind == request.kind && at.elapsed() < DISMISSED)
        {
            return;
        }
        self.show(ui, bars, request);
    }

    /// Win, Win+N, Win+A: the launcher, notifications or quick settings, anchored where Windows
    /// puts them (the top bar's right end). The launcher opens on the monitor under the mouse;
    /// the others on the monitor of the app in front.
    fn win_key(&mut self, ui: &mut Ui, bars: &mut Bars, target: winkey::Target) {
        let kind = match target {
            winkey::Target::Launcher => Kind::Launcher,
            winkey::Target::Notifications => Kind::Notifications,
            winkey::Target::Quick => Kind::Quick,
        };
        let (found, center) = if kind == Kind::Launcher {
            let mut cursor = windows::Win32::Foundation::POINT::default();
            // SAFETY: a read-only query into a local.
            let found = unsafe { GetCursorPos(&mut cursor) }.is_ok();
            (found, (cursor.x, cursor.y))
        } else {
            let mut front = windows::Win32::Foundation::RECT::default();
            // SAFETY: read-only queries; `front` outlives the call.
            let found = unsafe { GetWindowRect(GetForegroundWindow(), &mut front) }.is_ok();
            let center = (
                (front.left + front.right) / 2,
                (front.top + front.bottom) / 2,
            );
            (found, center)
        };
        let monitors = mypaper_desktop::monitor_infos();
        let monitor = monitors
            .iter()
            .find(|m| found && m.rect.contains_point(center.0, center.1))
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or_else(|| monitors.first())
            .cloned();
        let Some(monitor) = monitor else { return };
        let work = monitor.work;
        let anchor = Rect::new(
            (work.x + work.width) as f32 - 1.0,
            work.y as f32 - 1.0,
            1.0,
            1.0,
        );
        self.toggle(
            ui,
            bars,
            Request {
                kind,
                anchor,
                monitor,
                at: Instant::now(),
            },
        );
    }

    /// A flyout held for an app's menu (`Effect::HoldForMenu`) stays while that menu, or the
    /// app's hidden window that owns it, is in front; it closes once a visible window that isn't
    /// ours is (a click elsewhere, or a window the menu's command opened). Back in front, it
    /// closes on deactivation again. Runs each loop pass; foreground changes wake the loop.
    fn follow_held(&mut self) {
        let Some(open) = self.open.as_mut().filter(|o| o.held) else {
            return;
        };
        // SAFETY: read-only window queries; a stale handle reads as nothing.
        let (front, menu, visible) = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, IsWindowVisible};
            let front = GetForegroundWindow();
            let mut class = [0u16; 16];
            let len = GetClassNameW(front, &mut class).max(0) as usize;
            let menu = class[..len].iter().copied().eq("#32768".encode_utf16());
            (front, menu, IsWindowVisible(front).as_bool())
        };
        if front == self.surface.hwnd() {
            open.held = false;
        } else if !menu && visible && !front.is_invalid() {
            self.close();
        }
    }

    /// The Windows key goes to Windows while a fullscreen app (a game) is in front.
    fn follow_fullscreen(&mut self) {
        if self.winkey.is_none() {
            return;
        }
        // SAFETY: a read-only query.
        let front = unsafe { GetForegroundWindow() };
        if front != self.foreground {
            self.foreground = front;
            WinKey::set_fullscreen(mypaper_desktop::fullscreen_app_in_front());
        }
    }

    /// Opens `request`'s panel: drawn, placed and shown at once; its data follows.
    fn show(&mut self, ui: &mut Ui, bars: &mut Bars, request: Request) {
        let started = Instant::now();
        // A plugin restyled the `panel` surface since the look was read.
        let generation = crate::bars::plugin_style::generation();
        if std::mem::replace(&mut self.style_generation, generation) != generation {
            self.look = Look::load(&ui.text);
            self.look.install();
        }
        let return_to = bars.external_foreground();
        self.hide();
        self.release_due = None;
        let scale = request.monitor.dpi.max(96) as f32 / 96.0 * self.look.scale;
        let mut cx = Cx {
            lanes: &mut self.lanes,
            bars,
            images: &mut self.images,
            ui,
            look: &self.look,
            hit: request.anchor,
            scale,
        };
        let panel: Box<dyn Panel> = match request.kind {
            Kind::Integration(provider) => crate::integrations::panel(provider),
            Kind::AppActions => crate::integrations::app_panel(),
            Kind::Calendar => Box::new(calendar::Calendar::new(&mut cx)),
            Kind::Quick => Box::new(quick::Quick::new(&mut cx)),
            Kind::Wifi => Box::new(wifi::Wifi::new(&mut cx)),
            Kind::Mixer => Box::new(mixer::Mixer::new(&mut cx)),
            Kind::Notifications => crate::integrations::notification_center(&mut cx),
            Kind::Power => Box::new(power::Power::new(&mut cx)),
            Kind::TrayOverflow => Box::new(tray::Overflow::new(&mut cx)),
            Kind::Language => Box::new(language::Language::new(&mut cx)),
            Kind::Launcher => Box::new(launcher::Launcher::new(&mut cx, self.surface.hwnd())),
            Kind::Plugin(number) => match crate::bars::plugin_modules::panel_name(number) {
                Some(id) => Box::new(plugin_panel::PluginPanel::new(id)),
                None => return,
            },
        };
        let center_y = request.anchor.top + request.anchor.height() / 2.0;
        let monitor_mid = request.monitor.rect.y as f32 + request.monitor.rect.height as f32 / 2.0;
        self.open = Some(Open {
            kind: request.kind,
            panel,
            anchor: request.anchor,
            monitor: request.monitor,
            scale,
            origin: (0, 0),
            size: (0, 0),
            below: center_y < monitor_mid,
            motion: None,
            motion_style: self.look.opening_style,
            snapshot: None,
            interaction: mypaper_ui::interact::motion::InteractionMotion::new(
                self.look.hover_motion,
                self.look.press_motion,
            ),
            opened_at: Instant::now(),
            hover: None,
            pressed: None,
            focus: None,
            drag: None,
            swipe: None,
            hit: request.anchor,
            shown: false,
            draws: 0,
            settled_draws: 0,
            due: None,
            dirty: true,
            return_to,
            held: false,
        });
        if let Some(field) = self.open.as_ref().and_then(|o| o.panel.field()) {
            self.surface.set_text_field(Some(field.clone()));
        }
        let made = started.elapsed();
        self.render(ui);
        let drawn = started.elapsed();
        let Some(open) = &mut self.open else { return };
        // Shown at its first frame of motion, then focused (keys, and a click elsewhere closes).
        let delay = if mypaper_ui::glass::style().is_some() {
            GLASS_DELAY
        } else {
            Duration::ZERO
        };
        open.motion = self
            .look
            .opening
            .enabled
            .then(|| self.look.opening.start(0.0, 1.0, Instant::now() + delay));
        if open.motion.is_some() && crate::bars::motion::reveal(open.motion_style).scales() {
            open.snapshot =
                mypaper_ui::reveal::Snapshot::new(ui, self.surface.shown().clone(), open.size).ok();
        }
        open.opened_at = Instant::now();
        self.animate(ui, Instant::now());
        self.surface.show();
        let shown = started.elapsed();
        if let Some(open) = &mut self.open {
            open.shown = true;
        }
        let how = self.surface.take_focus();
        if let Some(field) = self.open.as_ref().and_then(|o| o.panel.field()) {
            field.focus();
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        log::info!(
            "flyout {:?}: on screen in {:.1} ms (made {:.1}, drawn {:.1}), focused at {:.1} \
             ({how}); asked for {:.1} ms before",
            request.kind,
            ms(shown),
            ms(made),
            ms(drawn),
            ms(started.elapsed()),
            ms(started.duration_since(request.at)),
        );
        // What can wait a frame (the volume endpoint, keyboard layouts, icons) comes now.
        self.with_panel(ui, bars, |panel, cx| {
            if panel.opened(cx) {
                Effect::Redraw
            } else {
                Effect::None
            }
        });
        if let Some(field) = self.open.as_ref().and_then(|o| o.panel.field()) {
            self.surface.set_text_field(Some(field.clone()));
            field.focus();
        }
        self.schedule_tick(ui, bars, Instant::now());
    }

    /// Closes the open flyout; its providers linger a few seconds for a quick reopen.
    pub fn close(&mut self) {
        if self.open.is_some() {
            // Hand focus back only when we still own it; outside clicks keep their own target.
            // SAFETY: read-only foreground query.
            let restore = (unsafe { GetForegroundWindow() } == self.surface.hwnd())
                .then(|| self.open.as_ref().unwrap().return_to)
                .filter(|w| *w != 0 && on_screen(*w));
            self.hide();
            if let Some(window) = restore {
                mypaper_bars::system::activate(mypaper_bars::system::hwnd(window));
            }
            self.release_due = Some(Instant::now() + LINGER);
        }
    }

    fn hide(&mut self) {
        if let Some(menu) = &mut self.menu {
            menu.close();
        }
        let Some(open) = self.open.take() else { return };
        log::info!(
            "flyout {:?} closed: drawn {} times, {} after its motion settled",
            open.kind,
            open.draws,
            open.settled_draws
        );
        if open.drag.is_some() {
            // SAFETY: releasing our own capture.
            let _ = unsafe { ReleaseCapture() };
        }
        self.surface.set_text_field(None);
        self.surface.hide();
        self.surface.set_glass(None);
        self.surface.release();
        self.surface.discard_events();
        self.images = Images::default();
        self.ticker.schedule(false);
        drop(open);
    }

    /// Draws the open panel again (only what changed reaches the screen).
    fn render(&mut self, ui: &mut Ui) {
        let Some(open) = &mut self.open else { return };
        open.dirty = false;
        let s = open.scale;
        let max_height = (open.monitor.work.height as f32 - 2.0 * GAP * s).max(120.0 * s);
        let glass = mypaper_ui::glass::style();
        let state = (open.hover, open.pressed, open.focus);
        // On glass, never shrink while open (the glass would be rebuilt: a flash); the next
        // open takes the new size.
        let keep = if glass.is_some() {
            open.size.1 as f32
        } else {
            0.0
        };
        let (list, size) = build_animated(
            open.panel.as_ref(),
            &mut ui.text,
            &self.look,
            s,
            max_height,
            state,
            glass.is_some(),
            keep,
            Some(&open.interaction),
        );
        let (width, height) = (size.0 as f32, size.1 as f32);
        // The body's radius (`Look::theme`): the glass has exactly the body's shape.
        let radius = (self.look.radius * s).round();
        open.size = size;
        open.origin = place(
            open.kind,
            (size.0 as i32, size.1 as i32),
            open.anchor,
            &open.monitor,
            s,
            open.below,
        );
        // The look's blur is in DIPs; the engine takes physical px, as the bars' and widgets'.
        self.surface.set_glass(glass.map(|material| Glass {
            body: Rect::new(0.0, 0.0, width, height),
            radius,
            material: mypaper_ui::GlassMaterial {
                blur: material.blur * s,
                ..material
            },
        }));
        let at = position(open, Instant::now());
        match self.surface.present(ui, at, size, list) {
            Ok(drew) => {
                if open.snapshot.is_some() {
                    open.snapshot =
                        mypaper_ui::reveal::Snapshot::new(ui, self.surface.shown().clone(), size)
                            .ok();
                }
                open.draws += u32::from(drew);
                open.settled_draws += u32::from(drew && open.shown && open.motion.is_none());
            }
            Err(e) => log::warn!("flyout: drawing: {e:#}"),
        }
    }

    /// One frame of the open motion (position and alpha only), or nothing at rest.
    fn animate(&mut self, ui: &mut Ui, now: Instant) {
        let Some(open) = &mut self.open else {
            self.ticker.schedule(false);
            return;
        };
        let Some(motion) = open.motion else {
            let _ = self.surface.set_alpha(255);
            let _ = self.surface.move_to(open.origin);
            self.ticker.schedule(open.interaction.moving());
            return;
        };
        let p = motion.value(now);
        if let Some(snapshot) = &open.snapshot {
            let full = Rect::new(0.0, 0.0, open.size.0 as f32, open.size.1 as f32);
            let anchor = open
                .anchor
                .offset(-open.origin.0 as f32, -open.origin.1 as f32);
            let body = crate::bars::motion::reveal(open.motion_style).body(full, anchor, p);
            let list = if motion.done(now) {
                snapshot.original.clone()
            } else {
                snapshot.frame(full, body)
            };
            let _ = self.surface.present(ui, open.origin, open.size, list);
            self.surface.set_glass(
                self.look
                    .glass
                    .filter(|_| mypaper_ui::glass::available())
                    .map(|material| Glass {
                        body,
                        radius: self.look.radius * open.scale * body.width() / full.width(),
                        material: mypaper_ui::GlassMaterial {
                            blur: material.blur * open.scale,
                            ..material
                        },
                    }),
            );
        }
        // Delay for the overlay's first frame, then use the configured curve for the content.
        let alpha = if mypaper_ui::glass::style().is_some() {
            p.clamp(0.0, 1.0)
        } else {
            (0.45 + p * 0.55).clamp(0.0, 1.0)
        };
        if motion.done(now) {
            open.motion = None;
            open.snapshot = None;
        }
        let at = position(open, now);
        let _ = self.surface.set_alpha((alpha * 255.0).round() as u8);
        let _ = self.surface.move_to(at);
        self.ticker
            .schedule(open.motion.is_some() || open.interaction.moving());
    }

    /// Input for the flyout's window.
    pub fn event(&mut self, ui: &mut Ui, bars: &mut Bars, id: SurfaceId, event: &Event) {
        if let Some(menu) = &mut self.menu
            && menu.owns(id)
        {
            if let Some(outcome) = menu.handle(ui, id, event) {
                if let mypaper_ui::Outcome::Chosen(command) = outcome {
                    self.with_panel(ui, bars, |panel, cx| panel.command(command, cx));
                }
                self.surface.take_focus();
            }
            return;
        }
        if !self.owns(id) || self.open.is_none() {
            return;
        }
        match *event {
            Event::Move(p) => {
                let hit = self.surface.shown().hit_at(p.at).filter(|h| h.enabled);
                let id = hit.map(|h| h.id);
                let drag = self.open.as_ref().and_then(|o| o.drag);
                if let Some(drag) = drag {
                    if let Some(value) = self.slider_value(drag, p.at.x) {
                        self.with_panel(ui, bars, |panel, cx| panel.slide(drag, value, false, cx));
                    }
                } else if let Some(open) = &mut self.open
                    && open.hover != id
                {
                    open.hover = id;
                    open.dirty = true;
                }
            }
            Event::Leave => {
                if let Some(open) = &mut self.open
                    && open.drag.is_none()
                    && (open.hover.is_some() || open.pressed.is_some())
                {
                    open.hover = None;
                    open.pressed = None;
                    open.dirty = true;
                }
            }
            Event::Press(p, Button::Left) => {
                let hit = self
                    .surface
                    .shown()
                    .hit_at(p.at)
                    .filter(|h| h.enabled)
                    .cloned();
                let Some(hit) = hit else { return };
                if let Some(open) = &mut self.open {
                    open.pressed = Some(hit.id);
                    open.swipe = Some((hit.id, p.at));
                    open.focus = None;
                    open.dirty = true;
                }
                if hit.role == Role::Slider {
                    // SAFETY: our own window, released on release or capture loss.
                    unsafe { SetCapture(self.surface.hwnd()) };
                    if let Some(open) = &mut self.open {
                        open.drag = Some(hit.id);
                    }
                    if let Some(value) = self.slider_value(hit.id, p.at.x) {
                        self.with_panel(ui, bars, |panel, cx| {
                            panel.slide(hit.id, value, false, cx)
                        });
                    }
                } else if hit.role == Role::Button {
                    // SAFETY: keeps a horizontal swipe's release even beyond the card/window.
                    unsafe { SetCapture(self.surface.hwnd()) };
                }
            }
            Event::Release(p, Button::Left) => {
                let Some(open) = &mut self.open else { return };
                let pressed = open.pressed.take();
                let swipe = open.swipe.take();
                // SAFETY: this thread owns any capture started by a card or slider.
                let _ = unsafe { ReleaseCapture() };
                open.dirty = true;
                if let Some(drag) = open.drag.take() {
                    // SAFETY: releasing our own capture.
                    let _ = unsafe { ReleaseCapture() };
                    if let Some(value) = self.slider_value(drag, p.at.x) {
                        self.with_panel(ui, bars, |panel, cx| panel.slide(drag, value, true, cx));
                    }
                    return;
                }
                if let Some((id, start)) = swipe.filter(|(_, start)| {
                    (p.at.x - start.x).abs() > 70.0 * open.scale
                        && (p.at.y - start.y).abs() < 50.0 * open.scale
                }) {
                    let dx = p.at.x - start.x;
                    self.with_panel(ui, bars, |panel, cx| panel.swipe(id, dx, cx));
                    return;
                }
                let hit = self
                    .surface
                    .shown()
                    .hit_at(p.at)
                    .filter(|h| h.enabled)
                    .map(|h| h.id);
                if let Some(id) = hit.filter(|&h| Some(h) == pressed) {
                    self.target(id);
                    self.with_panel(ui, bars, |panel, cx| panel.activate(id, cx));
                }
            }
            Event::Release(p, Button::Right) => {
                let hit = self
                    .surface
                    .shown()
                    .hit_at(p.at)
                    .filter(|h| h.enabled)
                    .map(|h| h.id);
                if let Some(id) = hit {
                    self.target(id);
                    self.with_panel(ui, bars, |panel, cx| panel.context(id, cx));
                }
            }
            Event::Release(p, Button::Middle) | Event::DoubleClick(p, Button::Left) => {
                if matches!(event, Event::DoubleClick(..))
                    && let Some(open) = &mut self.open
                {
                    open.pressed = None;
                }
                if let Some(hit) = self
                    .surface
                    .shown()
                    .hit_at(p.at)
                    .filter(|h| h.enabled)
                    .map(|h| h.id)
                {
                    self.target(hit);
                    self.with_panel(ui, bars, |panel, cx| {
                        if matches!(event, Event::DoubleClick(..)) {
                            panel.double(hit, cx)
                        } else {
                            panel.middle(hit, cx)
                        }
                    });
                }
            }
            Event::CaptureLost => {
                if let Some(open) = &mut self.open
                    && open.drag.take().is_some()
                {
                    open.pressed = None;
                    open.dirty = true;
                }
            }
            Event::Wheel(p, delta) => {
                let hit = self
                    .surface
                    .shown()
                    .hit_at(p.at)
                    .map(|h| (h.id, h.role, h.rect));
                if let Some((id, Role::Slider, _)) = hit {
                    let step = if delta > 0 { 0.04 } else { -0.04 };
                    if let Some(value) = self.slider_current(id).map(|v| v + step) {
                        self.with_panel(ui, bars, |panel, cx| {
                            panel.slide(id, value.clamp(0.0, 1.0), true, cx)
                        });
                    }
                } else {
                    self.with_panel(ui, bars, |panel, cx| {
                        panel.wheel(hit.map(|h| h.0), delta, cx)
                    });
                }
            }
            Event::Key(key) => self.key(ui, bars, key),
            Event::Invoke(hit) => {
                self.target(hit);
                self.with_panel(ui, bars, |panel, cx| panel.activate(hit, cx));
            }
            Event::Focus(hit) => {
                if let Some(open) = &mut self.open {
                    open.focus = Some(hit);
                    open.dirty = true;
                }
            }
            Event::TextChanged => {
                if let Some(open) = &mut self.open {
                    open.focus = None;
                }
                self.with_panel(ui, bars, |panel, cx| panel.text_changed(cx));
            }
            Event::TextSubmit => {
                if let Some(hit) = self.open.as_ref().and_then(|o| o.focus) {
                    self.target(hit);
                    self.with_panel(ui, bars, |panel, cx| panel.activate(hit, cx));
                } else {
                    self.with_panel(ui, bars, |panel, cx| panel.submit(cx));
                }
            }
            // Held for an app's menu: `follow_held` closes it once something else is in front.
            Event::Deactivated if self.open.as_ref().is_some_and(|o| o.held) => {}
            Event::Deactivated if !self.menu.as_ref().is_some_and(mypaper_ui::Menu::is_open) => {
                let kind = self.open.as_ref().map(|o| o.kind);
                self.close();
                self.dismissed = kind.map(|kind| (kind, Instant::now()));
            }
            Event::DpiChanged(_) => self.close(),
            _ => {}
        }
    }

    fn key(&mut self, ui: &mut Ui, bars: &mut Bars, key: u16) {
        let mut handled = None;
        self.with_panel(ui, bars, |panel, cx| {
            handled = panel.key(key, cx);
            handled.unwrap_or(Effect::None)
        });
        if handled.is_some() {
            if let Some(open) = &mut self.open {
                open.focus = None;
            }
            return;
        }
        let Some(open) = &mut self.open else { return };
        let hits = &self.surface.shown().hits;
        let focused = open
            .focus
            .and_then(|id| hits.iter().find(|h| h.id == id))
            .map(|h| (h.id, h.role));
        match key {
            k if k == VK_ESCAPE.0 => self.close(),
            k if k == VK_TAB.0
                || k == VK_DOWN.0
                || k == VK_UP.0
                || k == VK_NEXT.0
                || k == VK_PRIOR.0 =>
            {
                // SAFETY: a read-only query of this thread's keyboard state.
                let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
                let back = k == VK_UP.0 || k == VK_PRIOR.0 || (k == VK_TAB.0 && shift);
                open.focus = focus::step(hits, open.focus, if back { -1 } else { 1 });
                open.dirty = true;
            }
            k if (k == VK_LEFT.0 || k == VK_RIGHT.0)
                && matches!(focused, Some((_, Role::Slider))) =>
            {
                let id = focused.map(|f| f.0).unwrap_or_default();
                let step = if k == VK_RIGHT.0 { 0.05 } else { -0.05 };
                if let Some(value) = self.slider_current(id).map(|v| v + step) {
                    self.with_panel(ui, bars, |panel, cx| {
                        panel.slide(id, value.clamp(0.0, 1.0), true, cx)
                    });
                }
            }
            k if k == VK_RETURN.0 || k == VK_SPACE.0 => {
                if let Some((id, _)) = focused {
                    self.target(id);
                    self.with_panel(ui, bars, |panel, cx| panel.activate(id, cx));
                }
            }
            _ => {}
        }
    }

    /// Where along slider `id`'s track `x` (surface px) is, 0..=1.
    fn slider_value(&self, id: HitId, x: f32) -> Option<f32> {
        let hit = self.surface.shown().hits.iter().find(|h| h.id == id)?;
        Some(((x - hit.rect.left) / hit.rect.width().max(1.0)).clamp(0.0, 1.0))
    }

    /// Slider `id`'s current value, from its accessible label ("Volume 40%").
    fn slider_current(&self, id: HitId) -> Option<f32> {
        let hit = self.surface.shown().hits.iter().find(|h| h.id == id)?;
        let percent = hit
            .label
            .rsplit(' ')
            .next()?
            .trim_end_matches('%')
            .parse::<f32>()
            .ok()?;
        Some(percent / 100.0)
    }

    /// Records hit `id`'s screen rectangle for the panel call that follows (`Cx::hit`).
    fn target(&mut self, id: HitId) {
        let origin = self.surface.origin();
        let rect = self
            .surface
            .shown()
            .hits
            .iter()
            .find(|h| h.id == id)
            .map(|h| h.rect);
        if let (Some(open), Some(rect)) = (&mut self.open, rect) {
            open.hit = rect.offset(origin.0 as f32, origin.1 as f32);
        }
    }
}

/// A panel's display list and window size at scale `s`, as the screen shows it: its content,
/// cut at `max_height`, over its body (glass under an invisible hit fill, or the style's fill).
/// `state` is the hovered, pressed and focused hits.
pub fn build(
    panel: &dyn Panel,
    text: &mut mypaper_ui::Text,
    look: &Look,
    s: f32,
    max_height: f32,
    state: (Option<HitId>, Option<HitId>, Option<HitId>),
    glass: bool,
) -> (mypaper_ui::DisplayList, (u32, u32)) {
    build_at_least(panel, text, look, s, max_height, state, glass, 0.0)
}

/// [`build`], never shorter than `min_height` (device px): an open flyout keeps its height when
/// its content shrinks (a notification dismissed), because a smaller window makes the engine
/// rebuild its glass at the new size, which shows as a flash.
#[allow(clippy::too_many_arguments)]
pub fn build_at_least(
    panel: &dyn Panel,
    text: &mut mypaper_ui::Text,
    look: &Look,
    s: f32,
    max_height: f32,
    state: (Option<HitId>, Option<HitId>, Option<HitId>),
    glass: bool,
    min_height: f32,
) -> (mypaper_ui::DisplayList, (u32, u32)) {
    build_animated(
        panel, text, look, s, max_height, state, glass, min_height, None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_animated(
    panel: &dyn Panel,
    text: &mut mypaper_ui::Text,
    look: &Look,
    s: f32,
    max_height: f32,
    state: (Option<HitId>, Option<HitId>, Option<HitId>),
    glass: bool,
    min_height: f32,
    motion: Option<&mypaper_ui::interact::motion::InteractionMotion>,
) -> (mypaper_ui::DisplayList, (u32, u32)) {
    let width = (panel.width() * s).round();
    let mut kit = Kit::new(text, look, s, width);
    kit.motion = motion;
    let pad = kit.pad();
    kit.limit = max_height - pad;
    (kit.hover, kit.pressed, kit.focus) = state;
    panel.view(&mut kit);
    let height = (kit.y + pad).round().max(min_height).min(max_height);
    let radius = (kit.theme.radius.surface * s).round();
    let Kit {
        mut list, theme, ..
    } = kit;
    list.focus = state.2;
    Kit::body(&mut list, (width, height), radius, &theme, glass);
    (list, (width as u32, height as u32))
}

/// The gap between a flyout and its bar, and its margin to the screen's edges (design px).
const GAP: f32 = 8.0;

/// Where a `size` flyout of `kind` rests for `anchor` on `monitor`: under (or over) the
/// clicked module, centred on it, kept inside the work area; notifications at the right edge,
/// the launcher centred high on the screen.
pub fn place(
    kind: Kind,
    size: (i32, i32),
    anchor: Rect,
    monitor: &Monitor,
    scale: f32,
    below: bool,
) -> (i32, i32) {
    let gap = (GAP * scale).round() as i32;
    let (w, h) = size;
    let work = monitor.work;
    // A margin from the sides; vertically the bar's own band (and the gap) already separate.
    let left = work.x + gap;
    let right = (work.x + work.width - gap - w).max(left);
    let top = work.y;
    let bottom = (work.y + work.height - h).max(top);
    let x = match kind {
        Kind::Notifications => right,
        // Centred on the work area (a side bar shifts it), like Windows 11's Start.
        Kind::Launcher => work.x + (work.width - w) / 2,
        _ => (anchor.left + anchor.width() / 2.0).round() as i32 - w / 2,
    };
    let y = match kind {
        Kind::Launcher => work.y + work.height / 5,
        _ if below => anchor.bottom.round() as i32 + gap,
        _ => anchor.top.round() as i32 - gap - h,
    };
    (x.clamp(left, right), y.clamp(top, bottom))
}

/// The flyout's position now: sliding from its bar into place while it opens.
/// Whether `window` is still shown: a closing flyout hands the focus back only to such a
/// window, never to a shell panel that was dismissed meanwhile (Search, Start: cloaked).
fn on_screen(window: isize) -> bool {
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    let hwnd = HWND(window as *mut _);
    let mut cloaked = 0u32;
    // SAFETY: read-only queries; a stale handle just fails them.
    unsafe {
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        IsWindowVisible(hwnd).as_bool() && cloaked == 0
    }
}

fn position(open: &Open, now: Instant) -> (i32, i32) {
    // On glass it opens in place: every move makes the engine re-render its glass at the new
    // spot, and a frame of that showed as the glass blinking out while the flyout slid in.
    if mypaper_ui::glass::style().is_some()
        || open.motion_style != mypaper_bars::style::MotionStyle::Slide
    {
        return open.origin;
    }
    let p = open.motion.map_or(1.0, |m| m.value(now));
    let offset = ((1.0 - p) * 10.0 * open.scale).round() as i32;
    let dy = if open.below { -offset } else { offset };
    (open.origin.0, open.origin.1 + dy)
}

/// Hit ids: a panel's controls are small numbers; list items `LIST + index`.
pub const LIST: HitId = 1000;
