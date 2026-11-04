// @ts-check
// mypaper widget kit: small helpers over the global `mypaper` (loaded by mypaper.js).
// Source of truth is packages/widget-sdk/widget-kit/kit.js; `pnpm build` copies it into every
// bundled widget.
//
// The look (surface, text tone, variables, theme CSS) is applied by mypaper.js itself; see
// base.css. This file has no styling code.

const root = document.documentElement;

/** @typedef {Awaited<ReturnType<typeof mypaper.size.get>>} SizeInfo */

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
 * `px` is the size your layout really has: the widget's pixels divided by the appearance `scale`
 * (a scale of 1.5 lays the content out in a box a third smaller and zooms it back up).
 * @param {(size: SizeInfo) => void} cb
 * @returns {() => void}
 */
export function trackSize(cb) {
  /** @type {SizeInfo | undefined} */
  let size;
  let scale = 1;
  const emit = () => {
    if (!size) return;
    root.dataset.cols = String(size.cells[0]);
    root.dataset.rows = String(size.cells[1]);
    cb({ ...size, px: [size.px[0] / scale, size.px[1] / scale] });
  };
  /** @param {{ appearance: { scale: number } }} look */
  const setScale = (look) => {
    const next = Number.isFinite(look.appearance.scale) ? clamp(look.appearance.scale, 0.5, 2) : 1;
    if (next === scale) return;
    scale = next;
    emit();
  };
  void mypaper.size.get().then((s) => {
    size = s;
    emit();
  });
  void mypaper.appearance.get().then(setScale, () => {});
  const offSize = mypaper.size.onChange((s) => {
    size = s;
    emit();
  });
  const offLook = mypaper.appearance.onChange(setScale);
  return () => {
    offSize();
    offLook();
  };
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

/**
 * Calls `fn(dtMs, nowMs)` on every animation frame, only while the widget is visible; the loop
 * stops by itself when it is hidden (and `fps`, if given, caps the rate: 30 is plenty for a
 * visualizer, 10 for ambient motion). Returns a stop function.
 * @param {(dtMs: number, nowMs: number) => void} fn
 * @param {number} [fps]
 * @returns {() => void}
 */
export function frames(fn, fps = 60) {
  const minGap = 1000 / fps - 1;
  let raf = 0;
  let last = 0;
  /** @param {number} now */
  const tick = (now) => {
    raf = requestAnimationFrame(tick);
    if (now - last < minGap) return;
    const dt = last === 0 ? 16 : now - last;
    last = now;
    fn(dt, now);
  };
  return onVisible(
    () => {
      last = 0;
      raf = requestAnimationFrame(tick);
    },
    () => cancelAnimationFrame(raf),
  );
}

/**
 * Keeps a canvas's backing store at its CSS size times the device pixel ratio, and calls
 * `onResize(width, height, dpr)` (CSS px) now and whenever it changes. Draw in CSS px after
 * `ctx.setTransform(dpr, 0, 0, dpr, 0, 0)`.
 * @param {HTMLCanvasElement} canvas
 * @param {(width: number, height: number, dpr: number) => void} onResize
 * @returns {() => void}
 */
export function fitCanvas(canvas, onResize) {
  const apply = () => {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const { width, height } = canvas.getBoundingClientRect();
    // The page is laid out at 1/scale and scaled up; offsetWidth is the layout size.
    const w = canvas.offsetWidth || width;
    const h = canvas.offsetHeight || height;
    if (w < 1 || h < 1) return;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    onResize(w, h, dpr);
  };
  const observer = new ResizeObserver(apply);
  observer.observe(canvas);
  apply();
  return () => observer.disconnect();
}

/**
 * @param {number} n
 * @param {number} lo
 * @param {number} hi
 */
export const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n));

/** @param {number} n */
export const pad2 = (n) => String(n).padStart(2, "0");

/**
 * Sets `text` as the node's text, only when it changed (cheap to call every tick).
 * @param {Element} node
 * @param {string} text
 */
export function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

/**
 * A byte count as a short number and its unit: `{ n: "612", unit: "GB" }`.
 * @param {number} bytes
 */
export function formatBytes(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = Math.max(0, bytes);
  let i = 0;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return { n: n < 10 && i > 0 ? n.toFixed(1) : String(Math.round(n)), unit: units[i] ?? "B" };
}

/**
 * A rate in bytes per second, as a short number and its unit: `{ n: "1.4", unit: "MB/s" }`.
 * @param {number} bytesPerSecond
 */
export function formatRate(bytesPerSecond) {
  const { n, unit } = formatBytes(bytesPerSecond);
  return { n, unit: `${unit}/s` };
}

/**
 * Seconds as `3d 4h`, `4h 12m` or `12m`.
 * @param {number} seconds
 */
export function formatDuration(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

// Lifecycle flags every widget wants as CSS hooks (see base.css).
mypaper.on("visibility", (visible) => root.toggleAttribute("data-hidden", !visible));
mypaper.on("editmode", (editing) => root.toggleAttribute("data-editing", editing));
void mypaper.ready.then(() => {
  root.toggleAttribute("data-hidden", !mypaper.visible);
  root.toggleAttribute("data-editing", mypaper.editing);
});
