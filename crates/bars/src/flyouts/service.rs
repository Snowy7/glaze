//! The flyouts' side of the main loop: requests in, lanes started on demand, results out as
//! [`Done`] (see `flyouts/mod.rs`). `app.rs` owns one and handles the requests that touch the
//! bars themselves (`room`, `keyboard`, `watch`, `setLayout`, pins); everything else is here.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use notify::Watcher;
use serde_json::{Value, json};

use super::clipboard::ClipboardLane;
use super::index::{self, App, Builder, History};
use super::mixer::MixerLane;
use super::notifications::NotificationsLane;
use super::quick::{self, QuickLane};
use super::wifi::WifiLane;
use super::{Done, Lane, Request, Watched, Watches, on_lane, release_idle, reply};
use crate::bridge::{BarError, Topic};
use crate::wake::{Msg, Waker};

/// After startup, before the index is (re)built: the bars come first.
const INDEX_DELAY: Duration = Duration::from_secs(8);
/// A launcher opened this long after the last build rebuilds (packaged apps install without
/// touching the Start Menu folders).
const INDEX_STALE: Duration = Duration::from_secs(10 * 60);
/// Start Menu changes come in bursts (an installer writing shortcuts).
const INDEX_SETTLE: Duration = Duration::from_secs(3);
/// Live refreshes coalesce (a mixer fires an event per volume step).
const REFRESH_SETTLE: Duration = Duration::from_millis(120);
/// The notification list while its flyout is open (no change events without package identity).
const LIST_EVERY: Duration = Duration::from_secs(4);
/// The badge while a `notifications` module is on screen.
const COUNT_EVERY: Duration = Duration::from_secs(15);

pub struct Flyouts {
    waker: Waker,
    icons: PathBuf,
    pub watches: Watches,
    pub switch_output: bool,

    apps: Option<Lane<Builder>>,
    quick: Option<Lane<QuickLane>>,
    wifi: Option<Lane<WifiLane>>,
    mixer: Option<Lane<MixerLane>>,
    notifications: Option<Lane<NotificationsLane>>,
    clipboard: Option<Lane<ClipboardLane>>,

    index: Option<Vec<App>>,
    built: Option<Instant>,
    building: bool,
    build_at: Option<Instant>,
    /// `apps` requests waiting for the first index.
    waiting: Vec<(u32, u64)>,
    history: History,
    history_path: Option<PathBuf>,
    /// The signed-in user, for the launcher's footer.
    user: std::cell::OnceCell<String>,
    start_menu: Option<notify::RecommendedWatcher>,

    dirty: BTreeSet<Watched>,
    dirty_at: Option<Instant>,
    list_at: Option<Instant>,
    counting: bool,
    count_at: Option<Instant>,
}

impl Flyouts {
    pub fn new(waker: Waker, icons: PathBuf) -> Self {
        let history_path = mypaper_bars::files::local("launcher.json").ok();
        Self {
            history: history_path
                .as_deref()
                .map(History::load)
                .unwrap_or_default(),
            history_path,
            user: std::cell::OnceCell::new(),
            waker,
            icons,
            watches: Watches::default(),
            switch_output: false,
            apps: None,
            quick: None,
            wifi: None,
            mixer: None,
            notifications: None,
            clipboard: None,
            index: None,
            built: None,
            building: false,
            build_at: Some(Instant::now() + INDEX_DELAY),
            waiting: Vec::new(),
            start_menu: None,
            dirty: BTreeSet::new(),
            dirty_at: None,
            list_at: None,
            counting: false,
            count_at: None,
        }
    }

    fn cache_path() -> Option<PathBuf> {
        mypaper_bars::files::local("apps.json").ok()
    }

