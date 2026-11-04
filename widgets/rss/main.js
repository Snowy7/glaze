// @ts-check
import { $, clamp, every, trackSettings } from "./kit.js";
import { relativeTime } from "./time.js";

/**
 * Shapes of the rss plugin's `feeds` reply (plugins/rss).
 * @typedef {{ title: string, link: string, source: string, published?: number, summary?: string }} Item
 * @typedef {{ url: string, message: string }} FeedError
 * @typedef {{ key: string, items: Item[], errors: FeedError[], at: number }} Cache
 */

const POLL_MS = 30_000; // how often the (cheap) "is it due?" check runs while visible
const RETRY_MS = 60_000; // after a failure
const FETCH_LIMIT = 30; // always ask for the most; `maxItems` only slices (so the cache survives it)
const MAX_FEEDS = 8;

const list = $("#list");
const msg = $("#msg");
const msgTitle = $("#msg-title");
const msgHint = $("#msg-hint");
const foot = $("#foot");
const head = $("#head");
const headTitle = $("#head-title");

const defaults = { feeds: /** @type {unknown} */ ([]), maxItems: 12, showSource: true, showTime: true, refreshMinutes: 20, variant: "list" };
let settings = defaults;
/** @type {string[]} */
let urls = [];
/** Identifies the feed set; the cache is only valid for the key it was fetched with. */
let key = "";
/** @type {Cache | undefined} */
let cache;
/** The last failure: a wire error code, or "feeds" when every feed failed. */
let failure = /** @type {string | undefined} */ (undefined);
/** Newest request wins: the feeds can change while a call is in flight. */
let generation = 0;
let lastOk = 0;
let lastTry = 0;
/** Read once, on the first settings: what the previous session left in storage. */
let stored = mypaper.storage.get("cache");

/**
 * The `list` setting arrives as plain JSON from the user's layout file: keep usable http(s) URLs.
 * @param {unknown} value
 * @returns {string[]}
 */
function readFeeds(value) {
  if (!Array.isArray(value)) return [];
  const seen = new Set();
  for (const entry of value) {
    if (typeof entry !== "object" || entry === null || !("url" in entry) || typeof entry.url !== "string") continue;
    const url = entry.url.trim();
    if (isWebUrl(url)) seen.add(url);
  }
  return [...seen].slice(0, MAX_FEEDS);
}

/** @param {string} url */
const isWebUrl = (url) => /^https?:\/\/\S+$/i.test(url);

/**
 * Keeps the well-formed items of a plugin reply (or of what storage holds).
 * @param {unknown} reply
 * @returns {{ items: Item[], errors: FeedError[] } | undefined}
 */
function readReply(reply) {
  if (typeof reply !== "object" || reply === null) return undefined;
  const rawItems = "items" in reply && Array.isArray(reply.items) ? reply.items : [];
  const rawErrors = "errors" in reply && Array.isArray(reply.errors) ? reply.errors : [];
  /** @type {Item[]} */
  const items = [];
  for (const it of rawItems) {
    if (typeof it !== "object" || it === null || !("title" in it) || typeof it.title !== "string" || !it.title.trim()) continue;
    items.push({
      title: it.title.trim(),
      link: "link" in it && typeof it.link === "string" ? it.link : "",
      source: "source" in it && typeof it.source === "string" ? it.source : "",
      ...("published" in it && typeof it.published === "number" ? { published: it.published } : {}),
      ...("summary" in it && typeof it.summary === "string" && it.summary ? { summary: it.summary } : {}),
    });
  }
  /** @type {FeedError[]} */
  const errors = [];
  for (const e of rawErrors) {
    if (typeof e === "object" && e !== null && "url" in e && typeof e.url === "string") {
      errors.push({ url: e.url, message: "message" in e && typeof e.message === "string" ? e.message : "" });
    }
  }
  return { items, errors };
}

/** @param {unknown} value @returns {Cache | undefined} */
function readCache(value) {
  if (typeof value !== "object" || value === null) return undefined;
  if (!("key" in value) || typeof value.key !== "string" || !("at" in value) || typeof value.at !== "number") return undefined;
  const parsed = readReply(value);
  return parsed && { key: value.key, at: value.at, ...parsed };
}

/** @param {string} link */
const isWebLink = (link) => /^https?:\/\//i.test(link);

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

/**
 * One headline. A button when it has a link to open, a plain block otherwise.
 * @param {Item} item
 * @param {number} nowSec
 * @param {boolean} lead the first one: it gets the accent bar (shown when it is large)
 */
function itemElement(item, nowSec, lead) {
  const el = document.createElement(isWebLink(item.link) ? "button" : "div");
  el.className = settings.variant === "cards" ? "item tile" : "item";
  el.setAttribute("role", "listitem");

  const title = document.createElement("div");
  title.className = "title";
  title.textContent = item.title;
  el.append(title);

  if (item.summary) el.append(span("summary", item.summary));

  const meta = document.createElement("div");
  meta.className = "meta";
  if (settings.showSource && item.source) meta.append(span("source ellipsis", item.source));
  if (settings.showTime && item.published !== undefined) meta.append(span("time num", relativeTime(item.published, nowSec)));
  if (meta.childElementCount) el.append(meta);

  if (el instanceof HTMLButtonElement) {
    el.title = item.title;
    el.addEventListener("click", () => {
      // Feeds are untrusted: only ever hand a plain web link to the shell.
      void mypaper.launch(item.link).catch(() => {
        el.title = "Couldn't open this link";
      });
    });
  }
  return el;
}

