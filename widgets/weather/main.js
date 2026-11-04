// @ts-check
import { $, every, onVisible, setText, trackSettings, trackSize } from "./kit.js";
import { candidateLine, dayLabel, degrees, errorInfo, hourLabel, pickLayout, placeLabel, placeNow, placeOf, placeRequest, rangeBar, readCandidates, readLocated, readPlace, sceneFor, slotCount, upcomingHours } from "./weather-logic.js";

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

// `city` is a place setting: null (automatic) or { name, lat?, lon? } (readPlace).
const defaults = { variant: "card", city: /** @type {unknown} */ (null), units: "metric", animate: true };
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

/** The place shown ("Oslo, Oslo County, Norway"); null shows where the weather came back from. */
/** @type {import("./weather-logic.js").Where | null} */
let shownPlace = null;
/** The plugin found no location: the status offers Windows' settings and the town picker. */
let locationFailed = false;

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
  // Roomy layouts show the whole name (over two lines if need be); the others the town and region.
  const place = shownPlace ?? readPlace(current.location) ?? { name: current.location.name };
  const roomy = card.dataset.layout === "tall" || card.dataset.layout === "side";
  setText(els.place, placeLabel(place, !roomy));
  els.place.title = `${placeLabel(place)} (change the town)`;
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

/** @param {string} title @param {string} hint @param {"error" | "locating"} [state] */
function showError(title, hint, state = "error") {
  data = undefined;
  card.dataset["state"] = state;
  $("#status-actions").hidden = !locationFailed;
  setText(els.statusTitle, title);
  setText(els.statusHint, hint);
}

async function load() {
  const picked = readPlace(settings.city);
  const mine = ++generation;
  lastTry = Date.now();
  /** @type {{ lat: number, lon: number } | { city: string }} */
  let params;
  locationFailed = false;
  if (picked) {
    params = placeRequest(picked);
    shownPlace = picked.lat !== undefined ? picked : null;
  } else {
    // Automatic: the plugin's locate (Windows' location, else the network's; it caches), then
    // the weather there. Asked apart so "no location" and "no weather" read differently.
    if (!data) showError("Locating…", "", "locating");
    try {
      const here = readLocated(await mypaper.plugin("weather", "locate", {}));
      if (mine !== generation) return;
      if (!here) throw new Error("no location");
      params = placeRequest(here);
      shownPlace = here;
    } catch {
      if (mine !== generation) return;
      failed = true;
      if (data) return; // keep the last good weather
      locationFailed = true;
      showError("Location unavailable", "Let desktop apps use your location (Settings › Privacy & security › Location), or choose a town.");
      return;
    }
  }
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
    const { title, hint } = errorInfo(/** @type {{ code?: string }} */ (err).code, "city" in params ? params.city : "");
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

// ---- the town picker (from "Choose a town", or a click on the place's name)

const picker = $("#picker");
const query = $("#town-q", HTMLInputElement);
const results = $("#town-list");
let searchId = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let searchTimer;

/** @param {boolean} open */
function showPicker(open) {
  picker.hidden = !open;
  $("#town-auto").hidden = readPlace(settings.city) === null;
  if (open) {
    query.value = "";
    results.replaceChildren();
    query.focus();
  }
}

/** Searches about 300 ms after the typing stops, from two characters; a newer query wins. */
function search() {
  clearTimeout(searchTimer);
  const text = query.value.trim();
  const mine = ++searchId;
  if (text.length < 2) return results.replaceChildren();
  searchTimer = setTimeout(async () => {
    try {
      const found = readCandidates(await mypaper.plugin("weather", "search", { query: text }));
      if (mine !== searchId) return;
      results.replaceChildren(
        ...(found.length === 0
          ? [span("picker-none", "No towns found")]
          : found.map((c) => {
              const item = document.createElement("button");
              item.type = "button";
              item.className = "picker-item ellipsis";
              item.setAttribute("role", "option");
              item.textContent = candidateLine(c);
              // Only a picked candidate sets the location, never the typed text.
              item.addEventListener("click", () => {
                showPicker(false);
                void mypaper.settings.set({ city: placeOf(c) });
              });
              return item;
            })),
      );
    } catch {
      if (mine === searchId) results.replaceChildren(span("picker-none", "Couldn't search right now"));
    }
  }, 300);
}

query.addEventListener("input", search);
query.addEventListener("keydown", (e) => e.key === "Escape" && showPicker(false));
$("#town-close").addEventListener("click", () => showPicker(false));
$("#town-auto").addEventListener("click", () => {
  showPicker(false);
  void mypaper.settings.set({ city: null });
});
$("#choose-town").addEventListener("click", () => showPicker(true));
els.place.addEventListener("click", () => showPicker(true));
$("#open-location").addEventListener("click", () => void mypaper.launch("ms-settings:privacy-location").catch(() => {}));
mypaper.on("editmode", (editing) => editing && showPicker(false));

trackSettings(defaults, (next) => {
  const changedPlace = !settingsKnown || JSON.stringify(next.city) !== JSON.stringify(settings.city) || next.units !== settings.units;
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
