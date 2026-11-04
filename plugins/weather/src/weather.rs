//! Request parsing and the mapping from open-meteo's responses to this plugin's results.
//! No I/O here, so all of it is unit-tested.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rpc::RpcError;

// ------------------------------------------------------------------ request

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UnitSystem {
    #[default]
    Metric,
    Imperial,
}

impl UnitSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Imperial => "imperial",
        }
    }

    fn labels(self) -> Units {
        match self {
            Self::Metric => Units::new("\u{b0}C", "km/h", "mm"),
            Self::Imperial => Units::new("\u{b0}F", "mph", "in"),
        }
    }
}

/// Where to get the weather for.
#[derive(Debug, Clone, PartialEq)]
pub enum Where {
    Auto,
    Coords(Place),
    City(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub place: Where,
    pub units: UnitSystem,
    /// Forecast days wanted (`forecast` only).
    pub days: usize,
}

pub const MAX_DAYS: usize = 7;
const DEFAULT_DAYS: usize = 5;
/// How many hourly entries `forecast` returns.
pub const HOURS: usize = 24;

/// Missing location means auto. Explicit coordinates win over city, then auto.
pub fn parse_request(params: &Value) -> Result<Request, RpcError> {
    validate_object(params)?;
    if params.get("auto").is_some_and(|v| !v.is_boolean()) {
        return Err(RpcError::invalid_params("\"auto\" must be a boolean"));
    }
    let field = |name: &str| params.get(name).filter(|v| !v.is_null());
    let number = |name: &str| -> Result<Option<f64>, RpcError> {
        field(name)
            .map(|v| {
                v.as_f64()
                    .ok_or_else(|| RpcError::invalid_params(format!("\"{name}\" must be a number")))
            })
            .transpose()
    };

    let place = match (number("lat")?, number("lon")?) {
        (Some(lat), Some(lon)) => {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(RpcError::invalid_params(
                    "lat must be -90..90 and lon -180..180",
                ));
            }
            Where::Coords(Place {
                name: optional_text(params, "name")?
                    .unwrap_or_else(|| format!("{lat:.2}, {lon:.2}")),
                region: optional_text(params, "region")?,
                country: optional_text(params, "country")?,
                lat,
                lon,
                ..Place::default()
            })
        }
        (None, None) => match field("city") {
            Some(_) => Where::City(
                optional_text(params, "city")?
                    .ok_or_else(|| RpcError::invalid_params("\"city\" must not be empty"))?,
            ),
            None if params["auto"] == false => {
                return Err(RpcError::invalid_params(
                    "pass { lat, lon }, { city }, or { auto: true }",
                ));
            }
            None => Where::Auto,
        },
        _ => return Err(RpcError::invalid_params("\"lat\" and \"lon\" go together")),
    };

    let units = match field("units").map(Value::as_str) {
        None | Some(Some("metric")) => UnitSystem::Metric,
        Some(Some("imperial")) => UnitSystem::Imperial,
        Some(_) => {
            return Err(RpcError::invalid_params(
                "\"units\" is \"metric\" or \"imperial\"",
            ));
        }
    };

    let days = match field("days") {
        None => DEFAULT_DAYS,
        Some(v) => v
            .as_u64()
            .filter(|d| (1..=MAX_DAYS as u64).contains(d))
            .ok_or_else(|| RpcError::invalid_params(format!("\"days\" is 1 to {MAX_DAYS}")))?
            as usize,
    };

    Ok(Request { place, units, days })
}

pub fn validate_object(params: &Value) -> Result<(), RpcError> {
    if !params.is_null() && !params.is_object() {
        return Err(RpcError::invalid_params("params must be an object"));
    }
    Ok(())
}

