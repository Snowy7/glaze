// @ts-check
// mypaper widget kit: small helpers over the global `mypaper` (loaded by mypaper.js).
// Source of truth is packages/widget-sdk/widget-kit/kit.js; `pnpm build` copies it into every
// bundled widget.

const root = document.documentElement;

/** @typedef {Awaited<ReturnType<typeof mypaper.size.get>>} SizeInfo */
/** @typedef {Awaited<ReturnType<typeof mypaper.theme.current>>} ThemeInfo */

/**
 * `document.querySelector` that throws when missing and narrows the type.
 * @template {Element} [T=HTMLElement]
 * @param {string} selector
 * @param {{ new (): T }} [type]
 * @returns {T}
 */
export function $(selector, type) {
  const found = document.querySelector(selector);
  if (!found || (type && !(found instanceof type))) throw new Error(`missing element: ${selector}`);
  // Without `type` the caller accepts any HTMLElement.
  return /** @type {T} */ (found);
}

/**
 * Settings merged over the defaults, now and on every change.
 * @template {Record<string, unknown>} S
 * @param {S} defaults
 * @param {(settings: S) => void} cb
 * @returns {() => void}
 */
export function trackSettings(defaults, cb) {
  /** @param {Record<string, unknown>} raw */
  const apply = (raw) => {
    // Values come from the user's layout file: keep only those of the default's type.
    /** @type {Record<string, unknown>} */
    const merged = { ...defaults };
    for (const [key, value] of Object.entries(raw)) {
      if (key in defaults && typeof value === typeof defaults[key]) merged[key] = value;
    }
    cb(/** @type {S} */ (merged));
  };
  void mypaper.settings.get().then(apply);
  return mypaper.settings.onChange(apply);
}

/**
 * Size now and on every change; also mirrored into `data-cols`/`data-rows` on <html>.
 * @param {(size: SizeInfo) => void} cb
 * @returns {() => void}
 */
export function trackSize(cb) {
  /** @param {SizeInfo} size */
  const apply = (size) => {
    root.dataset.cols = String(size.cells[0]);
    root.dataset.rows = String(size.cells[1]);
    cb(size);
  };
  void mypaper.size.get().then(apply);
  return mypaper.size.onChange(apply);
}

/**
 * Calls `start` while the widget is visible and `stop` while it isn't. Use it for anything that
 * runs on its own: timers, animation loops, subscriptions.
 * @param {() => void} start
 * @param {() => void} stop
 * @returns {() => void}
 */
export function onVisible(start, stop) {
  let on = false;
  /** @param {boolean} visible */
  const set = (visible) => {
    if (visible === on) return;
    on = visible;
    if (visible) start();
    else stop();
  };
  const off = mypaper.on("visibility", set);
  void mypaper.ready.then(() => set(mypaper.visible));
  return () => {
    off();
    set(false);
  };
}

/**
 * Runs `fn` now and then every `ms`, only while visible (and once more on becoming visible).
 * @param {number} ms
 * @param {() => void} fn
 * @returns {() => void}
 */
export function every(ms, fn) {
  /** @type {ReturnType<typeof setInterval> | undefined} */
  let timer;
  const stop = () => {
    clearInterval(timer);
    timer = undefined;
  };
  return onVisible(() => {
    stop();
    fn();
    timer = setInterval(fn, ms);
  }, stop);
}

let wallpaperAccent = false;
/** @type {(() => void) | undefined} */
let stopTheme;

/**
 * Optional accent: `--accent` follows the wallpaper palette when enabled (needs the "theme"
 * permission), and is white otherwise.
 * @param {boolean} enabled
 */
export function useWallpaperAccent(enabled) {
  wallpaperAccent = enabled;
  if (!enabled) {
    root.style.removeProperty("--accent");
    stopTheme?.();
    stopTheme = undefined;
    return;
  }
  /** @param {ThemeInfo} theme */
  const apply = (theme) => {
    if (wallpaperAccent && theme.palette) root.style.setProperty("--accent", theme.palette.accent);
  };
  mypaper.theme.current().then(apply, () => {});
  stopTheme ??= mypaper.theme.onChange(apply);
}

/**
 * @param {number} n
 * @param {number} lo
 * @param {number} hi
 */
export const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n));

// Lifecycle flags every widget wants as CSS hooks (see base.css).
mypaper.on("visibility", (visible) => root.toggleAttribute("data-hidden", !visible));
mypaper.on("editmode", (editing) => root.toggleAttribute("data-editing", editing));
void mypaper.ready.then(() => {
  root.toggleAttribute("data-hidden", !mypaper.visible);
  root.toggleAttribute("data-editing", mypaper.editing);
});
