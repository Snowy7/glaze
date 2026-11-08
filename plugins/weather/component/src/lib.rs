//! Weather as a WebAssembly component: locates by IP (or the face's `latitude`/`longitude`
//! settings), reads open-meteo, and draws the current conditions with the next 12 hours. It
//! refreshes every 15 minutes while a face is placed and keeps the last forecast across unloads.
use std::collections::BTreeMap;

use glaze_plugin_sdk::guest::{self, Event, FaceOpen, Plugin, SizeClass, timers, ui};
use glaze_plugin_sdk::{Align, Role, Tone, View};
use serde::{Deserialize, Serialize};

const REFRESH_MS: u32 = 15 * 60 * 1000;

#[derive(Default, Clone, Serialize, Deserialize)]
struct Place {
    latitude: f64,
    longitude: f64,
    name: String,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Forecast {
    temperature: f32,
    code: u32,
    day: bool,
    high: f32,
    low: f32,
    hours: Vec<f32>,
    /// open-meteo's local time of the reading, `YYYY-MM-DDTHH:MM`.
    time: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    place: Option<Place>,
    forecast: Option<Forecast>,
}

#[derive(Default)]
struct Weather {
    saved: Saved,
    faces: BTreeMap<String, SizeClass>,
    fahrenheit: bool,
    locating: Option<u64>,
    fetching: Option<u64>,
    error: Option<String>,
}

fn condition(code: u32, day: bool) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", if day { "sun" } else { "moon" }),
        1 | 2 => ("Partly cloudy", if day { "cloud-sun" } else { "cloud-moon" }),
        3 => ("Cloudy", "cloud"),
        45 | 48 => ("Fog", "cloud-fog"),
        51..=57 => ("Drizzle", "cloud-drizzle"),
        61..=67 | 80..=82 => ("Rain", "cloud-rain"),
        71..=77 | 85 | 86 => ("Snow", "cloud-snow"),
        95..=99 => ("Thunderstorm", "cloud-lightning"),
        _ => ("—", "cloud"),
    }
}

impl Weather {
    fn refresh(&mut self) {
        let request = match &self.saved.place {
            Some(place) => {
                let unit = if self.fahrenheit { "&temperature_unit=fahrenheit" } else { "" };
                let url = format!(
                    "https://api.open-meteo.com/v1/forecast?latitude={:.3}&longitude={:.3}&current=temperature_2m,weather_code,is_day&hourly=temperature_2m&daily=temperature_2m_max,temperature_2m_min&forecast_days=2&timezone=auto{unit}",
                    place.latitude, place.longitude
                );
                guest::get(&url).map(|id| self.fetching = Some(id))
            }
            None => guest::get("https://ipwho.is/").map(|id| self.locating = Some(id)),
        };
        if let Err(e) = request {
            self.error = Some(format!("{e:?}"));
        }
    }

    fn open(&mut self, open: &FaceOpen) {
        let settings = guest::settings(open);
        self.fahrenheit = settings["units"].as_str() == Some("fahrenheit");
        if let (Some(latitude), Some(longitude)) = (settings["latitude"].as_f64(), settings["longitude"].as_f64()) {
            let name = settings["name"].as_str().unwrap_or("").to_owned();
            self.saved.place = Some(Place { latitude, longitude, name });
        }
        if self.faces.is_empty() {
            let _ = timers::every("refresh", REFRESH_MS);
        }
        self.faces.insert(open.instance.clone(), open.size);
        if self.fetching.is_none() && self.locating.is_none() {
            self.refresh();
        }
        self.draw(&open.instance, open.size);
    }