    /// The index, from the cache file the first time.
    fn index(&mut self) -> Option<&Vec<App>> {
        if self.index.is_none() {
            self.index = Self::cache_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|text| index::parse_cache(&text));
        }
        self.index.as_ref()
    }

    fn apps_value(&self) -> Value {
        let user = self.user.get_or_init(index::user_name);
        json!({
            "apps": self.index.as_deref().unwrap_or_default(),
            "recent": self.history.recent,
            "user": { "name": user },
        })
    }

    /// Rebuilds the index on the apps lane.
    fn build(&mut self) {
        if self.building {
            return;
        }
        self.building = true;
        self.watch_start_menu();
        let (waker, icons) = (self.waker.clone(), self.icons.clone());
        on_lane(
            &mut self.apps,
            "apps",
            move || Builder::new(icons),
            move |builder| {
                let apps = builder.build();
                if let Some(path) = Self::cache_path()
                    && let Err(e) = mypaper_bars::files::write_atomic(
                        &path,
                        index::cache_json(&apps).as_bytes(),
                    )
                {
                    log::warn!("launcher: not caching the index: {e:#}");
                }
                waker.send(Msg::Flyout(Done::Index(apps)));
            },
        );
    }

    /// Rebuilds when the Start Menu folders change.
    fn watch_start_menu(&mut self) {
        if self.start_menu.is_some() {
            return;
        }
        let waker = self.waker.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|e| !e.kind.is_access()) {
                waker.send(Msg::Flyout(Done::Changed(Watched::Apps)));
            }
        });
        self.start_menu = watcher
            .map(|mut watcher| {
                for dir in index::start_menu_dirs() {
                    let _ = watcher.watch(&dir, notify::RecursiveMode::Recursive);
                }
                watcher
            })
            .inspect_err(|e| log::warn!("launcher: not watching the Start Menu: {e}"))
            .ok();
    }

    /// The new index arrived: answers the `apps` requests waiting for it. Returns the value for
    /// the bars watching `apps`, and the replies.
    pub fn index_built(&mut self, apps: Vec<App>) -> (Value, Vec<(u32, u64, Value)>) {
        self.building = false;
        self.built = Some(Instant::now());
        self.index = Some(apps);
        let value = self.apps_value();
        let replies = self
            .waiting
            .drain(..)
            .map(|(bar, id)| (bar, id, value.clone()))
            .collect();
        (value, replies)
    }

    /// Handles a request. `None`: the answer comes later, as [`Done::Reply`].
    pub fn call(&mut self, bar: u32, id: u64, request: Request) -> Option<Result<Value, BarError>> {
        let waker = self.waker.clone();
        let later = move |result: Result<Value, String>| {
            reply(&waker, bar, id, result.map_err(BarError::unavailable));
        };
        let icons = self.icons.clone();
        match request {
            Request::Apps => {
                if self.built.is_none_or(|at| at.elapsed() > INDEX_STALE) {
                    self.build_at = Some(Instant::now());
                }
                if self.index().is_some() {
                    return Some(Ok(self.apps_value()));
                }
                self.waiting.push((bar, id));
                self.build_at = Some(Instant::now());
                None
            }
            Request::Run { id: app, path } => {
                self.history.record(&app, &path, index::epoch_ms());
                if let Some(file) = &self.history_path {
                    self.history.save(file);
                }
                crate::system::open(path);
                Some(Ok(json!({ "recent": self.history.recent })))
            }
            Request::RecentFiles => {
                on_lane(
                    &mut self.apps,
                    "apps",
                    move || Builder::new(icons),
                    move |builder| {
                        later(Ok(json!(builder.recent_files(40))));
                    },
                );
                None
            }
            Request::Clipboard => {
                on_lane(
                    &mut self.clipboard,
                    "clipboard",
                    ClipboardLane::new,
                    move |lane| {
                        later(Ok(json!(lane.read())));
                    },
                );
                None
            }
            Request::ClipboardPaste { index } => {
                on_lane(
                    &mut self.clipboard,
                    "clipboard",
                    ClipboardLane::new,
                    move |lane| {
                        later(lane.paste(index).map(|()| Value::Null));
                    },
                );
                None
            }
            Request::Quick => {
                on_lane(&mut self.quick, "quick", QuickLane::new, move |lane| {
                    later(Ok(json!(lane.read())));
                });
                None
            }
            Request::SetRadio { kind, on } => {
                on_lane(&mut self.quick, "quick", QuickLane::new, move |lane| {
                    later(lane.set_radio(kind, on).map(|()| json!(lane.read())));
                });
                None
            }
            Request::SetBrightness { level } => {
                on_lane(&mut self.quick, "quick", QuickLane::new, move |lane| {
                    later(lane.set_brightness(level).map(|()| Value::Null));
                });
                None
            }
            Request::SetPowerMode { mode } => {
                on_lane(&mut self.quick, "quick", QuickLane::new, move |lane| {
                    later(quick::set_power_mode(mode).map(|()| json!(lane.read())));
                });
                None
            }
            Request::Wifi { scan } => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.wifi,
                    "wifi",
                    move || WifiLane::new(waker),
                    move |lane| {
                        if scan {
                            lane.scan();
                        }
                        later(Ok(json!(lane.read())));
                    },
                );
                None
            }
            Request::WifiConnect { ssid, password } => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.wifi,
                    "wifi",
                    move || WifiLane::new(waker),
                    move |lane| {
                        later(
                            lane.connect(&ssid, password.as_deref())
                                .map(|()| Value::Null),
                        );
                    },
                );
                None
            }
            Request::WifiDisconnect => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.wifi,
                    "wifi",
                    move || WifiLane::new(waker),
                    move |lane| {
                        later(lane.disconnect().map(|()| Value::Null));
                    },
                );
                None
            }
            Request::Mixer => {
                let (waker, switchable) = (self.waker.clone(), self.switch_output);
                on_lane(
                    &mut self.mixer,
                    "mixer",
                    move || MixerLane::new(waker, icons),
                    move |lane| {
                        later(Ok(json!(lane.read(switchable))));
                    },
                );
                None
            }
            Request::SetSession {
                id: session,
                level,
                mute,
            } => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.mixer,
                    "mixer",
                    move || MixerLane::new(waker, icons),
                    move |lane| {
                        later(
                            lane.set_session(&session, level, mute)
                                .map(|()| Value::Null),
                        );
                    },
                );
                None
            }
            Request::SetOutput { id: device } => {
                if !self.switch_output {
                    return Some(Err(BarError::unavailable(
                        "switching the output needs audio.switchOutput (an undocumented API)",
                    )));
                }
                let waker = self.waker.clone();
                on_lane(
                    &mut self.mixer,
                    "mixer",
                    move || MixerLane::new(waker, icons),
                    move |lane| {
                        later(lane.set_output(&device).map(|()| json!(lane.read(true))));
                    },
                );
                None
            }
            Request::Notifications => {
                on_lane(
                    &mut self.notifications,
                    "notifications",
                    move || NotificationsLane::new(icons),
                    move |lane| {
                        later(Ok(json!(lane.list())));
                    },
                );
                None
            }
            Request::DismissNotification { id: toast } => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.notifications,
                    "notifications",
                    move || NotificationsLane::new(icons),
                    move |lane| {
                        let result = lane.dismiss(toast);
                        count_into(&waker, lane);
                        later(result.map(|()| json!(lane.list())));
                    },
                );
                None
            }
            Request::ClearNotifications => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.notifications,
                    "notifications",
                    move || NotificationsLane::new(icons),
                    move |lane| {
                        let result = lane.clear();
                        count_into(&waker, lane);
                        later(result.map(|()| json!(lane.list())));
                    },
                );
                None
            }
            // The bars' own business: app.rs answers these.
            Request::Watch { .. }
            | Request::Unwatch { .. }
            | Request::Room { .. }
            | Request::Keyboard { .. }
            | Request::SetLayout { .. }
            | Request::LauncherPin { .. }
            | Request::LauncherUnpin { .. } => Some(Err(BarError::internal("handled by the host"))),
        }
    }

    /// A provider saw a change: refresh what's watched, soon.
    pub fn changed(&mut self, what: Watched) {
        if what == Watched::Apps {
            self.build_at.get_or_insert(Instant::now() + INDEX_SETTLE);
            return;
        }
        if self.watches.any(what) {
            self.dirty.insert(what);
            self.dirty_at.get_or_insert(Instant::now() + REFRESH_SETTLE);
        }
    }

    /// A flyout started watching `what`: send it the current value.
    pub fn watched(&mut self, what: Watched) {
        match what {
            Watched::Mixer | Watched::Wifi | Watched::NotificationList => {
                self.dirty.insert(what);
                self.dirty_at = Some(Instant::now());
                if what == Watched::NotificationList {
                    self.list_at = Some(Instant::now() + LIST_EVERY);
                }
            }
            Watched::Apps | Watched::Core(_) => {}
        }
    }

    /// Recount notifications on a timer (a `notifications` module is on screen).
    pub fn set_counting(&mut self, on: bool) {
        if on != self.counting {
            self.counting = on;
            self.count_at = on.then(Instant::now);
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        let lanes = [
            self.apps.as_ref().map(|l| l.used),
            self.quick.as_ref().map(|l| l.used),
            self.wifi.as_ref().map(|l| l.used),
            self.mixer.as_ref().map(|l| l.used),
            self.notifications.as_ref().map(|l| l.used),
            self.clipboard.as_ref().map(|l| l.used),
        ]
        .into_iter()
        .flatten()
        .map(|used| used + super::IDLE)
        .min();
        [
            self.build_at,
            self.dirty_at,
            self.list_at,
            self.count_at,
            lanes,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn tick(&mut self, now: Instant) {
        if self.build_at.is_some_and(|at| at <= now) {
            self.build_at = None;
            self.build();
        }
        if self.dirty_at.is_some_and(|at| at <= now) {
            self.dirty_at = None;
            for what in std::mem::take(&mut self.dirty) {
                self.refresh(what);
            }
        }
        if self.list_at.is_some_and(|at| at <= now) {
            self.list_at = self
                .watches
                .any(Watched::NotificationList)
                .then(|| now + LIST_EVERY);
            if self.list_at.is_some() {
                self.refresh(Watched::NotificationList);
            }
        }
        if self.count_at.is_some_and(|at| at <= now) {
            self.count_at = self.counting.then(|| now + COUNT_EVERY);
            if self.counting {
                let (waker, icons) = (self.waker.clone(), self.icons.clone());
                on_lane(
                    &mut self.notifications,
                    "notifications",
                    move || NotificationsLane::new(icons),
                    move |lane| {
                        count_into(&waker, lane);
                    },
                );
            }
        }
        release_idle(&mut self.apps, now, self.building);
        release_idle(&mut self.quick, now, false);
        release_idle(&mut self.wifi, now, self.watches.any(Watched::Wifi));
        release_idle(&mut self.mixer, now, self.watches.any(Watched::Mixer));
        let counting = self.counting || self.watches.any(Watched::NotificationList);
        release_idle(&mut self.notifications, now, counting);
        release_idle(&mut self.clipboard, now, false);
    }

    /// Reads `what` again on its lane; the value comes back as [`Done::Event`].
    fn refresh(&mut self, what: Watched) {
        let waker = self.waker.clone();
        let icons = self.icons.clone();
        let event = move |data: Value| waker.send(Msg::Flyout(Done::Event { topic: what, data }));
        match what {
            Watched::Mixer => {
                let (waker, switchable) = (self.waker.clone(), self.switch_output);
                on_lane(
                    &mut self.mixer,
                    "mixer",
                    move || MixerLane::new(waker, icons),
                    move |lane| {
                        event(json!(lane.read(switchable)));
                    },
                );
            }
            Watched::Wifi => {
                let waker = self.waker.clone();
                on_lane(
                    &mut self.wifi,
                    "wifi",
                    move || WifiLane::new(waker),
                    move |lane| {
                        event(json!(lane.read()));
                    },
                );
            }
            Watched::NotificationList => {
                on_lane(
                    &mut self.notifications,
                    "notifications",
                    move || NotificationsLane::new(icons),
                    move |lane| {
                        event(json!(lane.list()));
                    },
                );
            }
            Watched::Apps | Watched::Core(_) => {}
        }
    }
}

/// Recounts notifications and reports the count as the `notifications` topic's value.
fn count_into(waker: &Waker, lane: &NotificationsLane) {
    let count = lane.count();
    waker.send(Msg::Flyout(Done::Event {
        topic: Watched::Core(Topic::Notifications),
        data: json!({ "count": count }),
    }));
}
