// @ts-check
// Pure logic for the weather widget: no DOM, so it is unit-tested
// (packages/widget-sdk/test/weather-logic.test.ts).

/**
 * Response shapes of the weather plugin (docs/writing-plugins.md, "The bundled weather plugin").
 * @typedef {{ temperature: string, speed: string, precipitation: string }} Units
 * @typedef {{ name: string, country?: string, lat: number, lon: number }} Place
 * @typedef {{ location: Place, units: Units, time: string, temperature: number, apparent: number, humidity: number, wind: number, code: number, condition: string, icon: string, is_day: boolean }} Current
 * @typedef {{ date: string, code: number, condition: string, icon: string, min: number, max: number, precipitation_probability?: number }} Day
 * @typedef {{ time: string, temperature: number, code: number, condition: string, icon: string, precipitation_probability?: number, is_day: boolean }} Hour
 * @typedef {{ location: Place, units: Units, days: Day[], hourly?: Hour[] }} Forecast
 */

/** @typedef {"card" | "compact" | "forecast"} Variant */
/** @typedef {"mini" | "strip" | "stack" | "tall" | "side"} Layout */

/**
 * How the widget is arranged at a size (CSS px, as `trackSize` reports). The names are what
 * style.css lays out:
 *   mini    one short row, now only            strip   one wide row, now and the next hours
 *   stack   now, then the next hours           tall    now, hours and the days (forecast: head and rows)
 *   side    now and hours on the left, the days on the right (card only)
 * `forecast` keeps its days as columns when short (`mini`, `strip`) and as rows when tall.
 * @param {Variant} variant
 * @param {number} width
 * @param {number} height
 * @returns {Layout}
 */
export function pickLayout(variant, width, height) {
  if (variant === "forecast") {
    if (height < 125) return "mini";
    if (height < 200 && width >= 260) return "strip";
    return "tall";
  }
  if (height < 160) return width < 300 ? "mini" : "strip"; // two rows need about 160px
  if (variant === "card") {
    if (width >= 400) return "side";
    if (height >= 270) return "tall";
  }
  return "stack";
}

/**
 * How many slots of `min` px (with `gap` between) fit in `available` px.
 * @param {number} available
 * @param {number} min
 * @param {number} [gap]
 */
export function slotCount(available, min, gap = 0) {
  return Math.max(0, Math.floor((available + gap) / (min + gap)));
}

/**
 * The illustration's layers for a plugin icon. `tone` is the cloud color (a storm is darker than
 * a cloudy day) and `intensity` how much falls, from the WMO code (drizzle and light showers are
 * light, heavy rain and snow heavy).
 * @typedef {"sun" | "moon" | "stars" | "cloud-b" | "cloud-a" | "fog" | "rain" | "snow" | "bolt"} Layer
 * @typedef {"clear-day" | "clear-night" | "partly-day" | "partly-night" | "cloudy" | "fog" | "rain" | "snow" | "storm"} Scene
 * @param {string} icon
 * @param {number} [code]
 * @returns {{ scene: Scene, layers: Layer[], tone: "light" | "grey" | "dark", intensity: "light" | "normal" | "heavy" }}
 */
export function sceneFor(icon, code = -1) {
  /** @type {"light" | "normal" | "heavy"} */
  let intensity = "normal";
  if ([51, 53, 55, 56, 57, 61, 71, 77, 80].includes(code)) intensity = "light";
  else if ([65, 67, 75, 82, 86, 99].includes(code)) intensity = "heavy";
  switch (icon) {
    case "clear-day":
      return { scene: "clear-day", layers: ["sun"], tone: "light", intensity };
    case "clear-night":
      return { scene: "clear-night", layers: ["moon", "stars"], tone: "light", intensity };
    case "partly-cloudy-day":
      return { scene: "partly-day", layers: ["sun", "cloud-a"], tone: "light", intensity };
    case "partly-cloudy-night":
      return { scene: "partly-night", layers: ["moon", "stars", "cloud-a"], tone: "light", intensity };
    case "fog":
      return { scene: "fog", layers: ["cloud-a", "fog"], tone: "grey", intensity };
    case "drizzle":
      return { scene: "rain", layers: ["cloud-b", "cloud-a", "rain"], tone: "grey", intensity: "light" };
    case "rain":
      return { scene: "rain", layers: ["cloud-b", "cloud-a", "rain"], tone: "grey", intensity };
    case "snow":
      return { scene: "snow", layers: ["cloud-b", "cloud-a", "snow"], tone: "grey", intensity };
    case "thunderstorm":
      return { scene: "storm", layers: ["cloud-b", "cloud-a", "rain", "bolt"], tone: "dark", intensity };
    default:
      return { scene: "cloudy", layers: ["cloud-b", "cloud-a"], tone: "light", intensity };
  }
}

