// @ts-check
import { $, onVisible, pad2, trackSettings } from "./kit.js";

const defaults = {
  zones: [
    { label: "New York", tz: "America/New_York" },
    { label: "London", tz: "Europe/London" },
    { label: "Tokyo", tz: "Asia/Tokyo" },
    { label: "Sydney", tz: "Australia/Sydney" },
  ],
  format24: true,
  showOffset: true,
};
let settings = defaults;

const list = $("#list");
const DAY_MS = 86_400_000;
const MAX_ZONES = 8;

const SUN = `<svg class="glyph" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4" /><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6" /></svg>`;
const MOON = `<svg class="glyph" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.2A8.2 8.2 0 0 1 9.8 4a8.3 8.3 0 1 0 10.2 10.2Z" /></svg>`;

/** @typedef {{ label: string, tz: string }} Zone */

/**
 * The list setting is untrusted: keep well-formed items only.
 * @param {unknown} raw
 * @returns {Zone[]}
 */
function parseZones(raw) {
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

// --- time zone math --------------------------------------------------------------------------

/** @typedef {{ display: Intl.DateTimeFormat, parts: Intl.DateTimeFormat }} Formats */
/** @type {Map<string, Formats | null>} null = not a zone this engine knows */
const formatCache = new Map();

/** Formatters for a zone, cached; null when the zone is invalid (Intl throws a RangeError). */
function formatsFor(/** @type {string} */ tz) {
  const key = `${settings.format24}|${tz}`;
  const cached = formatCache.get(key);
  if (cached !== undefined) return cached;
  /** @type {Formats | null} */
  let formats = null;
  try {
    formats = {
      display: new Intl.DateTimeFormat(undefined, {
        timeZone: tz,
        hour: settings.format24 ? "2-digit" : "numeric",
        minute: "2-digit",
        hourCycle: settings.format24 ? "h23" : "h12",
      }),
      // en-US with numeric fields: parsed below to get the zone's wall clock.
      parts: new Intl.DateTimeFormat("en-US", {
        timeZone: tz,
        hourCycle: "h23",
        year: "numeric",
        month: "numeric",
        day: "numeric",
        hour: "numeric",
        minute: "numeric",
        second: "numeric",
      }),
    };
  } catch {
    // Unknown or empty zone name.
  }
  formatCache.set(key, formats);
  return formats;
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

/** `+5h`, `-3h`, `+5:30` */
function signedHours(/** @type {number} */ minutes) {
  const abs = Math.abs(minutes);
  const m = abs % 60;
  return `${minutes < 0 ? "-" : "+"}${Math.floor(abs / 60)}${m ? `:${pad2(m)}` : "h"}`;
}

/**
 * Everything a row shows for one zone at `now`.
 * @param {Formats} formats
 * @param {Date} now
 */
function readZone(formats, now) {
  /** @type {Record<string, number>} */
  const f = {};
  for (const part of formats.parts.formatToParts(now)) f[part.type] = Number(part.value);
  const wallClock = Date.UTC(f.year ?? 0, (f.month ?? 1) - 1, f.day ?? 1, (f.hour ?? 0) % 24, f.minute ?? 0, f.second ?? 0);
  const zoneOffset = Math.round((wallClock - Math.floor(now.getTime() / 1000) * 1000) / 60_000);
  const localDay = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  const zoneDay = Date.UTC(f.year ?? 0, (f.month ?? 1) - 1, f.day ?? 1);

  const pieces = formats.display.formatToParts(now);
  return {
    hour: (f.hour ?? 0) % 24,
    hm: pieces
      .filter((p) => p.type !== "dayPeriod")
      .map((p) => p.value)
      .join("")
      .trim(),
    period: pieces.find((p) => p.type === "dayPeriod")?.value ?? "",
    dayDiff: Math.round((zoneDay - localDay) / DAY_MS),
    hoursDiff: zoneOffset - -now.getTimezoneOffset(),
  };
}

// --- layout: how many columns and rows fit ----------------------------------------------------

const MIN_ROW = 38;
const MAX_ROW = 84;
const IDEAL_ROW = 56;
const IDEAL_COL = 200;
const MIN_COL = 180;
const ROW_GAP = 0;

/** @type {{ cols: number, rows: number, rowHeight: number, shown: number }} */
let layout = { cols: 1, rows: MAX_ZONES, rowHeight: IDEAL_ROW, shown: MAX_ZONES };

/**
 * Picks the column count that shows the most cities, then the calmest row and column proportions.
 * @param {number} width
 * @param {number} height
 * @param {number} count
 */
function fit(width, height, count) {
  const rowsFit = Math.max(1, Math.floor((height + ROW_GAP) / (MIN_ROW + ROW_GAP)));
  const maxCols = Math.max(1, Math.min(4, Math.floor(width / MIN_COL)));
  let best = { cols: 1, rows: 1, rowHeight: MAX_ROW, shown: 0 };
  let bestScore = -Infinity;
  for (let cols = 1; cols <= maxCols; cols++) {
    const rows = Math.max(1, Math.min(rowsFit, Math.ceil(count / cols)));
    const shown = Math.min(count, cols * rows);
    const rowHeight = Math.min(MAX_ROW, (height - (rows - 1) * ROW_GAP) / rows);
    const score = shown * 1000 - Math.abs(rowHeight - IDEAL_ROW) - Math.abs(width / cols - IDEAL_COL) * 0.3 - (1 - (rows * rowHeight) / height) * 120;
    if (score > bestScore) {
      bestScore = score;
      best = { cols, rows, rowHeight, shown };
    }
  }
  return best;
}

// --- rendering --------------------------------------------------------------------------------

/**
 * One row's markup; every text goes in through textContent.
 * @param {{ glyph: string, name: string, sub: string, hm: string, period: string }} c
 */
function rowEl(c) {
  const el = document.createElement("div");
  el.className = "row";
  el.innerHTML = `${c.glyph}<div class="who"><span class="name ellipsis"></span><span class="sub caps ellipsis"></span></div><div class="time num"><span class="hm"></span><span class="ap"></span></div>`;
  const text = { ".name": c.name, ".sub": c.sub, ".hm": c.hm, ".ap": c.period };
  for (const [selector, value] of Object.entries(text)) {
    const node = el.querySelector(selector);
    if (node) node.textContent = value;
  }
  el.classList.toggle("no-sub", !c.sub);
  return el;
}

/** @param {Zone} zone @param {Date} now */
function row(zone, now) {
  const formats = zone.tz ? formatsFor(zone.tz) : null;
  const name = zone.label || zone.tz.split("/").pop()?.replaceAll("_", " ") || "Unknown";
  if (!formats) {
    const el = rowEl({ glyph: MOON, name, sub: "unknown zone", hm: "--:--", period: "" });
    el.classList.add("unknown");
    return el;
  }
  const info = readZone(formats, now);
  const day = info.hour >= 6 && info.hour < 18;
  // "+6h" on the same day, "Tomorrow · +14h" otherwise; "Same time" as you.
  const offset = info.hoursDiff === 0 ? "Same time" : signedHours(info.hoursDiff);
  const sub = !settings.showOffset ? "" : info.dayDiff === 0 ? offset : `${relative.format(info.dayDiff, "day")} · ${offset}`;
  const el = rowEl({ glyph: day ? SUN : MOON, name, sub, hm: info.hm, period: info.period });
  el.classList.toggle("day", day);
  return el;
}

/** @type {Zone[]} */
let zones = parseZones(defaults.zones);

function render() {
  const now = new Date();
  if (zones.length === 0) {
    const empty = rowEl({ glyph: MOON, name: "No cities", sub: "add one in settings", hm: "", period: "" });
    empty.classList.add("unknown", "first");
    list.replaceChildren(empty);
    return;
  }
  list.style.setProperty("--cols", String(layout.cols));
  list.style.setProperty("--rows", String(layout.rows));
  list.style.setProperty("--rh", `${layout.rowHeight}px`);
  list.replaceChildren(
    ...zones.slice(0, layout.shown).map((zone, i) => {
      const el = row(zone, now);
      el.classList.toggle("first", i % layout.rows === 0); // no hairline above a column's first row
      return el;
    }),
  );
}

// The list's own box is what the rows share (the card's padding is already out of it).
function relayout() {
  if (list.clientHeight === 0 || list.clientWidth === 0) return;
  layout = fit(list.clientWidth, list.clientHeight, zones.length);
  render();
}

// One timer, aligned to the next minute boundary, and only while visible.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
function tick() {
  render();
  timer = setTimeout(tick, 60_000 - (Date.now() % 60_000) + 5);
}
onVisible(tick, () => clearTimeout(timer));

// Measured after layout, so a size or scale change is always seen at its final box.
new ResizeObserver(relayout).observe(list);
trackSettings(defaults, (next) => {
  settings = next;
  zones = parseZones(next.zones);
  relayout();
});