/**
 * @param {string} title
 * @param {string} [hint]
 */
function showMessage(title, hint = "") {
  list.replaceChildren();
  head.hidden = true;
  foot.hidden = true;
  msg.hidden = false;
  msgTitle.textContent = title;
  msgHint.textContent = hint;
  msgHint.hidden = !hint;
}

/** @param {string | undefined} code */
function failureMessage(code) {
  if (code === "PluginUnavailable") return "RSS service unavailable";
  if (code === "InvalidParams") return "Check the feed URLs";
  if (code === "feeds") return "Couldn't load these feeds";
  return "Couldn't load headlines";
}

function render() {
  if (urls.length === 0) return showMessage("No feeds set", "Add feeds in settings");
  if (!cache || cache.items.length === 0) {
    if (failure) return showMessage(failureMessage(failure));
    return showMessage(cache ? "No headlines" : "Loading");
  }

  msg.hidden = true;
  // The header names the feed when every headline comes from one, else just "Headlines".
  const sources = new Set(cache.items.map((item) => item.source).filter(Boolean));
  const only = sources.size === 1 ? [...sources][0] : undefined;
  headTitle.textContent = only ?? "Headlines";
  head.hidden = false;
  const nowSec = Date.now() / 1000;
  list.dataset.variant = settings.variant === "cards" ? "cards" : "list";
  list.replaceChildren(...cache.items.slice(0, clamp(Math.round(settings.maxItems), 3, 30)).map((item, i) => itemElement(item, nowSec, i === 0)));

  // One quiet line for anything the reader should know: a stale list, or feeds that failed.
  const failed = cache.errors.length;
  const note = failure
    ? "Showing saved headlines"
    : failed
      ? `${failed} of ${urls.length} feed${urls.length === 1 ? "" : "s"} unavailable`
      : "";
  foot.hidden = !note;
  foot.textContent = note;
  fit();
}

/**
 * Shows only the headlines that fit whole: never a clipped one. Runs after every render and
 * whenever the list resizes.
 */
function fit() {
  const rows = Array.from(list.children).filter((el) => el instanceof HTMLElement);
  for (const row of rows) row.hidden = false;
  const height = list.clientHeight;
  // Read every position first, then write: one layout.
  const bottoms = rows.map((row) => row.offsetTop + row.offsetHeight);
  // Always at least one headline, however small the widget (its title clamps to fit).
  rows.forEach((row, i) => (row.hidden = i > 0 && (bottoms[i] ?? 0) > height + 0.5));
}

new ResizeObserver(() => fit()).observe(list);

// ---------------------------------------------------------------- loading

async function load() {
  if (urls.length === 0) return;
  const mine = ++generation;
  lastTry = Date.now();
  try {
    const reply = readReply(await mypaper.plugin("rss", "feeds", { urls, limit: FETCH_LIMIT }));
    if (mine !== generation) return;
    if (!reply) throw new Error("unexpected reply");
    if (reply.items.length === 0 && reply.errors.length > 0) {
      // Every feed failed: keep what we have.
      failure = "feeds";
    } else {
      cache = { key, at: Date.now(), ...reply };
      lastOk = cache.at;
      failure = undefined;
      void mypaper.storage.set("cache", cache);
    }
  } catch (err) {
    if (mine !== generation) return;
    // MypaperError: `code` is one of the documented wire codes.
    failure = typeof err === "object" && err !== null && "code" in err && typeof err.code === "string" ? err.code : "Internal";
  }
  render();
}

/** Fetch when stale, or retry a failure after a minute. The plugin caches for 15 minutes anyway. */
function maybeLoad() {
  if (!mypaper.visible || urls.length === 0) return;
  const now = Date.now();
  const refreshMs = clamp(settings.refreshMinutes, 15, 120) * 60_000;
  if (failure ? now - lastTry >= RETRY_MS : now - lastOk >= refreshMs) void load();
}

/** @param {typeof defaults} next */
async function applySettings(next) {
  settings = next;
  urls = readFeeds(next.feeds);
  const nextKey = urls.join("\n");
  if (nextKey !== key) {
    key = nextKey;
    cache = undefined;
    failure = undefined;
    lastOk = 0;
    generation++;
    const saved = readCache(await stored);
    stored = Promise.resolve(undefined); // only the first read can be the previous session's
    if (key !== nextKey) return; // changed again while reading: that call carries on
    if (saved?.key === key) {
      cache = saved; // stale-while-revalidate: paint now, the due check below refreshes
      lastOk = saved.at;
    }
  }
  render();
  maybeLoad();
}

trackSettings(defaults, (next) => void applySettings(next));

// While visible: check every 30 s whether a refresh is due (it also re-renders, so "2h" ages),
// and once on becoming visible. Nothing runs while hidden.
every(POLL_MS, () => {
  maybeLoad();
  if (cache) render();
});