/**
 * A temperature as "11°"; never "-0°".
 * @param {number} t
 */
export function degrees(t) {
  const r = Math.round(t);
  return `${r === 0 ? 0 : r}°`;
}

/**
 * Where a day's range sits on the week's scale, as percentages for a range bar.
 * @param {number} min the day's low
 * @param {number} max the day's high
 * @param {number} low the lowest low of all shown days
 * @param {number} high the highest high of all shown days
 */
export function rangeBar(min, max, low, high) {
  const span = high - low;
  if (!(span > 0)) return { start: 0, size: 100 };
  const start = ((min - low) / span) * 100;
  const size = Math.max(((max - min) / span) * 100, 8); // a day with no spread is still a dot
  return { start: Math.min(start, 100 - size), size };
}

/**
 * The place's local time now ("2026-11-04T14:20"): the plugin's `current.time` moved forward by the
 * time since it was fetched, so another city's hours are right whatever the user's time zone.
 * @param {string} currentTime "2026-11-04T14:00" as reported by the plugin
 * @param {number} fetchedAt ms since epoch, when `currentTime` was read
 * @param {number} nowMs
 */
export function placeNow(currentTime, fetchedAt, nowMs) {
  // The place's local time, read as if it were UTC. (Date.parse alone is too forgiving of junk.)
  const base = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(currentTime) ? Date.parse(`${currentTime}:00Z`) : Number.NaN;
  if (Number.isNaN(base)) return currentTime;
  return new Date(base + Math.max(0, nowMs - fetchedAt)).toISOString().slice(0, 16);
}

/**
 * The hourly entries from the current hour on (the plugin's list can be a little old), at most `count`.
 * @param {Hour[]} hourly
 * @param {string} nowIso the place's local time, see {@link placeNow}
 * @param {number} count
 */
export function upcomingHours(hourly, nowIso, count) {
  const thisHour = nowIso.slice(0, 13);
  return hourly.filter((h) => h.time.slice(0, 13) >= thisHour).slice(0, Math.max(0, count));
}

/**
 * The hour as the locale writes it ("14", "2 PM"). Hours are read from the ISO string, not parsed
 * as local time, so a daylight-saving gap can't shift them.
 * @param {string} iso "2026-11-04T14:00"
 * @param {string} [locale]
 */
export function hourLabel(iso, locale) {
  const match = /T(\d{2}):/.exec(iso);
  if (!match) return "";
  const hour = Number(match[1]);
  return new Intl.DateTimeFormat(locale, { hour: "numeric", timeZone: "UTC" }).format(Date.UTC(2000, 0, 1, hour));
}

/**
 * A forecast day's name: "Today", then the short weekday.
 * @param {string} date "2026-11-04"
 * @param {number} index position in the list
 * @param {string} [locale]
 */
export function dayLabel(date, index, locale) {
  if (index === 0) return "Today";
  const [y = 1970, m = 1, d = 1] = date.split("-").map(Number);
  return new Intl.DateTimeFormat(locale, { weekday: "short", timeZone: "UTC" }).format(Date.UTC(y, m - 1, d));
}

/**
 * A place to show the weather of: a town picked in the settings (with its coordinates and, for
 * showing, its region and country), one found by the plugin's `locate` (with how it was found),
 * or only a name (an older text setting, looked up by name).
 * @typedef {{ name: string, lat?: number, lon?: number, region?: string, country?: string,
 *   source?: string, accuracy?: number }} Where
 */
/** @typedef {{ name: string, admin1?: string | null, admin2?: string | null, country?: string | null, lat: number, lon: number }} Candidate */

/** The city every weather widget had before Automatic: an instance still on it moves to Automatic. */
export const OLD_DEFAULT_CITY = "London";

/** @param {unknown} v */
const text = (v) => (typeof v === "string" && v.trim() ? v.trim() : undefined);
/** @param {unknown} v */
const finite = (v) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);

/**
 * The `city` setting (a `place`): null is automatic, else a town. Untrusted JSON, so anything
 * malformed counts as automatic, and so does the old default city.
 * @param {unknown} value
 * @returns {Where | null}
 */