    fn view(&self, size: SizeClass) -> View {
        let Some(now) = &self.saved.forecast else {
            let detail = self.error.clone().unwrap_or_else(|| "Getting the forecast…".into());
            return View::empty("cloud-sun", "Weather", detail);
        };
        let (label, icon) = condition(now.code, now.day);
        let degrees = |t: f32| format!("{}°", t.round() as i32);
        let place = self.saved.place.as_ref().map(|p| p.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Here".into());
        let small = matches!(size, SizeClass::Small);
        let top = View::row([
            View::Icon { icon: icon.into(), size: Some(if small { 22.0 } else { 30.0 }), color: None },
            View::Text { text: degrees(now.temperature), role: Role::LargeTitle, color: None, align: Align::Start },
            View::Spacer,
        ]);
        if small {
            return View::stack([top, View::tinted(label, Role::Caption, Tone::Secondary)]);
        }
        View::stack([
            View::tinted(place, Role::Subheadline, Tone::Secondary),
            top,
            View::text(label, Role::Headline),
            View::tinted(format!("H {}  L {}", degrees(now.high), degrees(now.low)), Role::Caption, Tone::Secondary),
            View::Spacer,
            View::Sparkline { values: now.hours.clone(), color: Some(Tone::Accent) },
        ])
    }

    fn draw(&self, instance: &str, size: SizeClass) {
        let mut face = self.view(size).face();
        face.refresh = true;
        face.footer = self.saved.forecast.as_ref().map(|f| format!("Updated {}", f.time.get(11..16).unwrap_or("")));
        ui::set_view(instance, face);
    }

    fn redraw(&self) {
        for (instance, size) in &self.faces {
            self.draw(instance, *size);
        }
    }

    fn on_reply(&mut self, id: u64, body: Result<Vec<u8>, String>) {
        let json = body.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string()));
        if self.locating == Some(id) {
            self.locating = None;
            match json {
                Ok(v) if v["latitude"].is_number() => {
                    let name = v["city"].as_str().unwrap_or("").to_owned();
                    self.saved.place = Some(Place { latitude: v["latitude"].as_f64().unwrap_or(0.0), longitude: v["longitude"].as_f64().unwrap_or(0.0), name });
                    self.refresh();
                }
                Ok(_) => self.error = Some("Couldn't find where you are".into()),
                Err(e) => self.error = Some(e),
            }
        } else if self.fetching == Some(id) {
            self.fetching = None;
            match json {
                Ok(v) => {
                    let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
                    let time = v["current"]["time"].as_str().unwrap_or("").to_owned();
                    let hourly = v["hourly"]["time"].as_array().cloned().unwrap_or_default();
                    let start = hourly.iter().position(|t| t.as_str().is_some_and(|t| t >= time.as_str())).unwrap_or(0);
                    let temps = v["hourly"]["temperature_2m"].as_array().cloned().unwrap_or_default();
                    self.saved.forecast = Some(Forecast {
                        temperature: f(&v["current"]["temperature_2m"]),
                        code: v["current"]["weather_code"].as_u64().unwrap_or(0) as u32,
                        day: v["current"]["is_day"].as_u64() != Some(0),
                        high: f(&v["daily"]["temperature_2m_max"][0]),
                        low: f(&v["daily"]["temperature_2m_min"][0]),
                        hours: temps.iter().skip(start).take(12).map(f).collect(),
                        time,
                    });
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        self.redraw();
    }
}

impl Plugin for Weather {
    fn init(&mut self, _info: &guest::InitInfo, state: Option<&[u8]>) -> Result<(), String> {
        if let Some(saved) = state.and_then(|s| serde_json::from_slice(s).ok()) {
            self.saved = saved;
        }
        Ok(())
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => self.open(&open),
            Event::Close(instance) => {
                self.faces.remove(&instance);
                if self.faces.is_empty() {
                    timers::cancel("refresh");
                }
            }
            Event::Timer(_) => self.refresh(),
            Event::Action(action) if action.action == "refresh" => {
                self.refresh();
                self.redraw();
            }
            Event::Http(reply) => {
                let body = match reply.outcome {
                    Ok(r) if (200..300).contains(&r.status) => Ok(r.body),
                    Ok(r) => Err(format!("HTTP {}", r.status)),
                    Err(e) => Err(format!("{e:?}")),
                };
                self.on_reply(reply.id, body);
            }
            _ => {}
        }
    }

    fn save(&mut self) -> Vec<u8> {
        serde_json::to_vec(&self.saved).unwrap_or_default()
    }
}

glaze_plugin_sdk::export!(Weather);
