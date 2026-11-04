//! One sleeping worker lane for built-in fetches and file reads. Never an async runtime or a
//! periodic poller: the UI sends work only when a visible instance's deadline expires.
use super::model::{Widget, number, string};
use mypaper_plugin_weather::{client::OpenMeteo, http::Http, service::Service};
use mypaper_widgets::feed;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub enum Work {
    Fetch {
        instance: String,
        generation: u64,
        kind: Widget,
        settings: Map<String, Value>,
    },
    Photo {
        instance: String,
        generation: u64,
        path: PathBuf,
    },
    Launch(String),
}
pub enum Answer {
    Data {
        instance: String,
        generation: u64,
        result: Result<Value, String>,
    },
    Photo {
        instance: String,
        generation: u64,
        result: Result<Vec<u8>, String>,
    },
}
pub struct Worker {
    tx: Option<mpsc::Sender<Work>>,
    rx: mpsc::Receiver<Answer>,
    answers: mpsc::Sender<Answer>,
    window: isize,
    held: Arc<AtomicBool>,
}
impl Worker {
    pub fn new(window: HWND) -> Self {
        let (answers, rx) = mpsc::channel();
        Self {
            tx: None,
            rx,
            answers,
            window: window.0 as isize,
            held: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn hold(&self, held: bool) {
        self.held.store(held, Ordering::Relaxed);
    }
    pub fn next(&self) -> Option<Answer> {
        self.rx.try_recv().ok()
    }
    pub fn send(&mut self, work: Work) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Work>();
            let answers = self.answers.clone();
            let window = self.window;
            let held = self.held.clone();
            std::thread::Builder::new()
                .name("widget-data".into())
                .spawn(move || {
                    let _apartment =
                        mypaper_plugin_weather::location::native::Apartment::new().ok();
                    let weather = Service::new(OpenMeteo::new(), Duration::from_secs(600));
                    if let Ok(data_dir) = mypaper_widgets::files::user("plugin-data/weather") {
                        let _ = weather.initialize(&json!({"dataDir":data_dir}));
                    }
                    let http = Http::new(Duration::from_secs(8));
                    for work in rx {
                        let answer = match work {
                            Work::Fetch {
                                instance,
                                generation,
                                kind,
                                settings,
                            } => {
                                let result = if held.load(Ordering::Relaxed) {
                                    Err("Paused".into())
                                } else {
                                    match kind {
                                        Widget::Weather => fetch_weather(&weather, &settings),
                                        Widget::Crypto => fetch_crypto(&http, &settings),
                                        Widget::Rss => fetch_rss(&http, &settings),
                                        _ => Err("unsupported fetch".into()),
                                    }
                                };
                                Answer::Data {
                                    instance,
                                    generation,
                                    result,
                                }
                            }
                            Work::Photo {
                                instance,
                                generation,
                                path,
                            } => {
                                let result = std::fs::metadata(&path)
                                    .map_err(|e| e.to_string())
                                    .and_then(|m| {
                                        if m.len() > mypaper_widgets::pick::MAX_BYTES {
                                            Err("Photo exceeds 64 MB".into())
                                        } else {
                                            std::fs::read(path).map_err(|e| e.to_string())
                                        }
                                    });
                                Answer::Photo {
                                    instance,
                                    generation,
                                    result,
                                }
                            }
                            Work::Launch(target) => {
                                launch(&target);
                                continue;
                            }
                        };
                        if answers.send(answer).is_err() {
                            break;
                        }
                        // SAFETY: wake only; the queue owns all returned data.
                        let _ = unsafe {
                            PostMessageW(
                                Some(HWND(window as *mut _)),
                                super::WM_DATA,
                                WPARAM(0),
                                LPARAM(0),
                            )
                        };
                    }
                })
                .expect("widget worker");
            tx
        });
        let _ = tx.send(work);
    }
}
fn fetch_weather(
    service: &Service<OpenMeteo>,
    settings: &Map<String, Value>,
) -> Result<Value, String> {
    let mut params = Map::new();
    params.insert(
        "units".into(),
        Value::from(string(settings, "units", "metric")),
    );
    match settings.get("city") {
        Some(Value::Object(place)) if place.get("lat").and_then(Value::as_f64).is_some() => {
            params.extend(place.clone())
        }
        Some(Value::Object(place)) => {
            if let Some(name) = place.get("name") {
                params.insert("city".into(), name.clone());
            }
        }
        Some(Value::String(city)) if !city.is_empty() => {
            params.insert("city".into(), json!(city));
        }
        _ => {}
    }
    let params = Value::Object(params);
    let current = service.call("current", &params).map_err(|e| e.message)?;
    let forecast = service.call("forecast", &params).map_err(|e| e.message)?;
    Ok(json!({"current":current,"forecast":forecast}))
}
fn fetch_crypto(http: &Http, settings: &Map<String, Value>) -> Result<Value, String> {
    let ids: Vec<_> = settings
        .get("coins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .take(6)
        .collect();
    if ids.is_empty() {
        return Ok(json!({"coins":[]}));
    }
    let currency = string(settings, "currency", "usd");
    let raw: Value = http
        .get("https://api.coingecko.com/api/v3/simple/price")
        .query("ids", ids.join(","))
        .query("vs_currencies", currency)
        .query("include_24hr_change", "true")
        .json()?;
    let coins:Vec<_>=ids.iter().filter_map(|id|Some(json!({"id":id,"name":coin_name(id),"price":raw[*id][currency].as_f64()?,"change":raw[*id][format!("{currency}_24h_change")]}))).collect();
    let data:Map<String,Value>=ids.iter().map(|id|((*id).to_owned(),json!({"price":raw[*id][currency],"change":raw[*id][format!("{currency}_24h_change")]}))).collect();
    Ok(
        json!({"key":format!("{}|{currency}",ids.join(",")),"at":super::time::epoch_ms(),"data":data,"coins":coins}),
    )
}
fn coin_name(id: &str) -> String {
    let mut text = id.replace('-', " ");
    if let Some(first) = text.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    text
}
fn fetch_rss(http: &Http, settings: &Map<String, Value>) -> Result<Value, String> {
    let urls: Vec<_> = settings
        .get("feeds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v["url"].as_str())
        .take(8)
        .collect();
    let mut items = Vec::new();
    let mut errors = Vec::new();
    for url in &urls {
        let result = http
            .get(url)
            .bytes()
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
            .and_then(|xml| feed::parse(&xml).map_err(|e| e.to_string()));
        match result {
            Ok(feed) => {
                for entry in feed.entries {
                    items.push(json!({"title":entry.title,"link":entry.link,"source":feed.title.as_deref().unwrap_or(url),"published":entry.published,"summary":entry.summary}));
                }
            }
            Err(error) => errors.push(json!({"url":url,"message":error})),
        }
    }
    if items.is_empty() && !errors.is_empty() {
        return Err("Could not reach your feeds".into());
    }
    items.sort_by_key(|i| std::cmp::Reverse(i["published"].as_i64().unwrap_or(0)));
    items.dedup_by(|a, b| a["link"] == b["link"] && a["title"] == b["title"]);
    items.truncate(number(settings, "maxItems", 12.0).clamp(3.0, 30.0) as usize);
    Ok(json!({"key":urls.join("|"),"items":items,"errors":errors,"at":super::time::epoch_ms()}))
}
fn launch(target: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    // SAFETY: shell extension activation is deliberately isolated from the UI thread.
    unsafe {
        let result = ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
        if result.0 as isize <= 32 {
            log::warn!("launch failed: {target}");
        }
    }
}