export function readPlace(value) {
  // A plain name, as an older text setting saved it (the host already turns it into { name }).
  const raw = typeof value === "string" ? { name: value } : value;
  if (typeof raw !== "object" || raw === null) return null;
  const record = /** @type {Record<string, unknown>} */ (raw);
  const name = text(record["name"]);
  if (!name) return null;
  const lat = finite(record["lat"]);
  const lon = finite(record["lon"]);
  const coords = lat !== undefined && lon !== undefined;
  if (!coords && name === OLD_DEFAULT_CITY) return null;
  const region = text(record["region"]);
  const country = text(record["country"]);
  return {
    name,
    ...(coords ? { lat, lon } : {}),
    ...(region ? { region } : {}),
    ...(country ? { country } : {}),
  };
}

/**
 * The plugin's `locate` answer as a place: null without a name and coordinates.
 * @param {unknown} reply
 * @returns {Where | null}
 */
export function readLocated(reply) {
  const place = readPlace(reply);
  if (!place || place.lat === undefined || typeof reply !== "object" || reply === null) return null;
  const record = /** @type {Record<string, unknown>} */ (reply);
  const source = text(record["source"]);
  const accuracy = finite(record["accuracy_m"]);
  return { ...place, ...(source ? { source } : {}), ...(accuracy !== undefined ? { accuracy } : {}) };
}

/**
 * How to ask the plugin for a place: by coordinates (with the names to show) when known, else by
 * name (an older city setting, which the plugin looks up).
 * @param {Where} place
 */
export function placeRequest(place) {
  if (place.lat === undefined || place.lon === undefined) return { city: place.name };
  return {
    lat: place.lat,
    lon: place.lon,
    name: place.name,
    ...(place.region ? { region: place.region } : {}),
    ...(place.country ? { country: place.country } : {}),
  };
}

/** Distinct, non-empty parts joined: "Oslo, Oslo County, Norway". @param {(string | null | undefined)[]} parts */
function joinParts(parts) {
  const kept = parts.filter((p) => typeof p === "string" && p.trim() !== "").map((p) => String(p).trim());
  return kept.filter((p, i) => kept.indexOf(p) === i).join(", ");
}

/** A fix is approximate when it came from the network (IP), or Windows says it is coarse (over 5 km). */
export const isApproximate = (/** @type {Where} */ place) => place.source === "ip" || (place.accuracy ?? 0) > 5000;

/**
 * The place as the widget shows it: "Oslo, Oslo County, Norway" ("Oslo, Oslo County" when `short`), or
 * "Near Oslo, Norway" for an approximate fix (an IP address is a region, never a suburb).
 * @param {Where} place
 * @param {boolean} [short] the town and its region (or country) only
 */
export function placeLabel(place, short = false) {
  const label = joinParts(short ? [place.name, place.region || place.country] : [place.name, place.region, place.country]);
  return isApproximate(place) ? `Near ${label}` : label;
}

/** A search candidate as one line, so homonyms tell apart: "Maadi, Cairo, Egypt". @param {Candidate} c */
export const candidateLine = (c) => joinParts([c.name, c.admin2, c.admin1, c.country]);

/**
 * A `search` answer as candidates (the plugin is untrusted: keep the well-formed ones).
 * @param {unknown} value
 * @returns {Candidate[]}
 */
export function readCandidates(value) {
  if (!Array.isArray(value)) return [];
  return value.filter((c) => typeof c === "object" && c !== null && typeof c.name === "string" && c.name !== "" && Number.isFinite(c.lat) && Number.isFinite(c.lon));
}

/**
 * The setting a picked candidate is saved as: its coordinates and, for showing, where it is.
 * @param {Candidate} c
 * @returns {Where}
 */
export function placeOf(c) {
  return { name: c.name, lat: c.lat, lon: c.lon, ...(c.admin1 ? { region: c.admin1 } : {}), ...(c.country ? { country: c.country } : {}) };
}

/**
 * What to tell the user when loading fails: `code` is the wire error code of the failed call,
 * `city` the name of a town looked up by name ("" for coordinates or automatic).
 * @param {string | undefined} code
 * @param {string} city
 */
export function errorInfo(code, city) {
  if (code === "NotFound") {
    return { title: "Unknown town", hint: city ? `Nothing found for "${city}". Choose the town again in this widget's settings.` : "Choose a town in this widget's settings." };
  }
  if (code === "PluginUnavailable") return { title: "Weather unavailable", hint: "Can't reach the weather service. Trying again every minute." };
  return { title: "Couldn't load the weather", hint: "Trying again every minute." };
}
