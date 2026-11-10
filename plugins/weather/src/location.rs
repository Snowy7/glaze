//! On-demand location: Windows first, then coarse IP. Coordinates and names stay in dataDir.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::rpc::RpcError;
use crate::weather::Place;

pub const LOCATION_TTL: u64 = 30 * 60;
const NAME_TTL: u64 = 30 * 24 * 60 * 60;
const FAILURE_TTL: u64 = 60;
const CACHE_FILE: &str = "location-cache.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Windows,
    Ip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
    pub name: String,
    pub region: String,
    pub country: String,
    pub source: Origin,
}

impl Location {
    fn valid(&self) -> bool {
        valid_coordinates(self.lat, self.lon)
            && self.accuracy_m.is_finite()
            && self.accuracy_m > 0.0
    }

    pub fn place(&self) -> Place {
        Place {
            name: self.name.clone(),
            region: (!self.region.is_empty()).then(|| self.region.clone()),
            country: (!self.country.is_empty()).then(|| self.country.clone()),
            lat: self.lat,
            lon: self.lon,
            accuracy_m: Some(self.accuracy_m),
            source: Some(self.source),
        }
    }
}

pub fn valid_coordinates(lat: f64, lon: f64) -> bool {
    (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Names {
    pub name: String,
    pub region: String,
    pub country: String,
}

/// Platform and HTTP boundaries; fakes exercise ordering and cache expiry without network access.
pub trait LocationSource: Send + Sync {
    fn windows(&self) -> Result<Location, String>;
    fn ip(&self) -> Result<Location, String>;
    fn reverse(&self, location: &Location) -> Result<Names, String>;
    fn network(&self) -> String;
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedLocation {
    location: Location,
    fetched: u64,
    network: String,
}

#[derive(Serialize, Deserialize)]
struct CachedName {
    // About 100 m: repeated positions in the same small area reuse their locality name.
    lat: i32,
    lon: i32,
    fetched: u64,
    names: Names,
}

#[derive(Default, Serialize, Deserialize)]
struct DiskCache {
    location: Option<CachedLocation>,
    names: Vec<CachedName>,
}

#[derive(Default)]
struct State {
    data_dir: Option<PathBuf>,
    disk: DiskCache,
    failure: Option<(u64, String, RpcError)>,
}

pub struct Locator<S> {
    source: S,
    // Also coalesces concurrent locate/current/forecast calls, including reverse geocoding.
    state: Mutex<State>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn recent(fetched: u64, now: u64, ttl: u64) -> bool {
    now.checked_sub(fetched).is_some_and(|age| age < ttl)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl<S: LocationSource> Locator<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            state: Mutex::default(),
        }
    }

    pub fn set_data_dir(&self, data_dir: PathBuf) {
        let mut state = lock(&self.state);
        // A truncated/untrusted cache is a miss, never a plugin startup failure.
        let path = data_dir.join(CACHE_FILE);
        let disk = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= 128 * 1024)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<DiskCache>(&bytes).ok())
            .unwrap_or_default();
        *state = State {
            data_dir: Some(data_dir),
            disk,
            failure: None,
        };
    }

    pub fn locate(&self) -> Result<Location, RpcError> {
        self.locate_at(now())
    }

    fn locate_at(&self, now: u64) -> Result<Location, RpcError> {
        let mut network = self.source.network();
        let mut state = lock(&self.state);
        // A transient profile-query failure is not evidence that the user changed networks.
        if network == "unavailable"
            && let Some(hit) = &state.disk.location
        {
            network.clone_from(&hit.network);
        }
        if let Some(hit) = &state.disk.location
            && hit.network == network
            && recent(hit.fetched, now, LOCATION_TTL)
            && hit.location.valid()
        {
            return Ok(hit.location.clone());
        }
        if let Some((fetched, previous_network, error)) = &state.failure
            && *previous_network == network
            && recent(*fetched, now, FAILURE_TTL)
        {
            return Err(error.clone());
        }

        let windows = self.source.windows().and_then(|location| {
            if location.valid() {
                Ok(location)
            } else {
                Err("Windows returned an invalid position".into())
            }
        });
        let mut location = match windows {
            Ok(location) => location,
            Err(windows_error) => {
                eprintln!(
                    "weather: Windows location unavailable: {windows_error}; trying IP (city level)"
                );
                match self.source.ip().and_then(|location| {
                    if location.valid() {
                        Ok(location)
                    } else {
                        Err("IP service returned an invalid position".into())
                    }
                }) {
                    Ok(location) => location,
                    Err(ip_error) => {
                        let error = RpcError::unavailable(format!(
                            "Cannot locate this device. Windows: {windows_error}. IP: {ip_error}. Enable Settings > Privacy & security > Location > Location services and Let desktop apps access your location, or choose a town."
                        ));
                        state.failure = Some((now, network, error.clone()));
                        return Err(error);
                    }
                }
            }
        };

        // IP coordinates describe a city centre, not the user's suburb. Keep its coarse name.
        if location.source == Origin::Windows {
            let lat = (location.lat * 1000.0).round() as i32;
            let lon = (location.lon * 1000.0).round() as i32;
            state
                .disk
                .names
                .retain(|n| recent(n.fetched, now, NAME_TTL));
            let names = state
                .disk
                .names
                .iter()
                .find(|n| n.lat == lat && n.lon == lon)
                .map(|n| n.names.clone());
            let names = names.or_else(|| match self.source.reverse(&location) {
                Ok(names) => {
                    if state.disk.names.len() >= 32 {
                        state.disk.names.remove(0);
                    }
                    state.disk.names.push(CachedName {
                        lat,
                        lon,
                        fetched: now,
                        names: names.clone(),
                    });
                    Some(names)
                }
                Err(error) => {
                    eprintln!("weather: locality name unavailable: {error}");
                    None
                }
            });
            if let Some(names) = names {
                location.name = names.name;
                location.region = names.region;
                location.country = names.country;
            }
        }
        state.disk.location = Some(CachedLocation {
            location: location.clone(),
            fetched: now,
            network,
        });
        state.failure = None;
        if let Some(data_dir) = &state.data_dir {
            let save = || -> Result<(), Box<dyn std::error::Error>> {
                std::fs::create_dir_all(data_dir)?;
                let bytes = serde_json::to_vec(&state.disk)?;
                let temporary = data_dir.join("location-cache.tmp");
                std::fs::write(&temporary, bytes)?;
                std::fs::rename(temporary, data_dir.join(CACHE_FILE))?;
                Ok(())
            };
            if let Err(error) = save() {
                eprintln!("weather: cannot save location cache: {error}");
            }
        }
        Ok(location)
    }
}

