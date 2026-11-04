//! The real [`Source`]: open-meteo over HTTPS (no API key).

use std::time::Duration;

use crate::rpc::RpcError;
use crate::service::Source;
use crate::weather::{
    Bundle, Place, RawForecast, RawGeocoding, UnitSystem, map_forecast, pick_place, split_city,
};

const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

const CURRENT_FIELDS: &str = "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m,wind_direction_10m";
const DAILY_FIELDS: &str =
    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max";
const HOURLY_FIELDS: &str = "temperature_2m,weather_code,precipitation_probability,is_day";

pub struct OpenMeteo {
    agent: ureq::Agent,
}

impl OpenMeteo {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .user_agent("mypaper-weather/1.0")
            .build()
            .into();
        Self { agent }
    }
}

fn unreachable(e: &ureq::Error) -> RpcError {
    RpcError::unavailable(format!("weather service unreachable: {e}"))
}

impl Source for OpenMeteo {
    fn geocode(&self, city: &str) -> Result<Place, RpcError> {
        let (name, hint) = split_city(city);
        // With a hint ("Paris, Texas") we need more than the top match to choose from.
        let count = if hint.is_some() { "10" } else { "1" };
        let raw: RawGeocoding = self
            .agent
            .get(GEOCODING_URL)
            .query("name", name)
            .query("count", count)
            .query("language", "en")
            .query("format", "json")
            .call()
            .map_err(|e| unreachable(&e))?
            .body_mut()
            .read_json()
            .map_err(|e| RpcError::internal(format!("unexpected geocoding response: {e}")))?;
        pick_place(raw, hint)
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
            .query("latitude", place.lat.to_string())
            .query("longitude", place.lon.to_string())
            .query("current", CURRENT_FIELDS)
            .query("daily", DAILY_FIELDS)
            .query("hourly", HOURLY_FIELDS)
            .query("timezone", "auto")
            .query("forecast_days", crate::weather::MAX_DAYS.to_string())
            .query("temperature_unit", temperature)
            .query("wind_speed_unit", wind)
            .query("precipitation_unit", precipitation)
            .call()
            .map_err(|e| unreachable(&e))?
            .body_mut()
            .read_json()
            .map_err(|e| RpcError::internal(format!("unexpected forecast response: {e}")))?;
        map_forecast(place, units, raw)
    }
}
