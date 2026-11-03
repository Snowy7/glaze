// @ts-check
// The crypto widget's pure logic: ids, price and change formatting, the refresh backoff, the layout.

const MAX_COINS = 6;
const MAX_DELAY_MS = 30 * 60_000;

// The few well-known ids we label with a ticker and a name; anything else shows its id.
/** @type {Record<string, readonly [symbol: string, name: string]>} */
const KNOWN = {
  bitcoin: ["BTC", "Bitcoin"],
  ethereum: ["ETH", "Ethereum"],
  solana: ["SOL", "Solana"],
  ripple: ["XRP", "XRP"],
  dogecoin: ["DOGE", "Dogecoin"],
  cardano: ["ADA", "Cardano"],
  litecoin: ["LTC", "Litecoin"],
  polkadot: ["DOT", "Polkadot"],
  chainlink: ["LINK", "Chainlink"],
  binancecoin: ["BNB", "BNB"],
  tether: ["USDT", "Tether"],
  tron: ["TRX", "TRON"],
  "avalanche-2": ["AVAX", "Avalanche"],
  monero: ["XMR", "Monero"],
  stellar: ["XLM", "Stellar"],
};

/** @param {string} id */
export const symbolOf = (id) => KNOWN[id]?.[0] ?? id.toUpperCase();

/** @param {string} id */
export function nameOf(id) {
  const known = KNOWN[id];
  if (known) return known[1];
  return id.replace(/[-_.]+/g, " ").replace(/^\w/, (c) => c.toUpperCase());
}

/**
 * The `list` setting arrives as plain JSON from the user's layout file: keep unique, plausible
 * CoinGecko ids (lowercase letters, digits, `-_.`).
 * @param {unknown} value
 * @returns {string[]}
 */
export function readCoins(value) {
  if (!Array.isArray(value)) return [];
  const ids = new Set();
  for (const entry of value) {
    if (typeof entry !== "object" || entry === null || !("id" in entry) || typeof entry.id !== "string") continue;
    const id = entry.id.trim().toLowerCase();
    if (/^[a-z0-9][a-z0-9._-]*$/.test(id)) ids.add(id);
  }
  return [...ids].slice(0, MAX_COINS);
}

/**
 * `{ bitcoin: { usd: 67000, usd_24h_change: 2.4 } }` to `{ bitcoin: { price, change } }`; coins the
 * API didn't answer for (unknown ids) are left out.
 * @param {unknown} json
 * @param {string[]} ids
 * @param {string} currency lowercase, as in the request
 * @returns {Record<string, { price: number, change?: number }>}
 */
export function parsePrices(json, ids, currency) {
  /** @type {Record<string, { price: number, change?: number }>} */
  const out = {};
  if (typeof json !== "object" || json === null) return out;
  for (const id of ids) {
    const entry = /** @type {Record<string, unknown>} */ (json)[id];
    if (typeof entry !== "object" || entry === null) continue;
    const price = /** @type {Record<string, unknown>} */ (entry)[currency];
    const change = /** @type {Record<string, unknown>} */ (entry)[`${currency}_24h_change`];
    if (typeof price !== "number" || !Number.isFinite(price)) continue;
    out[id] = { price, ...(typeof change === "number" && Number.isFinite(change) ? { change } : {}) };
  }
  return out;
}

/**
 * A price in the currency with sensible decimals: none from 1,000 up, two from 1, then more as
 * coins get cheaper. When `maxChars` is smaller than the full text (a narrow widget), thousands
 * switch to a compact form: `$67.2K`.
 * @param {number} value
 * @param {string} currency ISO code, any case
 * @param {number} [maxChars]
 * @param {Intl.LocalesArgument} [locale]
 */
export function formatPrice(value, currency, maxChars = Infinity, locale) {
  const abs = Math.abs(value);
  const digits = abs >= 1000 ? 0 : abs >= 1 ? 2 : abs >= 0.01 ? 4 : 6;
  const code = currency.toUpperCase();
  const full = new Intl.NumberFormat(locale, {
    style: "currency",
    currency: code,
    currencyDisplay: "narrowSymbol", // `$`, not `US$`

    // Yen and friends have no minor unit: round the same way up from 1.
    minimumFractionDigits: code === "JPY" && abs >= 1 ? 0 : Math.min(digits, 2),
    maximumFractionDigits: code === "JPY" && abs >= 1 ? 0 : digits,
  }).format(value);
  if (full.length <= maxChars || abs < 1000) return full;
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: code,
    currencyDisplay: "narrowSymbol",
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

/**
 * The 24h change as `+2.4%` or `−1.8%` (a real minus sign), and its direction. A change that
 * rounds to zero counts as up.
 * @param {number} percent
 * @returns {{ text: string, up: boolean }}
 */
export function formatChange(percent) {
  const decimals = Math.abs(percent) >= 100 ? 0 : 1;
  const rounded = Number(percent.toFixed(decimals));
  const up = rounded >= 0;
  const body = `${Math.abs(rounded).toFixed(decimals)}%`;
  return { text: rounded === 0 ? body : `${up ? "+" : "−"}${body}`, up };
}

/**
 * Wait before the next request: the user's interval (2 to 30 minutes), doubled for every failure
 * in a row (a 429 counts) and capped at 30 minutes.
 * @param {number} baseMinutes
 * @param {number} failures
 */
export function nextDelayMs(baseMinutes, failures) {
  const base = Math.min(30, Math.max(2, baseMinutes)) * 60_000;
  return Math.min(MAX_DELAY_MS, base * 2 ** Math.max(0, failures));
}

const ROW_MIN = 40;
const ROW_MIN_NARROW = 58;
const FOOT = 18;
const clampN = (/** @type {number} */ n, /** @type {number} */ lo, /** @type {number} */ hi) => Math.min(hi, Math.max(lo, n));

/**
 * The card's padding as base.css computes it (`--pad`), for the layout maths below.
 * @param {number} w
 * @param {number} h
 * @param {boolean} surface whether a glass or solid card is behind the content
 */
export function padFor(w, h, surface) {
  const small = Math.min(w, h);
  return surface ? clampN(0.08 * small, 10, 20) : clampN(0.04 * small, 4, 12);
}

/**
 * How to show `count` coins in a w by h widget: one big coin when tiny, otherwise rows (in one or
 * two columns, filled column by column) for as many coins as fit, and a footer line when tall.
 * @param {{ w: number, h: number, count: number, pad: number }} box widget size in px
 * `narrow` (under 130 px of content) stacks each row: ticker over price over change.
 * @returns {{ mode: "single" | "rows", narrow: boolean, cols: number, perCol: number, shown: number, footer: boolean }}
 */
export function pickLayout({ w, h, count, pad }) {
  const iw = w - 2 * pad;
  const ih = h - 2 * pad;
  const narrow = iw < 130;
  if (narrow && ih < 110) return { mode: "single", narrow, cols: 1, perCol: 1, shown: Math.min(count, 1), footer: false };
  const footer = ih >= 140;
  const capacity = Math.max(1, Math.floor((ih - (footer ? FOOT : 0)) / (narrow ? ROW_MIN_NARROW : ROW_MIN)));
  // Two columns when wide, but not for a short list that fits in one (2 + 1 looks lopsided).
  const maxCols = iw >= 330 && (count >= 4 || count > capacity) ? 2 : 1;
  const shown = Math.max(1, Math.min(count, capacity * maxCols));
  const cols = Math.min(maxCols, shown);
  return { mode: "rows", narrow, cols, perCol: Math.ceil(shown / cols), shown: Math.min(count, shown), footer };
}
