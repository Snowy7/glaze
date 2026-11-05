//! Per-instance native state. Persisted values retain the HTML widgets' keys and JSON shapes.
use super::time::{self, Stamp};
use mypaper_ui::calendar::Date;
use mypaper_ui::{Image, TextField};
use mypaper_widgets::{layout::Placed, stats::SystemStats, storage::Storage};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::cell::Cell;
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
    /// Entering any new mode after a long break starts a fresh session cycle.
    pub fn enter(&self, settings: &Map<String, Value>, mode: &str) -> Self {
        Self::idle(
            settings,
            mode,
            if self.mode == "long" {
                0
            } else {
                self.sessions
            },
        )
    }
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
                && self.sessions.is_multiple_of(
                    number(settings, "sessionsBeforeLong", 4.0).clamp(1.0, 12.0) as u32,
                )
            {
                "long"
            } else {
                "short"
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPart {
    Title,
    Date,
    Time,
}
pub struct EventDraft {
    pub title: String,
    pub date: String,
    pub time: String,
    pub color: usize,
    pub part: EventPart,
}
impl EventDraft {
    pub fn capture(&mut self, text: String) {
        match self.part {
            EventPart::Title => self.title = text,
            EventPart::Date => self.date = text,
            EventPart::Time => self.time = text,
        }
    }
    pub fn event(&self) -> Result<Value, String> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err("Enter an event title".into());
        }
        let date = Date::parse(self.date.trim()).ok_or("Enter a valid date as YYYY-MM-DD")?;
        let time = self.time.trim();
        if !time.is_empty()
            && !time.split_once(':').is_some_and(|(h, m)| {
                h.len() == 2
                    && m.len() == 2
                    && h.parse::<u32>().is_ok_and(|h| h < 24)
                    && m.parse::<u32>().is_ok_and(|m| m < 60)
            })
        {
            return Err("Enter a time as HH:MM, or leave it empty for all day".into());
        }
        Ok(
            json!({"id":format!("event-{}",time::epoch_ms()),"title":title.chars().take(80).collect::<String>(),
            "date":date.key(),"time":time,"color":self.color.min(5)}),
        )
    }
}

pub struct Model {
    pub scroll: f32,
    pub scroll_max: Cell<f32>,
    pub scroll_page: Cell<f32>,
    pub focus: Option<mypaper_ui::HitId>,
    pub hover: Option<mypaper_ui::HitId>,
    pub notice: Option<String>,
    pub pressed: Option<mypaper_ui::HitId>,
    pub action_at: Option<Instant>,
    pub action_hit: Option<mypaper_ui::HitId>,
    pub flip_at: Option<Instant>,
    pub last_clock: Option<Stamp>,
    pub previous_clock: Option<Stamp>,
    pub placed: Placed,
    pub kind: Widget,
    pub data: Value,
    pub error: Option<String>,
    pub note: String,
    pub tasks: Vec<Task>,
    pub timer: Timer,
    pub events: Vec<Value>,
    pub event_draft: Option<EventDraft>,
    pub month: Date,
    pub selected: Date,
    pub stats: Option<SystemStats>,
    pub history: BTreeMap<String, VecDeque<f64>>,
    pub counts: BTreeMap<String, u32>,
    pub photo: Photo,
    pub icons: HashMap<usize, Arc<Image>>,
    pub artwork: Option<Arc<Image>>,
    pub artwork_backdrop: Option<Arc<Image>>,
    pub artwork_tint: Option<mypaper_ui::Rgba>,
    pub spectrum: Vec<f32>,
    pub field: Option<TextField>,
    pub field_keyboard: bool,
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
    /// Session completion credits move with the task when inserting/removing list entries.
    pub fn insert_counted(&mut self, at: usize) {
        self.counted = self
            .counted
            .drain()
            .map(|(index, day)| (index + usize::from(index >= at), day))
            .collect();
    }
    pub fn remove_counted(&mut self, at: usize) {
        self.counted = self
            .counted
            .drain()
            .filter_map(|(index, day)| {
                (index != at).then(|| (index - usize::from(index > at), day))
            })
            .collect();
    }
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
            scroll: 0.0,
            scroll_max: Cell::new(0.0),
            scroll_page: Cell::new(100.0),
            focus: None,
            hover: None,
            notice: None,
            pressed: None,
            action_at: None,
            action_hit: None,
            flip_at: None,
            last_clock: None,
            previous_clock: None,
            placed,
            kind,
            data,
            error: None,
            note,
            tasks,
            timer,
            events,
            event_draft: None,
            month: Date { day: 1, ..now.date },
            selected: now.date,
            stats: None,
            history: BTreeMap::new(),
            counts: BTreeMap::new(),
            photo: Photo::default(),
            icons: HashMap::new(),
            artwork: None,
            artwork_backdrop: None,
            artwork_tint: None,
            spectrum: Vec::new(),
            field: None,
            field_keyboard: false,
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
    pub fn stats_interval(&self) -> Duration {
        let value = self.str("interval", "2000");
        Duration::from_millis(match value.as_str() {
            "1000" => 1000,
            "5000" => 5000,
            _ => 2000,
        })
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
                // Interpolate displayed progress; the shared provider remains event-driven.
                Widget::Media => (self.flag("progress", true)
                    && self.data["playing"].as_bool() == Some(true)
                    && self.data["duration"]
                        .as_f64()
                        .is_some_and(|duration| duration > 0.0))
                .then(|| instant + time::boundary(now.ms, 1000)),
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
                Widget::Rss if self.flag("showTime", true) => self.data["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item["published"].as_i64())
                    .filter_map(|published| {
                        let age = (now.ms / 1000 - published).max(0);
                        let quantum = match age {
                            0..3600 => 60_000,
                            3600..86400 => 3_600_000,
                            86400..604800 => 86_400_000,
                            _ => return None,
                        };
                        Some(instant + time::boundary(now.ms - published * 1000, quantum))
                    })
                    .min(),
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
