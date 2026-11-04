//! The data behind the flyouts (docs/bars.md, "Flyouts"): the launcher's app index, quick
//! settings, Wi-Fi, the audio mixer, keyboard layouts, notifications and the clipboard history.
//!
//! Each provider runs on its own thread (a [`Lane`]), made on the first request a flyout sends
//! and released after [`IDLE`] without one, so nothing runs (or polls) while every flyout is
//! closed. Requests that take time (a Wi-Fi scan, DDC/CI brightness, reading notifications) answer
//! later: the lane hands the result to the main loop as a [`Done`], which posts it to the page.
//! An open flyout `watch`es the topics it shows live (the mixer's sessions, the Wi-Fi list, core
//! topics such as media or stats); the host sends those events to its bar until it `unwatch`es.

pub mod clipboard;
pub mod index;
pub mod language;
pub mod mixer;
pub mod notifications;
pub mod quick;
pub mod service;
pub mod wifi;
pub mod winkey;

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

pub use service::Flyouts;

use crate::bridge::{BarError, Topic};
use crate::wake::{Msg, Waker};

/// How long an unused provider lives on.
pub const IDLE: Duration = Duration::from_secs(60);

/// The bridge methods that belong to the flyouts (the page's side: `flyouts/protocol.ts`).
pub const METHODS: &[&str] = &[
    "watch",
    "unwatch",
    "room",
    "keyboard",
    "apps",
    "launcherRun",
    "recentFiles",
    "clipboard",
    "clipboardPaste",
    "quick",
    "setRadio",
    "setBrightness",
    "setPowerMode",
    "wifi",
    "wifiConnect",
    "wifiDisconnect",
    "mixer",
    "setSession",
    "setOutput",
    "setLayout",
    "launcherPin",
    "launcherUnpin",
    "notifications",
    "dismissNotification",
    "clearNotifications",
];

/// A flyout's request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum Request {
    /// Send `topic`'s events to this bar while the flyout is open.
    Watch {
        topic: String,
    },
    Unwatch {
        topic: String,
    },
    /// The window must reach `extent` DIPs past the bar's band (0: back to the default room).
    Room {
        extent: f64,
    },
    /// Take the keyboard (the bar becomes the foreground window), or let it go. `handBack`:
    /// the flyout closed by keyboard or by launching something (no press just before), so the
    /// app that was in front gets the foreground back.
    Keyboard {
        on: bool,
        #[serde(default, rename = "handBack")]
        hand_back: bool,
    },
    /// The app index and the launch history.
    Apps,
    /// Opens an app, a settings page or a file from the launcher, and remembers it.
    LauncherRun {
        id: String,
        path: String,
    },
    RecentFiles,
    Clipboard,
    ClipboardPaste {
        index: usize,
    },
    Quick,
    SetRadio {
        kind: quick::RadioKind,
        on: bool,
    },
    SetBrightness {
        level: u32,
    },
    SetPowerMode {
        mode: quick::PowerMode,
    },
    /// The Wi-Fi networks (`scan`: ask the adapter to look again first).
    Wifi {
        #[serde(default)]
        scan: bool,
    },
    WifiConnect {
        ssid: String,
        #[serde(default)]
        password: Option<String>,
    },
    WifiDisconnect,
    Mixer,
    SetSession {
        id: String,
        #[serde(default)]
        level: Option<f32>,
        #[serde(default)]
        mute: Option<bool>,
    },
    SetOutput {
        id: String,
    },
    SetLayout {
        id: String,
    },
    /// Pins to the launcher's grid (`launcher.pinned` in `bars.json`), at `index` or last.
    LauncherPin {
        path: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    LauncherUnpin {
        path: String,
    },
    Notifications,
    DismissNotification {
        id: u32,
    },
    ClearNotifications,
}

impl Request {
    pub fn parse(method: &str, params: Value) -> Result<Self, BarError> {
        let request = if params.is_null() {
            serde_json::json!({ "method": method })
        } else {
            serde_json::json!({ "method": method, "params": params })
        };
        let parsed: Self = serde_json::from_value(request)
            .map_err(|e| BarError::invalid(format!("{method}: {e}")))?;
        match &parsed {
            Self::SetSession { level: Some(l), .. } if !(0.0..=1.0).contains(l) => Err(
                BarError::invalid("setSession.level must be between 0 and 1"),
            ),
            Self::SetBrightness { level } if *level > 100 => {
                Err(BarError::invalid("setBrightness.level is a percent"))
            }
            Self::Room { extent } if !extent.is_finite() || *extent < 0.0 => Err(
                BarError::invalid("room.extent must be a positive number of DIPs"),
            ),
            Self::LauncherRun { path, .. } if path.trim().is_empty() => {
                Err(BarError::invalid("launcherRun: a path is required"))
            }
            Self::WifiConnect { ssid, .. } if ssid.is_empty() || ssid.len() > 32 => {
                Err(BarError::invalid("wifiConnect.ssid must be 1 to 32 bytes"))
            }
            _ => Ok(parsed),
        }
    }
}

