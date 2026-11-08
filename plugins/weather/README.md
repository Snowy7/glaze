# Weather plugin RPC

JSON-RPC 2.0, one compact object per line over stdio. The host must initialize the plugin with
`{ "protocol": 1, "plugin": "weather", "dataDir": "..." }`; it replies `{ "protocol": 1 }`.
`dataDir` is normally `%APPDATA%\glaze\plugin-data\weather`. Standalone runs without a
handshake use that same default directory. stdout contains protocol messages only.

## Location selection

`current` and `forecast` accept any of these location parameters:

| Params | Meaning |
| --- | --- |
| omitted, `null`, `{}`, or `{ "auto": true }` | Locate this device automatically. |
| `{ "lat": 29.96, "lon": 31.26 }` | A selected place. Both coordinates are required. |
| `{ "city": "Springfield, Illinois" }` | Backward-compatible town lookup with optional region/country hints. |

Explicit coordinates win over `city`, which wins over `auto`. `auto` must be boolean when
present; `auto: false` without an explicit location is invalid. Latitude is -90..90 and
longitude -180..180. Coordinate selections may include optional `name`, `region`, and
`country` strings for display. For example:

```json
{ "lat": 29.96, "lon": 31.26, "name": "Maadi", "region": "Cairo", "country": "Egypt" }
```

Strings are trimmed and limited to 200 UTF-8 bytes. Empty `city` is invalid. A city may have
multiple comma-separated hints (`"Paris, Texas, US"`); every hint must match a country,
country code, admin1, or admin2. Common administrative suffixes are optional (for example,
`Cairo` matches `Cairo Governorate`). Exact town names rank before prefix matches, then larger
populations rank first. An unmatched hint returns `NotFound`, never an unrelated homonym.

### `locate`

Params: omitted, `null`, or `{}`. Result (all fields always present):

```json
{
  "lat": 59.9139,
  "lon": 10.7522,
  "accuracy_m": 75,
  "name": "Al Mirqab",
  "region": "Oslo",
  "country": "Norway",
  "source": "windows"
}
```

This is an illustrative shape, not a measurement. `source` is `"windows"` or `"ip"`.
Windows supplies its accuracy in metres. IP accuracy is a conservative **25,000 m indicator**,
not a measured confidence radius; VPNs and mobile networks can put it much farther away.
Do not present IP results as a detected suburb. Unknown region/country are empty strings.
If reverse geocoding fails, a Windows position still works; its name is the rounded coordinate
pair until a later resolution succeeds. No address, postcode, or street is retained.

Resolution order:

1. Windows `Windows.Devices.Geolocation.Geolocator`, high accuracy, using an OS position up to
   one minute old (`MaximumAge`). Acquisition has a three-second timeout and cancellation;
   activation and acquisition together have a four-second caller deadline. Disabled/denied
   location falls back immediately. No permission prompt is launched by the plugin.
2. HTTPS `ipwho.is` when Windows is unavailable, denied, or times out. Only its city, region,
   country and coordinates are used. It is not reverse geocoded into a misleading suburb.
3. For a Windows position, BigDataCloud's client reverse geocoder supplies its finest named
   locality (suburb/village/town), then city, region, or country.

There is no location polling loop. Concurrent calls share one resolution. A successful
location is cached for 30 minutes in memory and in `dataDir/location-cache.json`. On each
automatic call, a hash of the current Windows network profile and adapter is compared with
the cache; a different network invalidates it immediately. Network names and adapter IDs
are never saved or sent. The synchronous profile check has a one-second deadline; a failed
check retains the last known network. Movement on the same network is picked up at the next expiry.
Reverse names are cached for 30 days in approximately 100 m cells, capped at 32 entries,
and survive restarts. Both sources failing returns `PluginUnavailable`; repeated failures
are cached for one minute in memory, with an immediate retry after a network change.
Unreadable, corrupt, expired, or future-dated caches are treated as misses. Disk-write errors
do not prevent weather from working.

### `search`

Params: `{ "query": "Maadi, Cairo, Egypt" }`. Result: an array of zero to eight candidates:

```json
[
  {
    "name": "Maadi",
    "admin1": "Cairo",
    "admin2": null,
    "country": "Egypt",
    "country_code": "EG",
    "lat": 29.96,
    "lon": 31.26,
    "population": null
  }
]
```

This is an illustrative shape. Missing upstream administrative fields/population are `null`.
Queries shorter than two characters return `[]`; empty/missing/non-string queries are invalid.
No matches return `[]`, not `NotFound`. Searches use Open-Meteo geocoding, with up to 100
upstream homonyms considered before applying all hints and returning eight.
Known non-settlement features (mountains, airports, etc.) are excluded. Identical queries
are cached for one hour in memory (at most 128 queries). Search does not change auto location.

## Weather results

`current` and `forecast` additionally accept `units: "metric" | "imperial"` (default metric).
`forecast` accepts `days: 1..7` (default 5). Existing weather fields and icons are unchanged:

- `current`: `{ location, units: { temperature, speed, precipitation }, time, temperature,
  apparent, humidity, wind, wind_direction, precipitation, code, condition, icon, is_day }`.
- `forecast`: `{ location, units, days: [{ date, code, condition, icon, min, max,
  precipitation_probability? }], hourly: [{ time, temperature, code, condition, icon,
  precipitation_probability?, is_day }] }`. Hourly data covers the next 24 hours starting
  with the current hour; daily data starts today. Times are local to the chosen place.
