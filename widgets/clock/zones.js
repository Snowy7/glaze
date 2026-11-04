// @ts-check
// The World face's logic: which cities, and what a city's wall clock says. No DOM, so it is
// unit-tested (packages/widget-sdk/test/clock-zones.test.ts).

/** @typedef {{ label: string, tz: string }} Zone */
/** @typedef {{ hours: number, minutes: number, seconds: number, dayDiff: number, offset: number }} ZoneClock */

export const MAX_ZONES = 8;
const DAY_MS = 86_400_000;

/**
 * The `zones` list setting is untrusted JSON: keep well-formed items, at most MAX_ZONES.
 * @param {unknown} raw
 * @returns {Zone[]}
 */
export function parseZones(raw) {
  if (!Array.isArray(raw)) return [];
  /** @type {Zone[]} */
  const zones = [];
  for (const item of raw) {
    if (typeof item !== "object" || item === null) continue;
    const tz = "tz" in item && typeof item.tz === "string" ? item.tz.trim() : "";
    const label = "label" in item && typeof item.label === "string" ? item.label.trim() : "";
    if (tz || label) zones.push({ label, tz });
  }
  return zones.slice(0, MAX_ZONES);
}

/**
 * The name a city row shows: its label, else the zone's last part ("America/New_York" is "New York").
 * @param {Zone} zone
 */
export function cityName(zone) {
  return zone.label || zone.tz.split("/").pop()?.replaceAll("_", " ") || "Unknown";
}

/** @type {Map<string, Intl.DateTimeFormat | null>} null: a zone this engine doesn't know */
const formats = new Map();

/** @param {string} tz */
function formatFor(tz) {
  let format = formats.get(tz);
  if (format !== undefined) return format;
  try {
    // en-US with numeric fields, parsed below: the zone's wall clock as numbers.
    format = new Intl.DateTimeFormat("en-US", {
      timeZone: tz,
      hourCycle: "h23",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      second: "numeric",
    });
  } catch {
    format = null; // RangeError: unknown or empty zone name
  }
  formats.set(tz, format);
  return format;
}

/**
 * What the clock in `tz` says at `now`, and how it differs from the local one: `dayDiff` is -1, 0
 * or 1 (yesterday, today, tomorrow there), `offset` the difference in minutes (positive: ahead).
 * Null for a zone name Intl doesn't know.
 * @param {string} tz
 * @param {Date} now
 * @returns {ZoneClock | null}
 */
export function zoneClock(tz, now) {
  const format = tz ? formatFor(tz) : null;
  if (!format) return null;
  /** @type {Record<string, number>} */
  const f = {};
  for (const part of format.formatToParts(now)) f[part.type] = Number(part.value);
  const [year, month, day] = [f.year ?? 1970, (f.month ?? 1) - 1, f.day ?? 1];
  const hours = (f.hour ?? 0) % 24;
  const wall = Date.UTC(year, month, day, hours, f.minute ?? 0, f.second ?? 0);
  const zoneOffset = Math.round((wall - Math.floor(now.getTime() / 1000) * 1000) / 60_000);
  const localDay = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  return {
    hours,
    minutes: f.minute ?? 0,
    seconds: f.second ?? 0,
    dayDiff: Math.round((Date.UTC(year, month, day) - localDay) / DAY_MS),
    offset: zoneOffset + now.getTimezoneOffset(),
  };
}

/**
 * The line under a city: "Same time", "+6h", "−3h 30m", and the day when it isn't today
 * ("Tomorrow, +8h"). `dayWord` names the day (Intl.RelativeTimeFormat in the widget).
 * @param {ZoneClock} clock
 * @param {(dayDiff: number) => string} dayWord
 */
export function offsetLabel(clock, dayWord) {
  const abs = Math.abs(clock.offset);
  const sign = clock.offset < 0 ? "−" : "+";
  const hours = Math.floor(abs / 60);
  const minutes = abs % 60;
  const diff = clock.offset === 0 ? "Same time" : `${sign}${hours}h${minutes ? ` ${minutes}m` : ""}`;
  return clock.dayDiff === 0 ? diff : `${dayWord(clock.dayDiff)}, ${diff}`;
}

/** Day or night at a wall-clock hour (06:00 to 17:59 is day). @param {number} hours */
export const isDaytime = (hours) => hours >= 6 && hours < 18;

/**
 * How the World face lays its cities out in a box (CSS px): dials in a row (wide and tall enough,
 * like macOS's world clock), text columns (short and wide), or rows (everything else, in one or
 * two columns); and how many cities fit.
 * @param {number} width
 * @param {number} height
 * @param {number} count
 * @returns {{ mode: "dials" | "cols" | "rows", cols: number, shown: number }}
 */
export function worldLayout(width, height, count) {
  if (count === 0) return { mode: "rows", cols: 1, shown: 0 };
  const dialCols = Math.min(count, Math.floor(width / 80));
  if (height >= 150 && dialCols >= 2 && width / height >= 1.45) return { mode: "dials", cols: dialCols, shown: dialCols };
  if (height < 96) {
    const cols = Math.max(1, Math.min(count, Math.floor(width / 86)));
    return { mode: "cols", cols, shown: cols };
  }
  const cols = width >= 400 && count > 3 ? 2 : 1;
  const rows = Math.max(1, Math.floor(height / 46));
  return { mode: "rows", cols, shown: Math.min(count, rows * cols) };
}
