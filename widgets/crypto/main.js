// @ts-check
import { $, onVisible, trackSettings, trackSize } from "./kit.js";
import { formatChange, formatPrice, glyphOf, nameOf, nextDelayMs, padFor, parsePrices, pickLayout, readCoins, symbolOf } from "./logic.js";

/**
 * @typedef {{ price: number, change?: number }} Quote
 * @typedef {{ key: string, at: number, data: Record<string, Quote> }} Cache
 */

const API = "https://api.coingecko.com/api/v3/simple/price";
const CURRENCIES = ["usd", "eur", "gbp", "jpy"];
const TIMEOUT_MS = 15_000;
const SETTINGS_DEBOUNCE_MS = 800; // typing a coin id changes the settings many times
const MIN_GAP_MS = 10_000; // never two requests closer than this, whatever the settings do
const SVG = "http://www.w3.org/2000/svg";

const rows = $("#rows");
const msg = $("#msg");
const msgText = $("#msg-text");
const foot = $("#foot");

const defaults = { coins: /** @type {unknown} */ ([]), currency: "usd", showChange: true, refreshMinutes: 5 };
let settings = defaults;
/** @type {string[]} */
let coins = [];
let currency = "usd";
/** Identifies what was asked for; a cache is only valid for the key it was fetched with. */
let key = "";
/** @type {Cache | undefined} */
let cache;
let size = { w: 200, h: 200 };
let layout = pickLayout({ ...size, count: 0, pad: 0 });

/** Consecutive failed requests (a 429 counts); drives the backoff. */
let failures = 0;
let offline = false;
let lastTry = 0;
/** When the next request is due. */
let nextAt = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
/** @type {AbortController | undefined} */
let controller;
/** Newest request wins: settings can change, or the widget be hidden, while one is in flight. */
let generation = 0;
/** Read once, on the first settings: what the previous session left in storage. */
let stored = mypaper.storage.get("cache");

/** @param {unknown} value @returns {Cache | undefined} */
function readCache(value) {
  if (typeof value !== "object" || value === null) return undefined;
  if (!("key" in value) || typeof value.key !== "string" || !("at" in value) || typeof value.at !== "number") return undefined;
  if (!("data" in value) || typeof value.data !== "object" || value.data === null) return undefined;
  // Re-validate the quotes: storage is a file the user can edit.
  /** @type {Record<string, Quote>} */
  const data = {};
  for (const [id, quote] of Object.entries(value.data)) {
    if (typeof quote !== "object" || quote === null || !("price" in quote) || typeof quote.price !== "number") continue;
    data[id] = { price: quote.price, ...("change" in quote && typeof quote.change === "number" ? { change: quote.change } : {}) };
  }
  return { key: value.key, at: value.at, data };
}

// ---------------------------------------------------------------- rendering

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

/** A small filled triangle, rounded at the corners. @param {boolean} up */
function triangle(up) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 8 8");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("tri");
  const path = document.createElementNS(SVG, "path");
  path.setAttribute("d", up ? "M4 1.6 6.9 6.4H1.1z" : "M4 6.4 1.1 1.6h5.8z");
  svg.append(path);
  return svg;
}

/**
 * @param {string} id
 * @param {Quote | undefined} quote
 * @param {boolean} first first row of its column (no hairline above it)
 */
function rowElement(id, quote, first) {
  const row = document.createElement("div");
  row.className = "row";
  if (first) row.dataset.first = "";

  const label = document.createElement("div");
  label.className = "label";
  label.append(span("sym ellipsis", symbolOf(id)), span("name caps ellipsis", nameOf(id)));

  const value = document.createElement("div");
  value.className = "value";
  value.append(span("price num ellipsis", quote ? formatPrice(quote.price, currency, layout.narrow ? 7 : Infinity) : "—"));

  if (settings.showChange && quote?.change !== undefined) {
    const { text, up } = formatChange(quote.change);
    const change = span("chg chip num", text);
    change.prepend(triangle(up));
    change.dataset.dir = up ? "up" : "down";
    value.append(change);
  }
  // A coin badge (its sign on a tile) when the row is wide enough for one.
  if (!layout.narrow) row.append(span("badge tile", glyphOf(id)));
  row.append(label, value);
  return row;
}