/// What a lane finished, for the main loop.
pub enum Done {
    /// The answer to request `id` from bar `bar`'s page load `page` (a page that reloaded since
    /// numbers its requests afresh: the answer is dropped).
    Reply {
        bar: u32,
        page: u64,
        id: u64,
        result: Result<Value, BarError>,
    },
    /// A flyout topic's new value, for the bars watching it.
    Event { topic: Watched, data: Value },
    /// A Windows key shortcut (the key alone, Win+N, Win+A), when it happened: open its flyout.
    WinKey(Instant, winkey::Target),
    /// The app index was (re)built.
    Index(Vec<index::App>),
    /// A provider noticed a change (a session came, a scan finished): refresh what's watched.
    Changed(Watched),
}

/// What a flyout can watch beyond the core topics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Watched {
    Core(Topic),
    Mixer,
    Wifi,
    Apps,
    NotificationList,
}

impl Watched {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "mixer" => Self::Mixer,
            "wifi" => Self::Wifi,
            "launcherApps" => Self::Apps,
            "notificationList" => Self::NotificationList,
            "media" => Self::Core(Topic::Media),
            "volume" => Self::Core(Topic::Volume),
            "network" => Self::Core(Topic::Network),
            "battery" => Self::Core(Topic::Battery),
            "stats" => Self::Core(Topic::Stats),
            "clock" => Self::Core(Topic::Clock),
            "language" => Self::Core(Topic::Language),
            "notifications" => Self::Core(Topic::Notifications),
            _ => return None,
        })
    }

    pub fn event(self) -> &'static str {
        match self {
            Self::Core(topic) => topic.event(),
            Self::Mixer => "mixer",
            Self::Wifi => "wifi",
            Self::Apps => "launcherApps",
            Self::NotificationList => "notificationList",
        }
    }
}

/// A unit of work for a lane.
type Job<S> = Box<dyn FnOnce(&mut S) + Send>;

/// One provider's thread: jobs go in; results come back through the waker. Dropping it ends the
/// thread (and drops its state there, in its own COM apartment).
pub struct Lane<S> {
    tx: Sender<Job<S>>,
    pub used: Instant,
}

impl<S: 'static> Lane<S> {
    /// Starts a lane; `init` runs on its thread and makes its state.
    pub fn spawn(name: &str, init: impl FnOnce() -> S + Send + 'static) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<Job<S>>();
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut state = init();
                for job in rx {
                    job(&mut state);
                }
            });
        spawned
            .inspect_err(|e| log::warn!("no {name} thread: {e}"))
            .ok()
            .map(|_| Self {
                tx,
                used: Instant::now(),
            })
    }

    pub fn run(&mut self, job: impl FnOnce(&mut S) + Send + 'static) {
        self.used = Instant::now();
        let _ = self.tx.send(Box::new(job));
    }

    pub fn idle(&self, now: Instant) -> bool {
        now.duration_since(self.used) >= IDLE
    }
}

/// Runs `job` on the lane in `slot`, starting it if needed.
pub fn on_lane<S: 'static>(
    slot: &mut Option<Lane<S>>,
    name: &str,
    init: impl FnOnce() -> S + Send + 'static,
    job: impl FnOnce(&mut S) + Send + 'static,
) {
    if slot.is_none() {
        *slot = Lane::spawn(name, init);
    }
    if let Some(lane) = slot {
        lane.run(job);
    }
}

/// Drops `slot`'s lane when it's been idle long enough. A `kept` one (something it feeds is
/// watched, or it's busy) counts as used now, so it goes [`IDLE`] after the last watch ends, and
/// its release time (the loop's deadline) never lies in the past.
pub fn release_idle<S: 'static>(slot: &mut Option<Lane<S>>, now: Instant, kept: bool) {
    if kept {
        if let Some(lane) = slot {
            lane.used = now;
        }
    } else if slot.as_ref().is_some_and(|lane| lane.idle(now)) {
        *slot = None;
    }
}

/// Which bars watch what.
#[derive(Default)]
pub struct Watches {
    set: BTreeSet<(u32, Watched)>,
}

impl Watches {
    pub fn add(&mut self, bar: u32, what: Watched) -> bool {
        self.set.insert((bar, what))
    }

    pub fn remove(&mut self, bar: u32, what: Watched) {
        self.set.remove(&(bar, what));
    }