fn optional_text(params: &Value, field: &str) -> Result<Option<String>, RpcError> {
    let Some(value) = params.get(field).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .filter(|v| v.len() <= 200)
        .ok_or_else(|| {
            RpcError::invalid_params(format!("\"{field}\" must be a string of at most 200 bytes"))
        })?
        .trim();
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

pub fn parse_query(params: &Value) -> Result<String, RpcError> {
    validate_object(params)?;
    optional_text(params, "query")?
        .ok_or_else(|| RpcError::invalid_params("pass a nonempty \"query\" string"))
}

/// Only these rounded coordinates are sent to the forecast endpoint.
pub fn weather_coordinate(degrees: f64) -> String {
    format!("{degrees:.2}")
}

// ------------------------------------------------------------------ results

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::location::Origin>,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Units {
    pub temperature: String,
    pub speed: String,
    pub precipitation: String,
}

impl Units {
    fn new(temperature: &str, speed: &str, precipitation: &str) -> Self {
        Self {
            temperature: temperature.into(),
            speed: speed.into(),
            precipitation: precipitation.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Current {
    pub location: Place,
    pub units: Units,
    /// Local time at the place, as open-meteo reports it ("2026-11-03T21:00").
    pub time: String,
    pub temperature: f64,
    pub apparent: f64,
    pub humidity: f64,
    pub wind: f64,
    pub wind_direction: f64,
    pub precipitation: f64,
    /// The WMO weather code.
    pub code: u8,
    pub condition: &'static str,
    /// One of: clear-day, clear-night, partly-cloudy-day, partly-cloudy-night, cloudy, fog,
    /// drizzle, rain, snow, thunderstorm.
    pub icon: &'static str,
    pub is_day: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Day {
    /// "2026-11-03", local to the place.
    pub date: String,
    pub code: u8,
    pub condition: &'static str,
    pub icon: &'static str,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precipitation_probability: Option<f64>,
}

/// One hour of the forecast.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hour {
    /// "2026-11-03T22:00", local to the place.
    pub time: String,
    pub temperature: f64,
    pub code: u8,
    pub condition: &'static str,
    /// Day or night variants of the icons that have them, from the hour's own `is_day`.
    pub icon: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precipitation_probability: Option<f64>,
    pub is_day: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Forecast {
    pub location: Place,
    pub units: Units,
    pub days: Vec<Day>,
    /// The next [`HOURS`] hours, starting with the current one.
    pub hourly: Vec<Hour>,
}

/// Everything one open-meteo call gives: serves both `current` and `forecast`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    pub current: Current,
    pub forecast: Forecast,
}

// ------------------------------------------------------------------ WMO codes

/// Condition text and icon for a WMO weather code
/// (<https://open-meteo.com/en/docs#weather_variable_documentation>).
pub fn describe(code: u8, is_day: bool) -> (&'static str, &'static str) {
    let day_night = |day: &'static str, night: &'static str| if is_day { day } else { night };
    match code {
        0 => ("Clear sky", day_night("clear-day", "clear-night")),
        1 => ("Mainly clear", day_night("clear-day", "clear-night")),
        2 => (
            "Partly cloudy",
            day_night("partly-cloudy-day", "partly-cloudy-night"),
        ),
        3 => ("Overcast", "cloudy"),
        45 | 48 => ("Fog", "fog"),
        51 => ("Light drizzle", "drizzle"),
        53 => ("Drizzle", "drizzle"),
        55 => ("Dense drizzle", "drizzle"),
        56 | 57 => ("Freezing drizzle", "drizzle"),
        61 => ("Light rain", "rain"),
        63 => ("Rain", "rain"),
        65 => ("Heavy rain", "rain"),
        66 | 67 => ("Freezing rain", "rain"),
        71 => ("Light snow", "snow"),
        73 => ("Snow", "snow"),
        75 => ("Heavy snow", "snow"),
        77 => ("Snow grains", "snow"),
        80 => ("Light showers", "rain"),
        81 => ("Showers", "rain"),
        82 => ("Violent showers", "rain"),
        85 | 86 => ("Snow showers", "snow"),
        95 => ("Thunderstorm", "thunderstorm"),
        96 | 99 => ("Thunderstorm with hail", "thunderstorm"),
        _ => ("Unknown", "cloudy"),
    }
}

// ------------------------------------------------------------------ open-meteo: forecast

/// The fields of `api.open-meteo.com/v1/forecast` we read.
#[derive(Debug, Deserialize)]
pub struct RawForecast {
    current: RawCurrent,
    daily: RawDaily,
    hourly: RawHourly,
}

#[derive(Debug, Deserialize)]
struct RawCurrent {
    time: String,
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    is_day: u8,
    precipitation: f64,
    weather_code: u8,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
}

#[derive(Debug, Deserialize)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Vec<u8>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

#[derive(Debug, Deserialize)]
struct RawHourly {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<u8>,
    is_day: Vec<u8>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
}

/// The [`HOURS`] entries starting at the hour of `now` ("2026-11-03T21:00" matches the 21:00 slot;
/// ISO local times compare as strings). Hourly arrays of different lengths are an error.
fn next_hours(h: RawHourly, now: &str) -> Result<Vec<Hour>, RpcError> {
    let n = h.time.len();
    if [h.temperature_2m.len(), h.weather_code.len(), h.is_day.len()]
        .iter()
        .any(|&len| len != n)
    {
        return Err(RpcError::internal(
            "open-meteo returned misaligned hourly data",
        ));
    }
    let this_hour = now.get(..13).unwrap_or(now);
    let first = h
        .time
        .iter()
        .position(|t| t.as_str() >= this_hour)
        .unwrap_or(n);
    Ok((first..n.min(first + HOURS))
        .map(|i| {
            let is_day = h.is_day[i] != 0;
            let (condition, icon) = describe(h.weather_code[i], is_day);
            Hour {
                time: h.time[i].clone(),
                temperature: h.temperature_2m[i],
                code: h.weather_code[i],
                condition,
                icon,
                precipitation_probability: h.precipitation_probability.get(i).copied().flatten(),
                is_day,
            }
        })
        .collect())
}

/// Maps a raw forecast response to a [`Bundle`]. Daily or hourly arrays of different lengths are
/// an error (open-meteo always returns them aligned, so that means the format changed).
pub fn map_forecast(
    place: &Place,
    units: UnitSystem,
    raw: RawForecast,
) -> Result<Bundle, RpcError> {
    let RawForecast {
        current: c,
        daily: d,
        hourly: h,
    } = raw;
    let n = d.time.len();
    if [
        d.weather_code.len(),
        d.temperature_2m_max.len(),
        d.temperature_2m_min.len(),
    ]
    .iter()
    .any(|&len| len != n)
    {
        return Err(RpcError::internal(
            "open-meteo returned misaligned daily data",
        ));
    }
    let hourly = next_hours(h, &c.time)?;

    let is_day = c.is_day != 0;
    let (condition, icon) = describe(c.weather_code, is_day);
    let labels = units.labels();

    let days = (0..n)
        .map(|i| {
            // Forecast icons are always the daytime ones.
            let (condition, icon) = describe(d.weather_code[i], true);
            Day {
                date: d.time[i].clone(),
                code: d.weather_code[i],
                condition,
                icon,
                min: d.temperature_2m_min[i],
                max: d.temperature_2m_max[i],
                precipitation_probability: d
                    .precipitation_probability_max
                    .get(i)
                    .copied()
                    .flatten(),
            }
        })
        .collect();

    Ok(Bundle {
        current: Current {
            location: place.clone(),
            units: labels.clone(),
            time: c.time,
            temperature: c.temperature_2m,
            apparent: c.apparent_temperature,
            humidity: c.relative_humidity_2m,
            wind: c.wind_speed_10m,
            wind_direction: c.wind_direction_10m,
            precipitation: c.precipitation,
            code: c.weather_code,
            condition,
            icon,
            is_day,
        },
        forecast: Forecast {
            location: place.clone(),
            units: labels,
            days,
            hourly,
        },
    })
}

// ------------------------------------------------------------------ open-meteo: geocoding

/// `geocoding-api.open-meteo.com/v1/search`. `results` is absent when nothing matches.
#[derive(Debug, Deserialize)]
pub struct RawGeocoding {
    #[serde(default)]
    pub results: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    #[serde(rename(deserialize = "latitude"))]
    pub lat: f64,
    #[serde(rename(deserialize = "longitude"))]
    pub lon: f64,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub admin1: Option<String>,
    pub admin2: Option<String>,
    pub population: Option<u64>,
    #[serde(skip_serializing)]
    pub feature_code: Option<String>,
}

impl Candidate {
    pub fn place(&self) -> Place {
        Place {
            name: self.name.clone(),
            country: self.country.clone(),
            region: self.admin1.clone(),
            lat: self.lat,
            lon: self.lon,
            ..Place::default()
        }
    }
}

/// Splits "Paris, Texas" into the name to search and an optional hint to choose between matches.
pub fn split_city(city: &str) -> (&str, Option<&str>) {
    match city.split_once(',') {
        Some((name, hint)) if !hint.trim().is_empty() => (name.trim(), Some(hint.trim())),
        Some((name, _)) => (name.trim(), None),
        None => (city.trim(), None),
    }
}

/// Apply every comma-separated region/country hint; never silently choose a different country.
pub fn rank_places(raw: RawGeocoding, query: &str) -> Vec<Candidate> {
    let (name, hint) = split_city(query);
    let mut candidates: Vec<_> = raw
        .results
        .into_iter()
        .filter(|p| {
            crate::location::valid_coordinates(p.lat, p.lon)
                && !p.name.trim().is_empty()
                && p.feature_code
                    .as_deref()
                    .is_none_or(|code| code.starts_with("PPL") || code == "LCTY")
                && hint.is_none_or(|hint| {
                    hint.split(',')
                        .map(str::trim)
                        .filter(|h| !h.is_empty())
                        .all(|hint| {
                            let country = [&p.country, &p.country_code]
                                .into_iter()
                                .flatten()
                                .any(|field| field.eq_ignore_ascii_case(hint));
                            country
                                || [&p.admin1, &p.admin2]
                                    .into_iter()
                                    .flatten()
                                    .any(|field| admin_name(field) == admin_name(hint))
                        })
                })
        })
        .collect();
    candidates.sort_by_key(|p| {
        std::cmp::Reverse((
            p.name.eq_ignore_ascii_case(name),
            p.population.unwrap_or_default(),
        ))
    });
    candidates
}

/// Accept "Cairo" for "Cairo Governorate" without accepting partial names like "Calif".
fn admin_name(name: &str) -> String {
    let name = name.trim().to_lowercase();
    for suffix in [
        " governorate",
        " province",
        " region",
        " state",
        " county",
        " municipality",
        " district",
    ] {
        if let Some(short) = name.strip_suffix(suffix) {
            return short.to_owned();
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_takes_coordinates_or_a_city_and_validates() {
        let r = parse_request(&json!({ "lat": 59.9, "lon": 10.7, "units": "imperial", "days": 3 }))
            .unwrap();
        assert_eq!(
            r.place,
            Where::Coords(Place {
                name: "59.90, 10.70".into(),
                lat: 59.9,
                lon: 10.7,
                ..Place::default()
            })
        );
        assert_eq!((r.units, r.days), (UnitSystem::Imperial, 3));

        let r = parse_request(&json!({ "city": "  Oslo " })).unwrap();
        assert_eq!(r.place, Where::City("Oslo".into()));
        assert_eq!((r.units, r.days), (UnitSystem::Metric, 5));

        // Coordinates win over the city.
        let r = parse_request(&json!({ "city": "Oslo", "lat": 1.0, "lon": 2.0 })).unwrap();
        assert!(matches!(r.place, Where::Coords(_)));

        for auto in [
            Value::Null,
            json!({}),
            json!({ "auto": true }),
            json!({ "units": "imperial" }),
        ] {
            assert_eq!(parse_request(&auto).unwrap().place, Where::Auto);
        }

        for bad in [
            json!({ "auto": false }),
            json!({ "auto": "yes" }),
            json!([]),
            json!({ "city": 10 }),
            json!({ "city": "   " }),
            json!({ "lat": 1.0 }),
            json!({ "lat": 91.0, "lon": 0.0 }),
            json!({ "lat": "x", "lon": 0.0 }),
            json!({ "city": "Oslo", "units": "kelvin" }),
            json!({ "city": "Oslo", "days": 0 }),
            json!({ "city": "Oslo", "days": 8 }),
        ] {
            let err = parse_request(&bad).unwrap_err();
            assert_eq!(err.code, -32602, "{bad}");
        }
    }

    #[test]
    fn query_validation_and_weather_rounding() {
        assert_eq!(
            parse_query(&json!({ "query": " Maadi, Cairo " })).unwrap(),
            "Maadi, Cairo"
        );
        for bad in [
            Value::Null,
            json!({ "query": " " }),
            json!({ "query": 5 }),
            json!({ "query": "a".repeat(201) }),
        ] {
            assert_eq!(parse_query(&bad).unwrap_err().code, -32602);
        }
        assert_eq!(weather_coordinate(59.913947), "59.91");
        assert_eq!(weather_coordinate(-80.123456), "-80.12");
    }

    #[test]
    fn describes_weather_codes_by_day_and_night() {
        assert_eq!(describe(0, true), ("Clear sky", "clear-day"));
        assert_eq!(describe(0, false), ("Clear sky", "clear-night"));
        assert_eq!(describe(2, false), ("Partly cloudy", "partly-cloudy-night"));
        assert_eq!(describe(63, true), ("Rain", "rain"));
        assert_eq!(describe(95, false).1, "thunderstorm");
        assert_eq!(describe(200, true), ("Unknown", "cloudy"));
    }

    // Trimmed real response from api.open-meteo.com/v1/forecast?...&forecast_days=3
    const FORECAST: &str = r#"{
        "latitude": 51.5, "longitude": -0.12, "timezone": "Europe/London",
        "current": {
            "time": "2026-11-03T21:00", "interval": 900,
            "temperature_2m": 11.2, "relative_humidity_2m": 82, "apparent_temperature": 9.8,
            "is_day": 0, "precipitation": 0.0, "weather_code": 3,
            "wind_speed_10m": 14.4, "wind_direction_10m": 230
        },
        "daily": {
            "time": ["2026-11-03", "2026-11-04", "2026-11-05"],
            "weather_code": [3, 61, 0],
            "temperature_2m_max": [13.1, 12.0, 9.5],
            "temperature_2m_min": [7.0, 8.2, 3.1],
            "precipitation_probability_max": [10, null, 5]
        },
        "hourly": {
            "time": ["2026-11-03T20:00", "2026-11-03T21:00", "2026-11-03T22:00", "2026-11-03T23:00", "2026-11-04T00:00"],
            "temperature_2m": [11.6, 11.2, 10.9, 10.5, 10.1],
            "weather_code": [3, 3, 0, 61, 61],
            "is_day": [0, 0, 0, 0, 0],
            "precipitation_probability": [5, 10, null, 60, 80]
        }
    }"#;

    fn london() -> Place {
        Place {
            name: "London".into(),
            country: Some("United Kingdom".into()),
            lat: 51.5,
            lon: -0.12,
            ..Place::default()
        }
    }

    #[test]
    fn maps_a_forecast_response() {
        let raw: RawForecast = serde_json::from_str(FORECAST).unwrap();
        let bundle = map_forecast(&london(), UnitSystem::Metric, raw).unwrap();

        let c = &bundle.current;
        assert_eq!((c.temperature, c.apparent, c.humidity), (11.2, 9.8, 82.0));
        assert_eq!(
            (c.condition, c.icon, c.is_day),
            ("Overcast", "cloudy", false)
        );
        assert_eq!(c.units.temperature, "\u{b0}C");
        assert_eq!(c.time, "2026-11-03T21:00");

        let days = &bundle.forecast.days;
        assert_eq!(days.len(), 3);
        assert_eq!(
            (
                days[1].date.as_str(),
                days[1].icon,
                days[1].min,
                days[1].max
            ),
            ("2026-11-04", "rain", 8.2, 12.0)
        );
        assert_eq!(days[0].precipitation_probability, Some(10.0));
        assert_eq!(days[1].precipitation_probability, None); // null in the response

        // The wire shape widgets rely on.
        let json = serde_json::to_value(&bundle.current).unwrap();
        assert_eq!(json["location"]["name"], "London");
        assert_eq!(json["icon"], "cloudy");
        assert_eq!(json["wind_direction"], 230.0);
        let json = serde_json::to_value(&bundle.forecast).unwrap();
        assert!(json["days"][1].get("precipitation_probability").is_none());
    }

    #[test]
    fn hourly_starts_at_the_current_hour_and_is_capped() {
        let raw: RawForecast = serde_json::from_str(FORECAST).unwrap();
        let hourly = map_forecast(&london(), UnitSystem::Metric, raw)
            .unwrap()
            .forecast
            .hourly;
        // The 20:00 slot is in the past; the current 21:00 one comes first.
        let times: Vec<_> = hourly.iter().map(|h| h.time.as_str()).collect();
        assert_eq!(
            times,
            [
                "2026-11-03T21:00",
                "2026-11-03T22:00",
                "2026-11-03T23:00",
                "2026-11-04T00:00"
            ]
        );
        assert_eq!((hourly[1].icon, hourly[1].is_day), ("clear-night", false));
        assert_eq!(hourly[2].precipitation_probability, Some(60.0));
        assert_eq!(hourly[1].precipitation_probability, None);
        let json = serde_json::to_value(&hourly[1]).unwrap();
        assert!(json.get("precipitation_probability").is_none());
        assert_eq!(json["temperature"], 10.9);

        // A long response is cut to HOURS entries.
        let mut long: Value = serde_json::from_str(FORECAST).unwrap();
        let n = 3 * HOURS;
        long["hourly"] = json!({
            "time": (0..n).map(|i| format!("2026-11-{:02}T{:02}:00", 3 + i / 24, i % 24)).collect::<Vec<_>>(),
            "temperature_2m": vec![1.0; n],
            "weather_code": vec![0; n],
            "is_day": vec![1; n],
        });
        let raw: RawForecast = serde_json::from_value(long).unwrap();
        let hourly = map_forecast(&london(), UnitSystem::Metric, raw)
            .unwrap()
            .forecast
            .hourly;
        assert_eq!(hourly.len(), HOURS);
        assert_eq!(hourly[0].time, "2026-11-03T21:00");
        assert!(hourly[0].precipitation_probability.is_none());
    }

    #[test]
    fn imperial_labels_and_misaligned_data() {
        let raw: RawForecast = serde_json::from_str(FORECAST).unwrap();
        let bundle = map_forecast(&london(), UnitSystem::Imperial, raw).unwrap();
        assert_eq!(bundle.current.units.speed, "mph");

        let mut broken: Value = serde_json::from_str(FORECAST).unwrap();
        broken["daily"]["temperature_2m_min"] = json!([1.0]);
        let raw: RawForecast = serde_json::from_value(broken).unwrap();
        assert_eq!(
            map_forecast(&london(), UnitSystem::Metric, raw)
                .unwrap_err()
                .code,
            -32603
        );

        let mut broken: Value = serde_json::from_str(FORECAST).unwrap();
        broken["hourly"]["is_day"] = json!([0]);
        let raw: RawForecast = serde_json::from_value(broken).unwrap();
        assert_eq!(
            map_forecast(&london(), UnitSystem::Metric, raw)
                .unwrap_err()
                .code,
            -32603
        );
    }

    #[test]
    fn geocoding_picks_by_hint_and_handles_no_results() {
        let raw = || -> RawGeocoding {
            serde_json::from_value(json!({ "results": [
                { "name": "Paris", "latitude": 48.85, "longitude": 2.35, "country": "France", "country_code": "FR", "admin1": "Ile-de-France" },
                { "name": "Paris", "latitude": 33.66, "longitude": -95.55, "country": "United States", "country_code": "US", "admin1": "Texas" }
            ]}))
            .unwrap()
        };
        assert_eq!(
            rank_places(raw(), "Paris")
                .first()
                .unwrap()
                .country
                .as_deref(),
            Some("France")
        );
        assert_eq!(rank_places(raw(), "Paris, texas")[0].lat, 33.66);
        assert_eq!(rank_places(raw(), "Paris, us")[0].lon, -95.55);
        assert_eq!(rank_places(raw(), "Paris, Texas, US")[0].lat, 33.66);
        assert!(rank_places(raw(), "Paris, Texas, France").is_empty());
        assert!(rank_places(raw(), "Paris, mars").is_empty());

        let none: RawGeocoding =
            serde_json::from_value(json!({ "generationtime_ms": 0.4 })).unwrap();
        assert!(rank_places(none, "Nothing").is_empty());
    }

    #[test]
    fn search_matches_admin_names_and_preserves_the_candidate_contract() {
        let raw = serde_json::from_value(json!({ "results": [
            { "name": "Maadi", "latitude": -17.83, "longitude": 145.99, "country": "Australia", "country_code": "AU", "admin1": "Queensland", "population": 45 },
            { "name": "Maadi", "latitude": 29.95958, "longitude": 31.25924, "country": "Egypt", "country_code": "EG", "admin1": "Cairo Governorate", "feature_code": "PPLX" },
            { "name": "Maadi", "latitude": 29.8, "longitude": 31.2, "country": "Egypt", "admin1": "Cairo Governorate", "feature_code": "MT" }
        ] })).unwrap();
        let hits = rank_places(raw, "Maadi, Cairo, Egypt");
        assert_eq!(hits.len(), 1);
        let result = serde_json::to_value(&hits[0]).unwrap();
        assert_eq!(result["lat"], 29.95958);
        assert!(result.get("latitude").is_none());
        assert!(result.get("feature_code").is_none());
        assert_eq!(result["admin1"], "Cairo Governorate");
        assert!(result["admin2"].is_null());
        assert!(result["population"].is_null());
        assert_ne!(admin_name("California"), admin_name("Calif"));
    }

    #[test]
    fn splits_a_city_and_its_hint() {
        assert_eq!(split_city("Paris"), ("Paris", None));
        assert_eq!(split_city("Paris, Texas"), ("Paris", Some("Texas")));
        assert_eq!(split_city("Paris, "), ("Paris", None));
    }
}