pub struct NativeLocation {
    agent: crate::http::Http,
}

impl Default for NativeLocation {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeLocation {
    pub fn new() -> Self {
        Self {
            agent: crate::http::Http::new(Duration::from_secs(5)),
        }
    }
}

#[derive(Deserialize)]
struct IpResponse {
    success: bool,
    message: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReverseResponse {
    locality: String,
    city: String,
    principal_subdivision: String,
    country_name: String,
}

impl ReverseResponse {
    fn names(self) -> Option<Names> {
        let name = [
            &self.locality,
            &self.city,
            &self.principal_subdivision,
            &self.country_name,
        ]
        .into_iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())?
        .to_owned();
        Some(Names {
            name,
            region: self.principal_subdivision,
            country: self.country_name,
        })
    }
}

impl LocationSource for NativeLocation {
    fn windows(&self) -> Result<Location, String> {
        native::position()
    }

    fn ip(&self) -> Result<Location, String> {
        let raw: IpResponse = self
            .agent
            .get("https://ipwho.is/")
            .query(
                "fields",
                "success,message,latitude,longitude,city,region,country",
            )
            .json()
            .map_err(|e| e.to_string())?;
        if !raw.success {
            return Err(raw.message.unwrap_or_else(|| "IP lookup failed".into()));
        }
        let lat = raw.latitude.ok_or("IP lookup has no latitude")?;
        let lon = raw.longitude.ok_or("IP lookup has no longitude")?;
        let region = raw.region.unwrap_or_default();
        let country = raw.country.unwrap_or_default();
        let name = raw
            .city
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                if region.is_empty() {
                    country.clone()
                } else {
                    region.clone()
                }
            });
        Ok(Location {
            lat,
            lon,
            name,
            region,
            country,
            // A conservative indicator, not a confidence radius supplied by the provider.
            accuracy_m: 25_000.0,
            source: Origin::Ip,
        })
    }

    fn reverse(&self, location: &Location) -> Result<Names, String> {
        // Only current OS positions reach this client endpoint; never search picks or IP centres.
        let raw: ReverseResponse = self
            .agent
            .get("https://api.bigdatacloud.net/data/reverse-geocode-client")
            .query("latitude", location.lat.to_string())
            .query("longitude", location.lon.to_string())
            .query("localityLanguage", "en")
            .json()
            .map_err(|e| e.to_string())?;
        raw.names()
            .ok_or_else(|| "reverse geocoder returned no locality".into())
    }

    fn network(&self) -> String {
        native::network()
    }
}

