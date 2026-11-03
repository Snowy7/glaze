// @ts-check
import { $, every, onVisible, trackSettings, trackSize } from "./kit.js";

/**
 * Response shapes of the weather plugin (plugins/weather).
 * @typedef {{ temperature: string, speed: string, precipitation: string }} Units
 * @typedef {{ name: string, country?: string, lat: number, lon: number }} Place
 * @typedef {{ location: Place, units: Units, temperature: number, apparent: number, humidity: number, condition: string, icon: string, is_day: boolean }} Current
 * @typedef {{ date: string, condition: string, icon: string, min: number, max: number }} Day
 * @typedef {{ location: Place, units: Units, days: Day[] }} Forecast
 */

const REFRESH_MS = 10 * 60_000; // the plugin caches for the same time
const RETRY_MS = 60_000;

const card = $("#card");
const els = {
  use: $("#icon-use", SVGUseElement),
  temp: $("#temp"),
  cond: $("#cond"),
  place: $("#place"),
  range: $("#range"),
  days: $("#days"),
  status: $("#status"),
};

const defaults = { city: "London", lat: "", lon: "", units: "metric" };
let settings = defaults;
/** Newest request wins: settings can change while a call is in flight. */
let generation = 0;
let lastOk = 0;
let lastTry = 0;
let failed = false;
let layout = { name: "tiny", days: 0 };
/** @type {{ current: Current, forecast: Forecast } | undefined} */
let data;

const weekday = new Intl.DateTimeFormat(undefined, { weekday: "short" });

/** @param {number} t */
const deg = (t) => `${Math.round(t)}°`;

/** @param {string} icon */
const href = (icon) => `#${/^[a-z-]+$/.test(icon) ? icon : "cloudy"}`;

/** `lat`/`lon` win when both parse; otherwise the city. */
function placeParams() {
  const lat = Number.parseFloat(settings.lat);
  const lon = Number.parseFloat(settings.lon);
  if (Number.isFinite(lat) && Number.isFinite(lon)) return { lat, lon };
  const city = settings.city.trim();
  return city ? { city } : undefined;
}

/** @param {string} message */
function showStatus(message) {
  els.status.hidden = false;
  els.status.textContent = message;
}

function render() {
  if (!data) return;
  const { current, forecast } = data;
  const today = forecast.days[0];
  card.dataset.state = "ready";
  els.status.hidden = true;
  els.use.setAttribute("href", href(current.icon));
  els.temp.textContent = deg(current.temperature);
  els.cond.textContent = current.condition;
  els.place.textContent = current.location.name;
  els.range.textContent = today ? `H ${deg(today.max)}   L ${deg(today.min)}` : "";

  // Today is already the big number, so the strip starts tomorrow.
  els.days.replaceChildren(...forecast.days.slice(1, 1 + layout.days).map(dayElement));
}

const SVG = "http://www.w3.org/2000/svg";

/**
 * @param {string} className
 * @param {string} text
 */
function span(className, text) {
  const el = document.createElement("span");
  el.className = className;
  el.textContent = text;
  return el;
}

/** @param {Day} d */
function dayElement(d) {
  const [y = 0, m = 1, day = 1] = d.date.split("-").map(Number);
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(SVG, "use");
  use.setAttribute("href", href(d.icon));
  svg.append(use);

  const temps = document.createElement("span");
  temps.className = "temps";
  temps.append(span("hi", deg(d.max)), span("lo", deg(d.min)));

  const el = document.createElement("div");
  el.className = "day";
  el.title = d.condition;
  el.append(span("name", weekday.format(new Date(y, m - 1, day))), svg, temps);
  return el;
}

async function load() {
  const params = placeParams();
  if (!params) {
    data = undefined;
    card.dataset.state = "error";
    return showStatus("Set a city in this widget's settings");
  }
  const mine = ++generation;
  lastTry = Date.now();
  try {
    const request = { ...params, units: settings.units };
    const [current, forecast] = await Promise.all([
      mypaper.plugin("weather", "current", request),
      mypaper.plugin("weather", "forecast", { ...request, days: 6 }),
    ]);
    if (mine !== generation) return;
    // The plugin's shapes are documented in docs/writing-plugins.md.
    data = { current: /** @type {Current} */ (current), forecast: /** @type {Forecast} */ (forecast) };
    lastOk = Date.now();
    failed = false;
    render();
  } catch (err) {
    if (mine !== generation) return;
    failed = true;
    // Keep showing the last good data if we have it.
    if (data) return;
    card.dataset.state = "error";
    // MypaperError: `code` is one of the documented wire codes.
    const code = /** @type {{ code?: string }} */ (err).code;
    showStatus(
      code === "NotFound"
        ? `No place called "${settings.city}"`
        : code === "PluginUnavailable"
          ? "Weather service unavailable"
          : "Couldn't load the weather",
    );
  }
}

/** Fetch when stale, or retry a failure after a minute. Cheap: the plugin has the cache. */
function maybeLoad() {
  if (!settingsKnown) return; // the first load comes with the settings
  const now = Date.now();
  if (failed ? now - lastTry >= RETRY_MS : now - lastOk >= REFRESH_MS) void load();
}

trackSize((size) => {
  const [w, h] = size.px;
  const wide = w >= 290;
  const tall = h >= 150;
  layout = !wide && !tall
    ? { name: "tiny", days: 0 }
    : wide && !tall
      ? { name: "side", days: w >= 380 ? 3 : 2 }
      : !wide
        ? { name: "stack", days: 3 }
        : { name: "full", days: w >= 380 ? 5 : 4 };
  card.dataset.layout = layout.name;
  render();
});

let settingsKnown = false;
trackSettings(defaults, (next) => {
  settings = next;
  settingsKnown = true;
  lastOk = 0;
  failed = false;
  // Hidden: wait. `every` below loads as soon as the widget is shown (nothing is fresh).
  if (mypaper.visible) void load();
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