    /// A bar reloaded or went away: its flyouts are gone.
    pub fn forget(&mut self, bar: u32) {
        self.set.retain(|(b, _)| *b != bar);
    }

    pub fn watching(&self, bar: u32, what: Watched) -> bool {
        self.set.contains(&(bar, what))
    }

    pub fn any(&self, what: Watched) -> bool {
        self.set.iter().any(|(_, w)| *w == what)
    }

    pub fn bars(&self, what: Watched) -> impl Iterator<Item = u32> + '_ {
        self.set
            .iter()
            .filter(move |(_, w)| *w == what)
            .map(|(b, _)| *b)
    }

    /// The core topics some flyout watches.
    pub fn core(&self) -> impl Iterator<Item = Topic> + '_ {
        self.set.iter().filter_map(|(_, w)| match w {
            Watched::Core(topic) => Some(*topic),
            _ => None,
        })
    }
}

/// Sends a lane's result to the main loop, for the page load that asked.
pub fn reply(waker: &Waker, bar: u32, page: u64, id: u64, result: Result<Value, BarError>) {
    waker.send(Msg::Flyout(Done::Reply {
        bar,
        page,
        id,
        result,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flyout_requests_and_checks_them() {
        let parse = |method: &str, params: Value| Request::parse(method, params);
        assert_eq!(
            parse("watch", serde_json::json!({ "topic": "mixer" })),
            Ok(Request::Watch {
                topic: "mixer".into()
            })
        );
        assert_eq!(parse("apps", Value::Null), Ok(Request::Apps));
        // `handBack` is optional (taking the keyboard has none).
        assert_eq!(
            parse("keyboard", serde_json::json!({ "on": true })),
            Ok(Request::Keyboard {
                on: true,
                hand_back: false
            })
        );
        assert_eq!(
            parse(
                "keyboard",
                serde_json::json!({ "on": false, "handBack": true })
            ),
            Ok(Request::Keyboard {
                on: false,
                hand_back: true
            })
        );
        assert_eq!(
            parse("wifiConnect", serde_json::json!({ "ssid": "Home" })),
            Ok(Request::WifiConnect {
                ssid: "Home".into(),
                password: None
            })
        );
        assert_eq!(
            parse(
                "setRadio",
                serde_json::json!({ "kind": "bluetooth", "on": false })
            ),
            Ok(Request::SetRadio {
                kind: quick::RadioKind::Bluetooth,
                on: false
            })
        );
        for (method, params) in [
            ("setSession", serde_json::json!({ "id": "x", "level": 2 })),
            ("setBrightness", serde_json::json!({ "level": 101 })),
            ("room", serde_json::json!({ "extent": -1 })),
            ("launcherRun", serde_json::json!({ "id": "x", "path": " " })),
            ("wifiConnect", serde_json::json!({ "ssid": "" })),
            ("setPowerMode", serde_json::json!({ "mode": "turbo" })),
        ] {
            assert_eq!(
                parse(method, params).map_err(|e| e.code),
                Err("InvalidParams"),
                "{method}"
            );
        }
        assert!(
            METHODS.iter().all(|m| *m != "hello"),
            "core methods stay in bridge.rs"
        );
    }

    #[test]
    fn watches_are_per_bar() {
        let mut watches = Watches::default();
        assert!(watches.add(1, Watched::Mixer));
        assert!(!watches.add(1, Watched::Mixer));
        watches.add(2, Watched::Core(Topic::Stats));
        assert!(watches.watching(1, Watched::Mixer));
        assert_eq!(watches.core().collect::<Vec<_>>(), [Topic::Stats]);
        watches.forget(1);
        assert!(!watches.any(Watched::Mixer));
        assert_eq!(Watched::parse("stats"), Some(Watched::Core(Topic::Stats)));
        assert_eq!(Watched::parse("nope"), None);
        assert_eq!(Watched::NotificationList.event(), "notificationList");
    }

    #[test]
    fn a_watched_lane_stays_without_a_past_deadline() {
        let mut slot = Lane::spawn("test-lane", || ());
        let start = Instant::now();
        let later = start + IDLE * 2;
        // Watched past its idle time: kept, and its release moves on (no deadline in the past
        // for the loop to spin on).
        release_idle(&mut slot, later, true);
        let lane = slot.as_ref().expect("kept while watched");
        assert_eq!(lane.used, later);
        assert!(lane.used + IDLE > later);
        // Unwatched: it goes once it's been idle IDLE since the last watch, not before.
        release_idle(&mut slot, later + IDLE / 2, false);
        assert!(slot.is_some());
        release_idle(&mut slot, later + IDLE, false);
        assert!(slot.is_none());
    }
}
