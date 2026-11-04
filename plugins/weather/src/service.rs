//! The plugin's logic: resolve a place, serve `current` / `forecast` from a time-limited cache,
//! and refresh stale entries. Network access is behind [`Source`], so this is tested with a fake.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::rpc::RpcError;
use crate::weather::{Bundle, Place, Request, UnitSystem, Where, parse_request};

/// Where weather data comes from.
pub trait Source: Send + Sync {
    /// Resolves a city name (optionally "Name, region or country"). `NotFound` if there is none.
    fn geocode(&self, city: &str) -> Result<Place, RpcError>;
    fn forecast(&self, place: &Place, units: UnitSystem) -> Result<Bundle, RpcError>;
}

/// A place and unit system, with coordinates rounded to ~10 m so that equal requests share a slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    lat: i64,
    lon: i64,
    units: UnitSystem,
}

impl Key {
    fn new(place: &Place, units: UnitSystem) -> Self {
        let round = |degrees: f64| (degrees * 1e4).round() as i64;
        Self {
            lat: round(place.lat),
            lon: round(place.lon),
            units,
        }
    }
}

struct Entry {
    place: Place,
    bundle: Arc<Bundle>,
    fetched: Instant,
}

pub struct Service<S: Source> {
    source: S,
    ttl: Duration,
    /// Resolved cities, by lowercase query. Places don't move, so these never expire.
    cities: Mutex<HashMap<String, Place>>,
    cache: Mutex<HashMap<Key, Entry>>,
    /// One lock per key, so concurrent calls for the same data make one request.
    flights: Mutex<HashMap<Key, Arc<Mutex<()>>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl<S: Source> Service<S> {
    pub fn new(source: S, ttl: Duration) -> Self {
        Self {
            source,
            ttl,
            cities: Mutex::default(),
            cache: Mutex::default(),
            flights: Mutex::default(),
        }
    }

    /// Handles `current` and `forecast`.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method != "current" && method != "forecast" {
            return Err(RpcError::method_not_found(method));
        }
        let request = parse_request(params)?;
        let place = self.resolve(&request.place)?;
        let bundle = self.bundle(&place, &request)?;
        let value = if method == "current" {
            serde_json::to_value(&bundle.current)
        } else {
            let mut forecast = bundle.forecast.clone();
            forecast.days.truncate(request.days);
            serde_json::to_value(&forecast)
        };
        value.map_err(|e| RpcError::internal(e.to_string()))
    }

    fn resolve(&self, place: &Where) -> Result<Place, RpcError> {
        match place {
            Where::Coords { lat, lon } => Ok(Place {
                name: format!("{lat:.2}, {lon:.2}"),
                country: None,
                lat: *lat,
                lon: *lon,
            }),
            Where::City(city) => {
                let key = city.to_lowercase();
                if let Some(place) = lock(&self.cities).get(&key) {
                    return Ok(place.clone());
                }
                let place = self.source.geocode(city)?;
                lock(&self.cities).insert(key, place.clone());
                Ok(place)
            }
        }
    }

    fn flight(&self, key: &Key) -> Arc<Mutex<()>> {
        Arc::clone(lock(&self.flights).entry(key.clone()).or_default())
    }

    fn fresh(&self, key: &Key) -> Option<Arc<Bundle>> {
        let cache = lock(&self.cache);
        let entry = cache.get(key)?;
        (entry.fetched.elapsed() < self.ttl).then(|| Arc::clone(&entry.bundle))
    }

    fn fetch(&self, key: Key, place: &Place, units: UnitSystem) -> Result<Arc<Bundle>, RpcError> {
        let bundle = Arc::new(self.source.forecast(place, units)?);
        lock(&self.cache).insert(
            key,
            Entry {
                place: place.clone(),
                bundle: Arc::clone(&bundle),
                fetched: Instant::now(),
            },
        );
        Ok(bundle)
    }

    fn bundle(&self, place: &Place, request: &Request) -> Result<Arc<Bundle>, RpcError> {
        let key = Key::new(place, request.units);
        if let Some(hit) = self.fresh(&key) {
            return Ok(hit);
        }
        let flight = self.flight(&key);
        let _turn = lock(&flight);
        // Whoever held the lock before us has probably filled the cache.
        if let Some(hit) = self.fresh(&key) {
            return Ok(hit);
        }
        self.fetch(key, place, request.units)
    }

    /// Entries past their time limit, for the background refresh.
    pub fn stale(&self) -> Vec<(Place, UnitSystem)> {
        lock(&self.cache)
            .iter()
            .filter(|(_, e)| e.fetched.elapsed() >= self.ttl)
            .map(|(key, e)| (e.place.clone(), key.units))
            .collect()
    }

    /// Re-fetches an entry if it is still stale. Returns whether the data changed.
    pub fn refresh(&self, place: &Place, units: UnitSystem) -> Result<bool, RpcError> {
        let key = Key::new(place, units);
        let flight = self.flight(&key);
        let _turn = lock(&flight);
        if self.fresh(&key).is_some() {
            return Ok(false); // another refresh got there first
        }
        let before = lock(&self.cache).get(&key).map(|e| Arc::clone(&e.bundle));
        let after = self.fetch(key, place, units)?;
        Ok(before.is_none_or(|b| *b != *after))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use serde_json::json;

    use super::*;
    use crate::weather::{Current, Day, Forecast, Units};

    /// Counts calls; every fetch yields a different temperature.
    #[derive(Default)]
    struct Fake {
        geocodes: AtomicUsize,
        forecasts: AtomicUsize,
        delay: Duration,
    }

    impl Source for Fake {
        fn geocode(&self, city: &str) -> Result<Place, RpcError> {
            self.geocodes.fetch_add(1, Ordering::SeqCst);
            if city.eq_ignore_ascii_case("oslo") {
                Ok(Place {
                    name: "Oslo".into(),
                    country: Some("Norway".into()),
                    lat: 59.91,
                    lon: 10.75,
                })
            } else {
                Err(RpcError::not_found(format!("no place called \"{city}\"")))
            }
        }

        fn forecast(&self, place: &Place, units: UnitSystem) -> Result<Bundle, RpcError> {
            thread::sleep(self.delay);
            let n = self.forecasts.fetch_add(1, Ordering::SeqCst) as f64;
            let unit_labels = Units {
                temperature: units.as_str().into(),
                speed: "".into(),
                precipitation: "".into(),
            };
            let day = |i: usize| Day {
                date: format!("2026-11-{:02}", 3 + i),
                code: 0,
                condition: "Clear sky",
                icon: "clear-day",
                min: 0.0,
                max: 1.0,
                precipitation_probability: None,
            };
            Ok(Bundle {
                current: Current {
                    location: place.clone(),
                    units: unit_labels.clone(),
                    time: "t".into(),
                    temperature: n,
                    apparent: n,
                    humidity: 50.0,
                    wind: 0.0,
                    wind_direction: 0.0,
                    precipitation: 0.0,
                    code: 0,
                    condition: "Clear sky",
                    icon: "clear-day",
                    is_day: true,
                },
                forecast: Forecast {
                    location: place.clone(),
                    units: unit_labels,
                    days: (0..7).map(day).collect(),
                    hourly: Vec::new(),
                },
            })
        }
    }

    const TEN_MIN: Duration = Duration::from_secs(600);

    #[test]
    fn current_and_forecast_share_one_fetch_and_the_geocode_is_remembered() {
        let service = Service::new(Fake::default(), TEN_MIN);
        let current = service.call("current", &json!({ "city": "Oslo" })).unwrap();
        service
            .call("forecast", &json!({ "city": "oslo" }))
            .unwrap();
        service.call("current", &json!({ "city": "Oslo" })).unwrap();
        assert_eq!(current["location"]["name"], "Oslo");
        assert_eq!(service.source.forecasts.load(Ordering::SeqCst), 1);
        assert_eq!(service.source.geocodes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn units_and_places_are_cached_separately() {
        let service = Service::new(Fake::default(), TEN_MIN);
        service
            .call("current", &json!({ "lat": 59.91, "lon": 10.75 }))
            .unwrap();
        service
            .call(
                "current",
                &json!({ "lat": 59.91, "lon": 10.75, "units": "imperial" }),
            )
            .unwrap();
        service
            .call("current", &json!({ "lat": 40.0, "lon": -3.7 }))
            .unwrap();
        assert_eq!(service.source.forecasts.load(Ordering::SeqCst), 3);
        // Coordinates differing below ~10 m share a slot.
        service
            .call("current", &json!({ "lat": 59.910001, "lon": 10.750001 }))
            .unwrap();
        assert_eq!(service.source.forecasts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn entries_expire() {
        let service = Service::new(Fake::default(), Duration::ZERO);
        service.call("current", &json!({ "city": "Oslo" })).unwrap();
        service.call("current", &json!({ "city": "Oslo" })).unwrap();
        assert_eq!(service.source.forecasts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_calls_for_the_same_data_make_one_request() {
        let fake = Fake {
            delay: Duration::from_millis(80),
            ..Fake::default()
        };
        let service = Arc::new(Service::new(fake, TEN_MIN));
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let service = Arc::clone(&service);
                let method = if i % 2 == 0 { "current" } else { "forecast" };
                thread::spawn(move || {
                    service
                        .call(method, &json!({ "lat": 1.0, "lon": 2.0 }))
                        .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(service.source.forecasts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn forecast_is_cut_to_the_requested_days() {
        let service = Service::new(Fake::default(), TEN_MIN);
        let days = |params: Value| {
            service.call("forecast", &params).unwrap()["days"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(days(json!({ "city": "Oslo" })), 5);
        assert_eq!(days(json!({ "city": "Oslo", "days": 2 })), 2);
        assert_eq!(days(json!({ "city": "Oslo", "days": 7 })), 7);
    }

    #[test]
    fn errors_have_the_right_codes() {
        let service = Service::new(Fake::default(), TEN_MIN);
        let nowhere = service
            .call("current", &json!({ "city": "Nowhere" }))
            .unwrap_err();
        assert_eq!(nowhere.data_code, Some("NotFound"));
        assert_eq!(
            service.call("current", &json!({})).unwrap_err().code,
            -32602
        );
        assert_eq!(
            service
                .call("hourly", &json!({ "city": "Oslo" }))
                .unwrap_err()
                .code,
            -32601
        );
        // A failed lookup isn't cached.
        service
            .call("current", &json!({ "city": "Nowhere" }))
            .unwrap_err();
        assert_eq!(service.source.geocodes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn refresh_updates_stale_entries_and_reports_a_change() {
        let service = Service::new(Fake::default(), Duration::ZERO);
        service.call("current", &json!({ "city": "Oslo" })).unwrap();
        let stale = service.stale();
        assert_eq!(stale.len(), 1);
        let (place, units) = &stale[0];
        // The fake's temperature differs on every fetch.
        assert!(service.refresh(place, *units).unwrap());

        let fresh = Service::new(Fake::default(), TEN_MIN);
        fresh.call("current", &json!({ "city": "Oslo" })).unwrap();
        assert!(fresh.stale().is_empty());
        // Not stale any more: nothing to do, no change.
        assert!(!fresh.refresh(&place.clone(), UnitSystem::Metric).unwrap());
        assert_eq!(fresh.source.forecasts.load(Ordering::SeqCst), 1);
    }
}
