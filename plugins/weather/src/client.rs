//! The real [`Source`]: open-meteo over HTTPS (no API key).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::location::{Location, Locator, NativeLocation};
use crate::rpc::RpcError;
use crate::service::Source;
use crate::weather::{
    Bundle, Candidate, Place, RawForecast, RawGeocoding, UnitSystem, map_forecast, rank_places,
    split_city, weather_coordinate,
};

const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

const CURRENT_FIELDS: &str = "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m,wind_direction_10m";
const DAILY_FIELDS: &str =
    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max";
const HOURLY_FIELDS: &str = "temperature_2m,weather_code,precipitation_probability,is_day";

pub struct OpenMeteo {
    agent: crate::http::Http,
    locator: Locator<NativeLocation>,
    searches: Mutex<HashMap<String, (Instant, Vec<Candidate>)>>,
}

impl Default for OpenMeteo {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenMeteo {
    pub fn new() -> Self {
        let agent = crate::http::Http::new(Duration::from_secs(8));
        let locator = Locator::new(NativeLocation::new());
        if let Some(app_data) = std::env::var_os("APPDATA") {
            locator.set_data_dir(PathBuf::from(app_data).join("mypaper/plugin-data/weather"));
        }
        Self {
            agent,
            locator,
            searches: Mutex::default(),
        }
    }

    fn candidates(&self, query: &str) -> Result<Vec<Candidate>, RpcError> {
        let (name, _) = split_city(query);
        if name.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let key = query.to_lowercase();
        let mut cache = self.searches.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((fetched, hits)) = cache.get(&key)
            && fetched.elapsed() < Duration::from_secs(60 * 60)
        {
            return Ok(hits.clone());
        }
        // Fetch enough homonyms for admin/country disambiguation, then return at most eight.
        let raw: RawGeocoding = self
            .agent
            .get(GEOCODING_URL)
            .query("name", name)
            .query("count", "100")
            .query("language", "en")
            .query("format", "json")
            .json()
            .map_err(|e| RpcError::internal(format!("unexpected geocoding response: {e}")))?;
        let hits = rank_places(raw, query);
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), hits.clone()));
        Ok(hits)
    }
}

impl Source for OpenMeteo {
    fn initialize(&self, data_dir: PathBuf) {
        self.locator.set_data_dir(data_dir);
    }

    fn locate(&self) -> Result<Location, RpcError> {
        self.locator.locate()
    }

    fn search(&self, query: &str) -> Result<Vec<Candidate>, RpcError> {
        Ok(self.candidates(query)?.into_iter().take(8).collect())
    }

    fn geocode(&self, city: &str) -> Result<Place, RpcError> {
        self.candidates(city)?
            .first()
            .map(Candidate::place)
            .ok_or_else(|| RpcError::not_found(format!("no place called \"{city}\"")))
    }

    fn forecast(&self, place: &Place, units: UnitSystem) -> Result<Bundle, RpcError> {
        let (temperature, wind, precipitation) = match units {
            UnitSystem::Metric => ("celsius", "kmh", "mm"),
            UnitSystem::Imperial => ("fahrenheit", "mph", "inch"),
        };
        let raw: RawForecast = self
            .agent
            .get(FORECAST_URL)
            .query("latitude", weather_coordinate(place.lat))
            .query("longitude", weather_coordinate(place.lon))
            .query("current", CURRENT_FIELDS)
            .query("daily", DAILY_FIELDS)
            .query("hourly", HOURLY_FIELDS)
            .query("timezone", "auto")
            .query("forecast_days", crate::weather::MAX_DAYS.to_string())
            .query("temperature_unit", temperature)
            .query("wind_speed_unit", wind)
            .query("precipitation_unit", precipitation)
            .json()
            .map_err(|e| RpcError::internal(format!("unexpected forecast response: {e}")))?;
        map_forecast(place, units, raw)
    }
}
