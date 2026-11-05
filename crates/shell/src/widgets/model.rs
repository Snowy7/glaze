//! Per-instance native state. Persisted values retain the HTML widgets' keys and JSON shapes.
use super::time::{self, Stamp};
use mypaper_ui::calendar::Date;
use mypaper_ui::{Image, TextField};
use mypaper_widgets::{layout::Placed, stats::SystemStats, storage::Storage};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widget {
    Clock,
    Calendar,
    Media,
    QuickToggles,
    Launcher,
    Weather,
    System,
    Timer,
    Countdown,
    Photo,
    Notes,
    Todo,
    Rss,
    Crypto,
    Heatmap,
    Quote,
}
impl Widget {
    pub fn of(id: &str) -> Option<Self> {
        Some(match id {
            "clock" => Self::Clock,
            "calendar" => Self::Calendar,
            "now-playing" | "media-pill" => Self::Media,
            "quick-toggles" => Self::QuickToggles,
            "launcher" => Self::Launcher,
            "weather" => Self::Weather,
            "system-monitor" => Self::System,
            "timer" => Self::Timer,
            "countdown" => Self::Countdown,
            "photo" => Self::Photo,
            "notes" => Self::Notes,
            "todo" => Self::Todo,
            "rss" => Self::Rss,
            "crypto" => Self::Crypto,
            "heatmap" => Self::Heatmap,
            "quote" => Self::Quote,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub text: String,
    #[serde(default)]
    pub done: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub mode: String,
    pub total: i64,
    pub remaining: i64,
    pub ends_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub sessions: u32,
}
impl Timer {
    pub fn idle(settings: &Map<String, Value>, mode: &str, sessions: u32) -> Self {
        let key = match mode {
            "short" => "shortMinutes",
            "long" => "longMinutes",
            "custom" => "customMinutes",
            _ => "focusMinutes",
        };
        let default = match mode {
            "short" => 5.0,
            "long" => 15.0,
            "custom" => 10.0,
            _ => 25.0,
        };
        let total = (number(settings, key, default).clamp(1.0, 600.0) * 60_000.0) as i64;
        Self {
            mode: mode.into(),
            total,
            remaining: total,
            ends_at: None,
            finished_at: None,
            sessions,
        }
    }
    pub fn left(&self, now: i64) -> i64 {
        self.ends_at.map_or(self.remaining, |end| end - now).max(0)
    }
    pub fn next_mode(&self, settings: &Map<String, Value>) -> &'static str {
        if self.mode == "focus" {
            if self.sessions > 0
                && self
                    .sessions
                    .is_multiple_of(number(settings, "sessionsBeforeLong", 4.0) as u32)
            {
                "long"
            } else {
                "short"
            }
        } else if self.mode == "custom" {
            "custom"
        } else {
            "focus"
        }
    }
    fn valid(&self) -> bool {
        matches!(self.mode.as_str(), "focus" | "short" | "long" | "custom")
            && self.total > 0
            && self.total <= 36_000_000
            && self.remaining >= 0
            && self.remaining <= self.total
    }
}
#[derive(Default)]
pub struct Photo {
    pub current: Option<Arc<Image>>,
    pub previous: Option<Arc<Image>>,
    pub index: usize,
    pub fade: Option<Instant>,
    pub change: Option<Instant>,
}

pub struct Model {
    pub placed: Placed,
    pub kind: Widget,
    pub data: Value,
    pub error: Option<String>,
    pub note: String,
    pub tasks: Vec<Task>,
    pub timer: Timer,
    pub events: Vec<Value>,
    pub month: Date,
    pub selected: Date,
    pub stats: Option<SystemStats>,
    pub history: VecDeque<[f64; 3]>,
    pub counts: BTreeMap<String, u32>,
    pub photo: Photo,
    pub icons: HashMap<usize, Arc<Image>>,
    pub artwork: Option<Arc<Image>>,
    pub spectrum: Vec<f32>,
    pub field: Option<TextField>,
    pub task_edit: Option<usize>,
    pub quote_offset: usize,
    pub visible: bool,
    pub dirty: bool,
    pub next: Option<Instant>,
    pub fetch_at: Option<Instant>,
    pub fetching: bool,
    pub generation: u64,
    pub counted: HashMap<usize, String>,
    pub save_at: Option<Instant>,
}
impl Model {
    pub fn new(placed: Placed, storage: &mut Storage, now: Stamp) -> anyhow::Result<Self> {
        let kind =
            Widget::of(&placed.widget).ok_or_else(|| anyhow::anyhow!("unknown native widget"))?;
        let id = &placed.instance;
        let note = if kind == Widget::Notes {
            storage.get(id, "text")?.as_str().unwrap_or_default().into()
        } else {
            String::new()
        };
        let tasks = if kind == Widget::Todo {
            storage
                .get(id, "items")?
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| serde_json::from_value::<Task>(v.clone()).ok())
                .filter(|t| !t.text.trim().is_empty())
                .take(300)
                .collect()
        } else {
            Vec::new()
        };
        let timer = if kind == Widget::Timer {
            serde_json::from_value::<Timer>(storage.get(id, "state")?)
                .ok()
                .filter(Timer::valid)
        } else {
            None
        }
        .unwrap_or_else(|| Timer::idle(&placed.settings, "focus", 0));
        let events = if kind == Widget::Calendar {
            storage
                .get(id, "events")?
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut data = if matches!(kind, Widget::Rss | Widget::Crypto) {
            storage.get(id, "cache")?
        } else {
            Value::Null
        };
        // Earlier crypto caches keyed quotes by coin id; expose the same stored values before
        // the first native fetch completes, without rewriting the user's cache on load.
        if kind == Widget::Crypto && data["coins"].is_null() && data["data"].is_object() {
            let coins:Vec<_>=placed.settings.get("coins").and_then(Value::as_array).into_iter().flatten().filter_map(|c|c["id"].as_str()).filter_map(|id|Some(json!({"id":id,"name":id,"price":data["data"][id]["price"].as_f64()?,"change":data["data"][id]["change"]}))).collect();
            data["coins"] = json!(coins);
        }
        Ok(Self {
            placed,
            kind,
            data,
            error: None,
            note,
            tasks,
            timer,
            events,
            month: Date { day: 1, ..now.date },
            selected: now.date,
            stats: None,
            history: VecDeque::new(),
            counts: BTreeMap::new(),
            photo: Photo::default(),
            icons: HashMap::new(),
            artwork: None,
            spectrum: Vec::new(),
            field: None,
            task_edit: None,
            quote_offset: 0,
            visible: false,
            dirty: true,
            next: None,
            fetch_at: None,
            fetching: false,
            generation: 0,
            counted: HashMap::new(),
            save_at: None,
        })
    }
    pub fn str(&self, key: &str, default: &str) -> String {
        string(&self.placed.settings, key, default).into()
    }
    pub fn num(&self, key: &str, default: f64) -> f64 {
        number(&self.placed.settings, key, default)
    }
    pub fn flag(&self, key: &str, default: bool) -> bool {
        flag(&self.placed.settings, key, default)
    }
    pub fn list(&self, key: &str) -> &[Value] {
        self.placed
            .settings
            .get(key)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
    pub fn photo_paths(&self) -> Vec<String> {
        (1..=6)
            .map(|i| self.str(&format!("image{i}"), ""))
            .filter(|s| !s.is_empty())
            .collect()
    }
    pub fn countdown(&self, now: Stamp) -> (i64, i64) {
        let date = Date::parse(&self.str("date", "")).unwrap_or_else(|| {
            let jan = Date {
                year: now.date.year,
                month: 1,
                day: 1,
            };
            if now.date == jan {
                jan
            } else {
                Date {
                    year: now.date.year + 1,
                    ..jan
                }
            }
        });
        let clock = self.str("time", "00:00");
        let (hour, minute) = clock
            .split_once(':')
            .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
            .filter(|(h, m)| *h < 24 && *m < 60)
            .unwrap_or((0, 0));
        let target = time::local_epoch(date, hour, minute).unwrap_or(now.ms);
        let start = Date::parse(&self.str("startDate", ""))
            .and_then(|d| time::local_epoch(d, 0, 0))
            .filter(|s| *s < target)
            .unwrap_or(target - 365 * 86_400_000);
        (target, start)
    }
    pub fn schedule(&mut self, now: Stamp, instant: Instant) {
        self.next = if !self.visible {
            None
        } else {
            match self.kind {
                Widget::Clock => Some(
                    instant
                        + time::boundary(
                            now.ms,
                            if self.flag("showSeconds", false)
                                && !matches!(
                                    self.str("variant", "desk").as_str(),
                                    "world" | "words"
                                )
                            {
                                1000
                            } else {
                                60_000
                            },
                        ),
                ),
                Widget::Media => None,
                Widget::Calendar | Widget::Heatmap => Some(
                    instant + Duration::from_millis((now.midnight_after() - now.ms).max(1) as u64),
                ),
                Widget::Timer => self
                    .timer
                    .ends_at
                    .map(|_| instant + time::remaining_boundary(self.timer.left(now.ms), 1000)),
                Widget::Countdown => {
                    let (target, _) = self.countdown(now);
                    let left = target - now.ms;
                    let quantum = if self.flag("showSeconds", false) {
                        1000
                    } else {
                        60_000
                    };
                    Some(
                        instant
                            + if left > 0 {
                                time::remaining_boundary(left, quantum)
                            } else {
                                Duration::from_millis((now.midnight_after() - now.ms).max(1) as u64)
                            },
                    )
                }
                Widget::Quote => match self.str("mode", "daily").as_str() {
                    "click" => None,
                    "hourly" => Some(instant + time::boundary(now.ms, 3_600_000)),
                    _ => Some(
                        instant
                            + Duration::from_millis((now.midnight_after() - now.ms).max(1) as u64),
                    ),
                },
                Widget::Photo => {
                    if self.photo.fade.is_some() {
                        None
                    } else {
                        self.photo.change
                    }
                }
                _ => None,
            }
        };
    }
    pub fn persist(&self, storage: &mut Storage) -> anyhow::Result<()> {
        match self.kind {
            Widget::Notes => storage.set(&self.placed.instance, "text", Some(json!(self.note))),
            Widget::Todo => storage.set(
                &self.placed.instance,
                "items",
                Some(serde_json::to_value(&self.tasks)?),
            ),
            Widget::Calendar => storage.set(
                &self.placed.instance,
                "events",
                Some(serde_json::to_value(&self.events)?),
            ),
            Widget::Timer => storage.set(
                &self.placed.instance,
                "state",
                Some(serde_json::to_value(&self.timer)?),
            ),
            _ => Ok(()),
        }
    }
}
pub fn string<'a>(settings: &'a Map<String, Value>, key: &str, default: &'a str) -> &'a str {
    settings.get(key).and_then(Value::as_str).unwrap_or(default)
}
pub fn number(settings: &Map<String, Value>, key: &str, default: f64) -> f64 {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}
pub fn flag(settings: &Map<String, Value>, key: &str, default: bool) -> bool {
    settings
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}
