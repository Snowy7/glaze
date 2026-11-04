// @ts-check
import { $, every, onVisible, setText, trackSettings, trackSize } from "./kit.js";
import { dayLabel, degrees, errorInfo, hourLabel, pickLayout, placeNow, rangeBar, sceneFor, slotCount, upcomingHours } from "./weather-logic.js";

/** @typedef {import("./weather-logic.js").Current} Current */
/** @typedef {import("./weather-logic.js").Forecast} Forecast */
/** @typedef {import("./weather-logic.js").Day} Day */
/** @typedef {import("./weather-logic.js").Hour} Hour */
/** @typedef {import("./weather-logic.js").Variant} Variant */

const REFRESH_MS = 10 * 60_000; // the plugin caches for the same time
const RETRY_MS = 60_000;
const VARIANTS = ["card", "compact", "forecast"];
// Smallest slots, in px, of the hourly strip (and its gap), a day row and a day column.
const SLOT_MIN = 46;
const SLOT_GAP = 6;
const ROW_MIN = 32;
const COL_MIN = 52;
const MAX_HOURS = 24;
const MAX_ROWS = 6;

const card = $("#card");
const els = {
  art: $("#art", SVGSVGElement),
  miniUse: $("#mini-use", SVGUseElement),
  temp: $("#temp"),
  cond: $("#cond"),
  place: $("#place"),
  hilo: $("#hilo"),
  hours: $("#hours"),
  days: $("#days"),
  statusTitle: $("#status-title"),
  statusHint: $("#status-hint"),
};
const layers = [...els.art.querySelectorAll("[data-layer]")].filter((g) => g instanceof SVGElement);

const defaults = { variant: "card", city: "London", lat: "", lon: "", units: "metric", animate: true };
let settings = defaults;
/** @type {Variant} */
let variant = "card";
/** Newest request wins: settings can change while a call is in flight. */
let generation = 0;
let lastOk = 0;
let lastTry = 0;
let failed = false;
let settingsKnown = false;
/** @type {{ current: Current, forecast: Forecast, fetchedAt: number } | undefined} */
let data;
let hourKey = "";

/** `lat`/`lon` win when both parse; otherwise the city. */
function placeParams() {
  const lat = Number.parseFloat(settings.lat);
  const lon = Number.parseFloat(settings.lon);
  if (Number.isFinite(lat) && Number.isFinite(lon)) return { lat, lon };
  const city = settings.city.trim();
  return city ? { city } : undefined;
}

/** @param {string} icon */
const iconHref = (icon) => `#i-${/^[a-z-]+$/.test(icon) ? icon : "cloudy"}`;

/**
 * @param {string} className
 * @param {string} [text]
 */
function span(className, text) {
  const el = document.createElement("span");
  el.className = className;
  if (text !== undefined) el.textContent = text;
  return el;
}

const SVG = "http://www.w3.org/2000/svg";
/** @param {string} icon */
function iconElement(icon) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "ico");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(SVG, "use");
  use.setAttribute("href", iconHref(icon));
  svg.append(use);
  return svg;
}

/** The big temperature and the facts around it, and the illustration's scene. */
function renderNow() {
  if (!data) return;
  const { current, forecast } = data;
  const today = forecast.days[0];
  setText(els.temp, degrees(current.temperature));
  setText(els.cond, current.condition);
  setText(els.place, current.location.name);
  setText(els.hilo, today ? `H ${degrees(today.max)}  L ${degrees(today.min)}` : "");
  els.miniUse.setAttribute("href", iconHref(current.icon));

  const scene = sceneFor(current.icon, current.code);
  els.art.dataset.scene = scene.scene;
  els.art.dataset.tone = scene.tone;
  els.art.dataset.intensity = scene.intensity;
  /** @type {Set<string>} */
  const shown = new Set(scene.layers);
  for (const layer of layers) layer.toggleAttribute("hidden", !shown.has(layer.dataset.layer ?? ""));
}

/** @param {Hour} h @param {boolean} first */
function hourSlot(h, first) {
  const slot = document.createElement("div");
  slot.className = first ? "slot tile cur" : "slot tile";
  const chance = h.precipitation_probability ?? 0;
  slot.append(
    span("t caps", first ? "Now" : hourLabel(h.time)),
    iconElement(h.icon),
    span("v num", degrees(h.temperature)),
    span("p num", chance >= 25 ? `${Math.round(chance)}%` : ""),
  );
  slot.title = h.condition;
  return slot;
}

/**
 * @param {Day} d
 * @param {number} index
 * @param {number} low the lowest low shown
 * @param {number} high the highest high shown
 * @param {boolean} columns a column of a strip (a tile) rather than a row of a list
 */
function daySlot(d, index, low, high, columns) {
  const el = document.createElement("div");
  el.className = columns ? "day tile" : "day";
  el.title = d.condition;
  const bar = rangeBar(d.min, d.max, low, high);
  const track = span("track");
  const fill = document.createElement("i");
  fill.style.marginLeft = `${bar.start}%`;
  fill.style.width = `${bar.size}%`;
  track.append(fill);
  const chance = d.precipitation_probability ?? 0;
  el.append(
    span("d", dayLabel(d.date, index)),
    iconElement(d.icon),
    span("pp num", chance >= 25 ? `${Math.round(chance)}%` : ""),
    span("hi num", degrees(d.max)),
    span("lo num", degrees(d.min)),
    track,
  );
  return el;
}

