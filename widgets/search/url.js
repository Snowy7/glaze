// @ts-check
// Turns what the user typed into the one http(s) URL to open.

/** Engine presets: label shown in the chip, and the URL template (`{q}` is the encoded query). */
export const ENGINES = /** @type {const} */ ({
  duckduckgo: { label: "DuckDuckGo", url: "https://duckduckgo.com/?q={q}" },
  google: { label: "Google", url: "https://www.google.com/search?q={q}" },
  bing: { label: "Bing", url: "https://www.bing.com/search?q={q}" },
  brave: { label: "Brave", url: "https://search.brave.com/search?q={q}" },
  ecosia: { label: "Ecosia", url: "https://www.ecosia.org/search?q={q}" },
  startpage: { label: "Startpage", url: "https://www.startpage.com/do/search?q={q}" },
});

/** @typedef {keyof typeof ENGINES} EngineId */

/** @param {string} value @returns {value is EngineId} */
export const isEngineId = (value) => Object.hasOwn(ENGINES, value);

/**
 * @param {string} href
 * @returns {string | null} the normalized URL if it is http or https, else null
 */
function httpUrl(href) {
  try {
    const url = new URL(href);
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname !== "" ? url.href : null;
  } catch {
    return null;
  }
}

// A host with at least one dot and a letters-only last label, an IPv4 address, or localhost; then an
// optional port and path. No spaces, no scheme.
const DOMAIN_LIKE = /^(?:localhost|(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}|\d{1,3}(?:\.\d{1,3}){3})(?::\d{1,5})?(?:[/?#]\S*)?$/i;

/**
 * The URL to open directly when the text is an address (`https://x.y/z`, `github.com/foo`), else null.
 * @param {string} text
 */
export function directUrl(text) {
  const t = text.trim();
  if (t === "" || /\s/.test(t)) return null;
  if (/^https?:\/\//i.test(t)) return httpUrl(t);
  return DOMAIN_LIKE.test(t) ? httpUrl(`https://${t}`) : null;
}

/**
 * The URL template for the chosen engine, or null for a custom one that has no `{q}`.
 * @param {string} engine a preset id or "custom"
 * @param {string} customUrl
 */
export function templateFor(engine, customUrl) {
  if (isEngineId(engine)) return ENGINES[engine].url;
  const template = customUrl.trim();
  return template.includes("{q}") ? template : null;
}

/**
 * Chip label for an engine: the preset's name, or the custom template's host.
 * @param {string} engine
 * @param {string} customUrl
 */
export function engineLabel(engine, customUrl) {
  if (isEngineId(engine)) return ENGINES[engine].label;
  const probe = httpUrl(customUrl.trim().replaceAll("{q}", "q"));
  return probe ? new URL(probe).hostname.replace(/^www\./, "") : "Custom";
}

/**
 * What to launch for `query`: an address directly (when `openUrls`), else a search through the
 * template. The result is always an http(s) URL; `error` says why there is none.
 * @param {{ query: string, template: string | null, openUrls: boolean }} input
 * @returns {{ url: string } | { error: string }}
 */
export function resolveTarget({ query, template, openUrls }) {
  const q = query.trim();
  if (q === "") return { error: "Type something to search" };
  if (openUrls) {
    const direct = directUrl(q);
    if (direct) return { url: direct };
  }
  if (template === null) return { error: "The custom engine URL needs {q}" };
  const encoded = encodeURIComponent(q).replaceAll("%20", "+");
  const url = httpUrl(template.replaceAll("{q}", encoded));
  return url ? { url } : { error: "The engine URL must start with http(s)://" };
}