- `location`: `{ name, lat, lon, country?, region?, accuracy_m?, source? }`.
  Automatic results include `accuracy_m` and `source`; manual selections omit them.

Weather is cached for 10 minutes per coordinates/unit system. `current` and `forecast` share
the fetch; an individual caller's chosen display name is preserved even when sharing weather.
`updated` events retain `{ location, units: "metric" | "imperial" }`; refetch with the widget's
current settings. `suspend` stops weather background refresh, `resume` catches up, and
`shutdown` replies `null` and exits. Location lookup itself never starts background refresh.

## Widget integration handoff

Default to automatic location: call `current` / `forecast` with `{ "auto": true, "units": ... }`.
Remove the London default and migrate the old default to auto. Keep deliberately chosen cities.
Use the returned `location.name`, with region/country where useful; indicate approximate
location for `source: "ip"` or a coarse Windows `accuracy_m`. An explicit **Location** setting should offer **Automatic** and a
town search. Debounce `search` calls (about 300 ms), wait for two characters, and ignore responses
for superseded queries. Format candidates from distinct nonempty `name`, `admin2`, `admin1`,
and `country` values so homonyms are distinguishable. Persist the selected candidate locally;
send its `lat`, `lon`, `name`, `region: admin1`, and `country` on subsequent weather calls.
Choosing Automatic removes that selection. Search text alone should never set a location.

## Errors

Every error has `{ code, message, data: { code: ... } }`:

| Numeric code | `data.code` | Trigger |
| --- | --- | --- |
| -32602 | `InvalidParams` | Invalid location, query, units, days, or params shape. |
| -32000 | `NotFound` | `city` and its hints match no town. |
| -32001 | `PluginUnavailable` | Both automatic sources failed, or upstream weather/search is unreachable. |
| -32603 | `Internal` | Malformed upstream weather/search payload or serialization failure. |
| -32601 | `NotFound` | Unknown method. |

Framing errors retain JSON-RPC -32700 (parse error) / -32600 (invalid request), without a
`data.code`. Reverse-geocoder failure is nonfatal. Windows permission denial alone is nonfatal
because IP is tried next. When both sources fail, offer town search instead of showing London.

On Windows 11, enable **Settings > Privacy & security > Location > Location services**, then
**Let desktop apps access your location**. The URI is `ms-settings:privacy-location`. On
Windows 10 the page is **Settings > Privacy > Location**. The plugin does not alter these settings.
An existing successful cache may take up to 30 minutes to expire after permission changes.

## Services and privacy

- [ipwho.is documentation](https://ipwhois.io/documentation): no API key, HTTPS, commercial
  use allowed on the free endpoint; 1,000 requests/day per client IP, with no SLA. Normal
  30-minute caching uses at most 48/day per device, plus network-change resolutions.
- [BigDataCloud client reverse geocoding](https://www.bigdatacloud.com/docs/free-api/free-reverse-geocode-to-city-api)
  and [fair use policy](https://www.bigdatacloud.com/docs/article/fair-use-policy-for-free-client-side-reverse-geocoding-api):
  no key; calls must originate on the device using its current, permission-granted native
  location. Only newly obtained Windows positions are sent, never manual picks, IP city centres,
  or stored coordinates. Cached names are reused without sending cached positions back.
  BigDataCloud explains that coordinate/IP pairings help improve its IP geolocation database.
- [Open-Meteo geocoding](https://open-meteo.com/en/docs/geocoding-api) and
  [terms](https://open-meteo.com/en/terms): the existing no-key free service is for
  noncommercial use, limited to 10,000 calls/day, 5,000/hour and 600/minute. Attribute weather
  to Open-Meteo and search data to GeoNames (CC BY 4.0). Commercial distribution needs an
  appropriate Open-Meteo plan or self-hosted endpoint.

No analytics or location upload to Glaze is implemented. Exact location and the cache stay
on the device except for the necessary direct reverse-geocoding request. That provider sees
current coordinates and the public IP; the IP provider sees the public IP; search sends typed
queries to Open-Meteo. The forecast endpoint receives **only two-decimal coordinates**
(approximately 1 km) as location input, plus weather options, never place names, network IDs,
or the full-precision position. HTTP requests use an identifying `glaze-weather/1.1` User-Agent.

## Validation and standalone use

```powershell
$env:CARGO_TARGET_DIR = 'C:\glaze-targets\weather'
cargo test -p glaze-plugin-weather
cargo clippy -p glaze-plugin-weather --all-targets -- -D warnings
cargo build -p glaze-plugin-weather
```

Run `C:\glaze-targets\weather\debug\weather.exe`, keep stdin open, write each request line,
and read its matching response before sending shutdown. Example requests:

```jsonl
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"plugin":"weather","dataDir":"C:\\glaze-targets\\weather\\rpc-data"}}
{"jsonrpc":"2.0","id":1,"method":"locate"}
{"jsonrpc":"2.0","id":2,"method":"current","params":{"auto":true}}
{"jsonrpc":"2.0","id":3,"method":"search","params":{"query":"Springfield, Illinois"}}
{"jsonrpc":"2.0","id":4,"method":"shutdown"}
```

Calls run concurrently, so piping shutdown immediately after slow requests will terminate them.
Never copy over `plugins/weather/weather.exe` while Glaze is using it; deployment is separate.