#[cfg(windows)]
pub mod native {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::mpsc;
    use std::thread;

    use windows::Devices::Geolocation::{Geolocator, PositionAccuracy, PositionStatus};
    use windows::Foundation::TimeSpan;
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

    use super::*;

    pub struct Apartment;

    impl Apartment {
        pub fn new() -> windows::core::Result<Self> {
            // SAFETY: Each plugin worker is a fresh thread; every successful init is balanced.
            unsafe {
                RoInitialize(RO_INIT_MULTITHREADED)?;
            }
            Ok(Self)
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: Runs on the thread where this guard successfully initialized WinRT.
            unsafe {
                RoUninitialize();
            }
        }
    }

    pub fn position() -> Result<Location, String> {
        // Bound activation as well as the async operation; a broken location service cannot hang RPC.
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = sender.send(position_inner().map_err(|e| e.to_string()));
        });
        receiver
            .recv_timeout(Duration::from_secs(4))
            .map_err(|_| "Windows location timed out after 4 seconds".to_owned())?
    }

    fn position_inner() -> windows::core::Result<Location> {
        let _apartment = Apartment::new()?;
        let locator = Geolocator::new()?;
        if matches!(
            locator.LocationStatus()?,
            PositionStatus::Disabled | PositionStatus::NotAvailable
        ) {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80070005u32 as i32),
                "Windows location is disabled or unavailable",
            ));
        }
        locator.SetDesiredAccuracy(PositionAccuracy::High)?;
        let operation = locator.GetGeopositionAsyncWithAgeAndTimeout(
            TimeSpan {
                Duration: 60 * 10_000_000,
            },
            TimeSpan {
                Duration: 3 * 10_000_000,
            },
        )?;
        let (sender, receiver) = mpsc::sync_channel(1);
        operation.when(move |result| {
            let _ = sender.send(result);
        })?;
        let position = match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(result) => result?,
            Err(_) => {
                let _ = operation.Cancel();
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x800705B4u32 as i32),
                    "Windows location timed out",
                ));
            }
        };
        let coordinate = position.Coordinate()?;
        let point = coordinate.Point()?.Position()?;
        Ok(Location {
            lat: point.Latitude,
            lon: point.Longitude,
            accuracy_m: coordinate.Accuracy()?,
            name: format!("{:.2}, {:.2}", point.Latitude, point.Longitude),
            region: String::new(),
            country: String::new(),
            source: Origin::Windows,
        })
    }

    /// Check the current profile on demand. Only its hash is persisted, never SSIDs or adapter IDs.
    pub fn network() -> String {
        // This WinRT API is synchronous. Bound it too, including apartment activation/teardown.
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let read = || -> windows::core::Result<String> {
                let _apartment = Apartment::new()?;
                let profile = NetworkInformation::GetInternetConnectionProfile()?;
                let mut hash = DefaultHasher::new();
                profile.ProfileName()?.to_string().hash(&mut hash);
                profile
                    .NetworkAdapter()?
                    .NetworkAdapterId()?
                    .hash(&mut hash);
                Ok(format!("{:016x}", hash.finish()))
            };
            let _ = sender.send(read().ok());
        });
        receiver
            .recv_timeout(Duration::from_secs(1))
            .ok()
            .flatten()
            .unwrap_or_else(|| "unavailable".into())
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn position() -> Result<Location, String> {
        Err("Windows location requires Windows".into())
    }
    pub fn network() -> String {
        "unsupported".into()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct Fake {
        denied: bool,
        offline: bool,
        network: Mutex<String>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                denied: false,
                offline: false,
                network: Mutex::new("home".into()),
                calls: Mutex::default(),
            }
        }
    }

    fn position(source: Origin) -> Location {
        Location {
            lat: 59.9139,
            lon: 10.7522,
            accuracy_m: if source == Origin::Windows {
                75.0
            } else {
                25_000.0
            },
            name: "Oslo".into(),
            region: "Oslo County".into(),
            country: "Norway".into(),
            source,
        }
    }

    impl LocationSource for Fake {
        fn windows(&self) -> Result<Location, String> {
            lock(&self.calls).push("windows");
            if self.denied {
                Err("access denied".into())
            } else {
                Ok(position(Origin::Windows))
            }
        }

        fn ip(&self) -> Result<Location, String> {
            lock(&self.calls).push("ip");
            if self.offline {
                Err("offline".into())
            } else {
                Ok(position(Origin::Ip))
            }
        }

        fn reverse(&self, _location: &Location) -> Result<Names, String> {
            lock(&self.calls).push("reverse");
            Ok(Names {
                name: "Sentrum".into(),
                region: "Oslo County".into(),
                country: "Norway".into(),
            })
        }

        fn network(&self) -> String {
            lock(&self.network).clone()
        }
    }

    #[test]
    fn windows_is_preferred_and_ip_never_invents_a_suburb() {
        let windows = Locator::new(Fake::default());
        let found = windows.locate_at(100).unwrap();
        assert_eq!(found.source, Origin::Windows);
        assert_eq!(found.name, "Sentrum");
        assert_eq!(*lock(&windows.source.calls), ["windows", "reverse"]);

        let ip = Locator::new(Fake {
            denied: true,
            ..Fake::default()
        });
        let found = ip.locate_at(100).unwrap();
        assert_eq!(found.source, Origin::Ip);
        assert_eq!(found.name, "Oslo");
        assert_eq!(found.accuracy_m, 25_000.0);
        assert_eq!(*lock(&ip.source.calls), ["windows", "ip"]);
    }

    #[test]
    fn cache_expires_or_follows_network_changes_and_reuses_names() {
        let locator = Locator::new(Fake::default());
        locator.locate_at(100).unwrap();
        locator.locate_at(100 + LOCATION_TTL - 1).unwrap();
        assert_eq!(*lock(&locator.source.calls), ["windows", "reverse"]);
        locator.locate_at(100 + LOCATION_TTL).unwrap();
        assert_eq!(
            *lock(&locator.source.calls),
            ["windows", "reverse", "windows"]
        );
        *lock(&locator.source.network) = "office".into();
        locator.locate_at(101 + LOCATION_TTL).unwrap();
        assert_eq!(lock(&locator.source.calls).len(), 4);
        // Future timestamps are not trusted after a clock correction.
        locator.locate_at(1).unwrap();
        assert_eq!(lock(&locator.source.calls).len(), 6);
    }

    #[test]
    fn failures_are_bounded_and_coalesced_but_network_changes_retry() {
        let locator = Locator::new(Fake {
            denied: true,
            offline: true,
            ..Fake::default()
        });
        assert_eq!(
            locator.locate_at(100).unwrap_err().data_code,
            Some("PluginUnavailable")
        );
        locator.locate_at(101).unwrap_err();
        assert_eq!(*lock(&locator.source.calls), ["windows", "ip"]);
        *lock(&locator.source.network) = "new".into();
        locator.locate_at(102).unwrap_err();
        locator.locate_at(102 + FAILURE_TTL).unwrap_err();
        assert_eq!(lock(&locator.source.calls).len(), 6);
    }

    #[test]
    fn disk_cache_survives_a_restart_and_bad_files_are_ignored() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "glaze-weather-test-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let first = Locator::new(Fake::default());
        first.set_data_dir(directory.clone());
        let expected = first.locate_at(100).unwrap();
        let restarted = Locator::new(Fake::default());
        restarted.set_data_dir(directory.clone());
        assert_eq!(restarted.locate_at(101).unwrap(), expected);
        assert!(lock(&restarted.source.calls).is_empty());
        restarted.locate_at(100 + LOCATION_TTL).unwrap();
        assert_eq!(*lock(&restarted.source.calls), ["windows"]);
        std::fs::write(directory.join(CACHE_FILE), "{truncated").unwrap();
        let corrupt = Locator::new(Fake::default());
        corrupt.set_data_dir(directory.clone());
        corrupt.locate_at(102).unwrap();
        assert_eq!(*lock(&corrupt.source.calls), ["windows", "reverse"]);
        // Delete only the files this test created, never a recursive/computed directory tree.
        std::fs::remove_file(directory.join(CACHE_FILE)).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn names_prefer_suburb_then_town_city_region_and_country() {
        let raw = r#"{"locality":"Maadi","city":"Cairo","principalSubdivision":"Cairo","countryName":"Egypt"}"#;
        let mut response: ReverseResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(response.names().unwrap().name, "Maadi");
        response = serde_json::from_str(raw).unwrap();
        response.locality.clear();
        assert_eq!(response.names().unwrap().name, "Cairo");
        assert_eq!(
            ReverseResponse {
                principal_subdivision: "Region".into(),
                country_name: "Country".into(),
                ..Default::default()
            }
            .names()
            .unwrap()
            .name,
            "Region"
        );
        assert_eq!(
            ReverseResponse {
                country_name: "Country".into(),
                ..Default::default()
            }
            .names()
            .unwrap()
            .name,
            "Country"
        );
        assert!(ReverseResponse::default().names().is_none());
    }
}
