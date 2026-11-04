// @ts-check
// App shortcuts' pure logic: the shortcut list, which icon size to ask the host for, and the dock's
// magnification (how big each icon gets near the pointer, and how far its neighbours are pushed).

/** @typedef {{ label: string, target: string }} Shortcut */

const MAX_SHORTCUTS = 24;
/** The icon sizes asked from the host: few distinct sizes, so the host's cache and ours get reused. */
const ICON_STEPS = [32, 48, 64, 96, 128, 192, 256];

/**
 * The list setting arrives as plain JSON from the user's layout file: keep usable items (a target
 * is required, the label falls back to the target's file name).
 * @param {unknown} value
 * @returns {Shortcut[]}
 */
export function readShortcuts(value) {
  if (!Array.isArray(value)) return [];
  /** @type {Shortcut[]} */
  const out = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const label = "label" in item && typeof item.label === "string" ? item.label.trim() : "";
    const target = "target" in item && typeof item.target === "string" ? item.target.trim() : "";
    if (target) out.push({ label: label || nameOf(target), target });
    if (out.length === MAX_SHORTCUTS) break;
  }
  return out;
}

/**
 * A readable name for a target: a link's host, the file name without extension, or the last part
 * of an AUMID.
 * @param {string} target
 */
export function nameOf(target) {
  const host = /^[a-z][a-z0-9+.-]*:\/\/(?:www\.)?([^/?#]+)/i.exec(target)?.[1];
  if (host) return host;
  const last = target.split(/[\\/]/).filter(Boolean).pop() ?? target;
  const bang = last.split("!").pop() ?? last;
  const bare = bang.replace(/\.(exe|lnk|bat|cmd)$/i, "").split(".").pop() ?? bang;
  return bare || target;
}

/**
 * The letter shown while an icon loads (or when Windows has none).
 * @param {Shortcut} shortcut
 */
export function monogram({ label, target }) {
  return (Array.from(label.trim() || nameOf(target))[0] ?? "?").toUpperCase();
}

/**
 * The pixel size to request for an icon drawn at `cssSize` CSS px: twice that for sharpness (the
 * page may be zoomed or the screen dense), rounded up to one of a few steps.
 * @param {number} cssSize
 * @param {number} [density] how many device pixels per CSS pixel to cover, at least 2
 */
export function iconPixels(cssSize, density = 2) {
  const wanted = cssSize * Math.max(2, density);
  return ICON_STEPS.find((step) => step >= wanted) ?? ICON_STEPS[ICON_STEPS.length - 1] ?? 256;
}

/**
 * The tile of the grid or list for an icon size: its size and the gaps, as laid out by style.css
 * (keep the two in sync).
 * @param {"grid" | "list"} variant
 * @param {number} size icon size
 * @param {boolean} labels
 */
function tileOf(variant, size, labels) {
  if (variant === "list") {
    return labels
      ? { w: 140, h: size * 0.7 + 10, gapX: 10, gapY: 4 }
      : { w: size * 0.7 + 16, h: size * 0.7 + 10, gapX: 10, gapY: 4 };
  }
  return { w: size * 1.25 + 20, h: size * 1.25 + 16 + (labels ? 20 : 0), gapX: 4, gapY: 6 };
}

/**
 * The biggest icon size, up to `max`, at which every shortcut fits in the box without scrolling
 * (`min` when none does).
 * @param {"grid" | "list"} variant
 * @param {number} count
 * @param {number} width
 * @param {number} height
 * @param {boolean} labels
 * @param {number} max
 * @param {number} [min]
 */
export function fitIconSize(variant, count, width, height, labels, max, min = 28) {
  for (let size = max; size > min; size -= 2) {
    if (capacity(variant, size, width, height, labels) >= count) return size;
  }
  return Math.min(min, max);
}

/**
 * How many tiles of the grid or list fit in the box at an icon size (at least 1): when even the
 * smallest icons can't show every shortcut, the widget shows this many.
 * @param {"grid" | "list"} variant
 * @param {number} size
 * @param {number} width
 * @param {number} height
 * @param {boolean} labels
 */
export function capacity(variant, size, width, height, labels) {
  const { w, h, gapX, gapY } = tileOf(variant, size, labels);
  const cols = Math.max(1, Math.floor((width + gapX) / (w + gapX)));
  const rows = Math.max(1, Math.floor((height + gapY) / (h + gapY)));
  return cols * rows;
}

/**
 * @typedef {Object} DockShape
 * @property {number} base an icon's resting size, px
 * @property {number} peak the scale of the icon right under the pointer, at least 1
 * @property {number} radius how far from the pointer (px) an icon still grows
 */

/**
 * How the dock looks with the pointer at `pointerX`: per icon, its scale (a smooth bell around the
 * pointer, `peak` right under it, 1 beyond `radius`) and a horizontal shift that makes room for the
 * grown neighbours, symmetric around the dock's centre so its middle stays put. `centers` are the
 * icons' resting centres, left to right, in the same px as `pointerX`.
 * @param {readonly number[]} centers
 * @param {number} pointerX
 * @param {DockShape} shape
 * @returns {{ scale: number, shift: number }[]}
 */
export function dockPose(centers, pointerX, { base, peak, radius }) {
  const scales = centers.map((center) => {
    const t = Math.abs(center - pointerX) / radius;
    return t >= 1 ? 1 : 1 + (Math.max(1, peak) - 1) * ((1 + Math.cos(Math.PI * t)) / 2);
  });
  const extras = scales.map((s) => base * (s - 1));
  const total = extras.reduce((a, b) => a + b, 0);
  let before = 0;
  return scales.map((scale, i) => {
    const extra = extras[i] ?? 0;
    const shift = before + extra / 2 - total / 2;
    before += extra;
    return { scale, shift };
  });
}

/**
 * How big the grown icon may get: the headroom above the dock, minus a strip for the hover label,
 * spent on the icon's extra height (it grows upward). 1 when there is no room.
 * @param {number} boxHeight the widget's height
 * @param {number} base resting icon size
 * @param {number} dockHeight the dock capsule's height at rest
 * @param {number} reserve what to keep free above the icon for the label
 * @param {number} [max]
 */
export function peakScale(boxHeight, base, dockHeight, reserve, max = 1.8) {
  const room = boxHeight - dockHeight - reserve;
  return Math.min(max, Math.max(1, 1 + room / Math.max(1, base)));
}