/** @param {string} text */
function showMessage(text) {
  rows.replaceChildren();
  foot.hidden = true;
  msg.hidden = false;
  msgText.textContent = text;
}

function render() {
  if (coins.length === 0) return showMessage("No coins set");
  if (!cache) return showMessage(offline ? "Offline" : "Loading");
  const { data, at } = cache;

  msg.hidden = true;
  rows.dataset.mode = layout.mode;
  rows.toggleAttribute("data-narrow", layout.narrow);
  rows.style.setProperty("--cols", String(layout.cols));
  rows.style.setProperty("--per-col", String(layout.perCol));
  rows.replaceChildren(...coins.slice(0, layout.shown).map((id, i) => rowElement(id, data[id], i % layout.perCol === 0)));

  const time = new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  foot.hidden = !layout.footer;
  foot.toggleAttribute("data-offline", offline); // the live dot goes quiet
  // Narrow: just the time (and a dot when offline); there is no room for words.
  foot.textContent = layout.narrow ? `${offline ? "· " : ""}${time}` : `${offline ? "offline · " : ""}updated ${time}`;
}

function relayout() {
  const surface = document.documentElement.dataset.surface !== "none";
  layout = pickLayout({ ...size, count: coins.length, pad: padFor(size.w, size.h, surface) });
  render();
}

trackSize((s) => {
  size = { w: s.px[0], h: s.px[1] };
  relayout();
});
// The padding depends on whether there is a surface.
void mypaper.ready.then(() => mypaper.appearance.onChange(relayout));

// ---------------------------------------------------------------- loading

async function load() {
  timer = undefined;
  if (coins.length === 0) return;
  const mine = ++generation;
  controller?.abort();
  const ctl = (controller = new AbortController());
  const timeout = setTimeout(() => ctl.abort(), TIMEOUT_MS);
  lastTry = Date.now();
  const sentKey = key;
  const ids = coins;
  const cur = currency;
  try {
    // Commas stay literal; each id is encoded.
    const res = await fetch(`${API}?ids=${ids.map(encodeURIComponent).join(",")}&vs_currencies=${cur}&include_24hr_change=true`, {
      signal: ctl.signal,
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`); // 429 is the likely one
    const data = parsePrices(await res.json(), ids, cur);
    if (mine !== generation) return;
    cache = { key: sentKey, at: Date.now(), data };
    failures = 0;
    offline = false;
    void mypaper.storage.set("cache", cache);
  } catch {
    if (mine !== generation) return; // superseded or the widget was hidden: not a failure
    failures++;
    offline = true;
  } finally {
    clearTimeout(timeout);
  }
  nextAt = Date.now() + nextDelayMs(settings.refreshMinutes, failures);
  render();
  plan();
}

/** One timer for the next request, only while visible. Hidden, nothing is scheduled or in flight. */
function plan() {
  clearTimeout(timer);
  timer = undefined;
  if (!mypaper.visible || coins.length === 0) return;
  timer = setTimeout(() => void load(), Math.max(0, nextAt - Date.now()));
}

function stop() {
  clearTimeout(timer);
  timer = undefined;
  generation++;
  controller?.abort();
}

/** @param {typeof defaults} next */
async function applySettings(next) {
  const previous = key;
  settings = next;
  coins = readCoins(next.coins);
  currency = CURRENCIES.includes(next.currency) ? next.currency : "usd";
  key = `${coins.join(",")}|${currency}`;
  if (key !== previous) {
    const mine = key;
    cache = undefined;
    failures = 0;
    offline = false;
    stop();
    nextAt = Math.max(Date.now() + SETTINGS_DEBOUNCE_MS, lastTry + MIN_GAP_MS);
    const saved = readCache(await stored);
    stored = Promise.resolve(undefined); // only the first read can be the previous session's
    if (key !== mine) return; // changed again while reading: that call carries on
    if (saved?.key === key) {
      cache = saved; // paint now; a fresh one is only fetched when due
      nextAt = Math.max(saved.at + nextDelayMs(next.refreshMinutes, 0), lastTry + MIN_GAP_MS);
    }
  } else if (failures === 0 && lastTry) {
    nextAt = lastTry + nextDelayMs(next.refreshMinutes, 0); // the interval may have changed
  }
  relayout();
  plan();
}

trackSettings(defaults, (next) => void applySettings(next));
onVisible(plan, stop);