/** The hourly strip and the days, as many as their boxes hold (measured: it depends on the layout). */
function renderLists() {
  if (!data) return;
  const { forecast, fetchedAt, current } = data;
  const nHours = Math.min(MAX_HOURS, slotCount(els.hours.clientWidth, SLOT_MIN, SLOT_GAP));
  const hourly = forecast.hourly ?? [];
  const upcoming = upcomingHours(hourly, placeNow(current.time, fetchedAt, Date.now()), nHours);
  els.hours.replaceChildren(...upcoming.map((h, i) => hourSlot(h, i === 0)));
  els.hours.toggleAttribute("data-no-p", !upcoming.some((h) => (h.precipitation_probability ?? 0) >= 25));
  hourKey = upcoming[0]?.time ?? "";
  card.toggleAttribute("data-no-hours", hourly.length === 0);

  // The card's list starts tomorrow (today is the big number); the forecast's with today.
  const days = variant === "forecast" ? forecast.days : forecast.days.slice(1);
  const columns = card.dataset["days"] === "cols";
  const nDays = columns ? slotCount(els.days.clientWidth, COL_MIN, SLOT_GAP) : slotCount(els.days.clientHeight, ROW_MIN);
  const shown = days.slice(0, Math.min(nDays, columns ? 7 : MAX_ROWS));
  const low = Math.min(...shown.map((d) => d.min));
  const high = Math.max(...shown.map((d) => d.max));
  const first = variant === "forecast" ? 0 : 1;
  els.days.replaceChildren(...shown.map((d, i) => daySlot(d, i + first, low, high, columns)));
}

function render() {
  if (!data) return;
  card.dataset["state"] = "ready";
  renderNow();
  renderLists();
}

/** @param {string} title @param {string} hint */
function showError(title, hint) {
  data = undefined;
  card.dataset["state"] = "error";
  setText(els.statusTitle, title);
  setText(els.statusHint, hint);
}

async function load() {
  const params = placeParams();
  if (!params) {
    showError("Set a city", "Add a city (or a latitude and longitude) in this widget's settings.");
    return;
  }
  const mine = ++generation;
  lastTry = Date.now();
  try {
    const request = { ...params, units: settings.units };
    const [current, forecast] = await Promise.all([
      mypaper.plugin("weather", "current", request),
      mypaper.plugin("weather", "forecast", { ...request, days: 7 }),
    ]);
    if (mine !== generation) return;
    // The plugin's shapes are documented in docs/writing-plugins.md.
    data = { current: /** @type {Current} */ (current), forecast: /** @type {Forecast} */ (forecast), fetchedAt: Date.now() };
    lastOk = data.fetchedAt;
    failed = false;
    render();
  } catch (err) {
    if (mine !== generation) return;
    failed = true;
    // Keep showing the last good data if we have it.
    if (data) return;
    // MypaperError: `code` is one of the documented wire codes.
    const { title, hint } = errorInfo(/** @type {{ code?: string }} */ (err).code, settings.city);
    showError(title, hint);
  }
}

/** Fetch when stale, or retry a failure after a minute. Cheap: the plugin has the cache. */
function maybeLoad() {
  if (!settingsKnown) return; // the first load comes with the settings
  const now = Date.now();
  if (failed ? now - lastTry >= RETRY_MS : now - lastOk >= REFRESH_MS) void load();
  // The hour turned over since the strip was drawn.
  else if (data) {
    const first = upcomingHours(data.forecast.hourly ?? [], placeNow(data.current.time, data.fetchedAt, now), 1)[0];
    if (first && first.time !== hourKey) renderLists();
  }
}

// The size and the variant decide the layout (CSS arranges by data-layout) and what the days are.
let box = { width: 0, height: 0 };
function applyLayout() {
  const layout = pickLayout(variant, box.width, box.height);
  card.dataset.layout = layout;
  card.dataset.days = variant === "forecast" && layout !== "tall" ? "cols" : "rows";
  render();
}
trackSize(({ px: [width, height] }) => {
  box = { width, height };
  applyLayout();
});

// The lists' counts depend on their boxes, which the layout sets.
const lists = new ResizeObserver(renderLists);
lists.observe(els.hours);
lists.observe(els.days);

trackSettings(defaults, (next) => {
  const changedPlace = !settingsKnown || next.city !== settings.city || next.lat !== settings.lat || next.lon !== settings.lon || next.units !== settings.units;
  settings = next;
  settingsKnown = true;
  variant = /** @type {Variant} */ (VARIANTS.includes(next.variant) ? next.variant : "card");
  card.dataset.variant = variant;
  card.dataset.motion = next.animate ? "on" : "off";
  applyLayout();
  if (changedPlace) {
    lastOk = 0;
    failed = false;
    // Hidden: wait. `every` below loads as soon as the widget is shown (nothing is fresh).
    if (mypaper.visible) void load();
  }
});

// While visible: poll cheaply, and refresh when the plugin says its data changed.
every(30_000, maybeLoad);
/** @type {(() => void) | undefined} */
let stopUpdates;
onVisible(
  () => {
    stopUpdates = mypaper.onPlugin("weather", "updated", () => void load());
  },
  () => {
    stopUpdates?.();
    stopUpdates = undefined;
  },
);
