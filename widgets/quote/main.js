// @ts-check
import { $, clamp, onVisible, trackSettings, trackSize } from "./kit.js";
import { indexForPeriod, nextPeriodStart, otherIndex, periodOf } from "./pick.js";
import { QUOTES } from "./quotes.js";

/** @typedef {import("./quotes.js").Quote} Quote */

const MIN_FONT_PX = 11;
const COMPACT_BELOW_PX = 130; // shorter than this the glyph makes way for the text
const FADE_OUT_MS = 220;
const FADE_IN_MS = 420;

const card = $("#card");
const quoteEl = $("#quote");
const body = $("#body");
const text = $("#text");
const by = $("#by");
const author = $("#author");

const defaults = { mode: "daily", source: "builtin", custom: /** @type {unknown} */ ([]) };
let settings = defaults;
/** @type {readonly Quote[]} */
let pool = QUOTES;
let index = 0;
/** The rotation period last shown (NaN: none yet, so the next sync shows the scheduled quote). */
let period = Number.NaN;
let running = false;
let shown = false;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;

const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

/**
 * The custom list setting is plain JSON from the user's layout file: keep only usable quotes.
 * @param {unknown} value
 * @returns {Quote[]}
 */
function readCustom(value) {
  if (!Array.isArray(value)) return [];
  /** @type {Quote[]} */
  const out = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const quote = "text" in item && typeof item.text === "string" ? item.text.trim() : "";
    const name = "author" in item && typeof item.author === "string" ? item.author.trim() : "";
    if (quote) out.push({ text: quote, author: name });
  }
  return out;
}

// ---- fitting: the largest font size at which the whole quote fits its box

/** Whether the text, at its current font size, fits the space it may use. */
const fits = () => text.offsetHeight <= body.clientHeight + 0.5;

function fit() {
  const w = body.clientWidth;
  const h = body.clientHeight;
  if (w === 0 || h === 0) return;
  text.removeAttribute("data-clamped");
  let lo = MIN_FONT_PX;
  // Capped well below "fill the box": a short quote in a big widget stays a calm reading size.
  let hi = clamp(Math.min(w / 14, h / 3), 13, 24);
  text.style.fontSize = `${hi}px`;
  if (fits()) return;
  for (let i = 0; i < 8; i++) {
    const mid = (lo + hi) / 2;
    text.style.fontSize = `${mid}px`;
    if (fits()) lo = mid;
    else hi = mid;
  }
  text.style.fontSize = `${lo}px`;
  if (!fits()) {
    // Last resort: even the smallest size overflows, so show as many lines as fit, then an ellipsis.
    const lineHeight = parseFloat(getComputedStyle(text).lineHeight) || lo * 1.22;
    text.style.setProperty("--lines", String(Math.max(1, Math.floor(h / lineHeight))));
    text.toggleAttribute("data-clamped", true);
  }
}

// ---- showing a quote, with a fade between quotes

let fadeToken = 0;

/** Swaps the content through `update` with a fade, or at once when there is nothing to fade from. */
async function transition(/** @type {() => void} */ update) {
  const mine = ++fadeToken;
  for (const a of quoteEl.getAnimations()) a.cancel();
  if (!shown || reducedMotion.matches || document.documentElement.hasAttribute("data-hidden")) {
    update();
    return;
  }
  const out = quoteEl.animate([{ opacity: 1 }, { opacity: 0 }], { duration: FADE_OUT_MS, easing: "ease-out", fill: "forwards" });
  // A timer rather than `out.finished`: the swap must happen even if the page isn't being painted.
  await new Promise((resolve) => setTimeout(resolve, FADE_OUT_MS));
  if (mine !== fadeToken) return; // a newer transition took over
  update();
  out.cancel();
  quoteEl.animate(
    [
      { opacity: 0, transform: "translateY(4px)" },
      { opacity: 1, transform: "none" },
    ],
    { duration: FADE_IN_MS, easing: "cubic-bezier(0.2, 0.7, 0.2, 1)" },
  );
}

/** @param {number} next */
function show(next) {
  index = next;
  const quote = pool[index] ?? QUOTES[0];
  if (!quote) return;
  // Settings arrive more than once at load: don't fade to the very same quote.
  if (shown && text.textContent === quote.text && author.textContent === quote.author) return;
  void transition(() => {
    text.textContent = quote.text;
    author.textContent = quote.author;
    by.hidden = quote.author === "";
    shown = true;
    fit();
  });
}

// ---- rotation: one timer, aligned to the next period, only while visible

/** @param {string} value the `mode` setting */
const modeOf = (value) => (value === "hourly" || value === "click" ? value : "daily");

function arm() {
  clearTimeout(timer);
  timer = undefined;
  if (!running) return;
  const now = new Date();
  const start = nextPeriodStart(modeOf(settings.mode), now);
  if (start) timer = setTimeout(sync, start.getTime() - now.getTime() + 50);
}

/** Shows the scheduled quote when the period has changed (or a setting reset it), then re-arms. */
function sync() {
  const mode = modeOf(settings.mode);
  const now = new Date();
  const current = periodOf(mode, now);
  if (current !== period) {
    period = current;
    // Click mode has no schedule; it starts from today's quote and moves on by click.
    const basis = periodOf(mode === "click" ? "daily" : mode, now);
    show(indexForPeriod(basis, pool.length));
  }
  arm();
}

function next() {
  if (pool.length > 0) show(otherIndex(index, pool.length));
}

card.addEventListener("click", next);
card.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    next();
  }
});

onVisible(
  () => {
    running = true;
    sync();
  },
  () => {
    running = false;
    clearTimeout(timer);
  },
);

trackSettings(defaults, (value) => {
  settings = value;
  const custom = readCustom(value.custom);
  pool = value.source === "custom" && custom.length > 0 ? custom : value.source === "both" ? [...QUOTES, ...custom] : QUOTES;
  period = Number.NaN;
  sync();
});

trackSize(({ px: [, h] }) => card.toggleAttribute("data-compact", h < COMPACT_BELOW_PX));

// Refit whenever the space for the quote changes: a resize, the glyph hiding, or the user's
// surface changing the card's padding.
new ResizeObserver(fit).observe(body);
