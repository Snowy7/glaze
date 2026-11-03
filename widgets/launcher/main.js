// @ts-check
import { $, clamp, trackSettings, trackSize } from "./kit.js";

const grid = $("#grid");
const empty = $("#empty");

/** @typedef {{ label: string, target: string }} Shortcut */

const LABEL_TILE = { w: 68, h: 80 }; // icon + name
const COMPACT_MAX = 46; // icon-only tiles are square, shrinking down to COMPACT_MIN to fit everything
const COMPACT_MIN = 32;
const GAP = 6;

const defaults = { shortcuts: /** @type {unknown} */ ([]), labels: "auto" };
let settings = defaults;
/** @type {Shortcut[]} */
let shortcuts = [];
let size = { w: 200, h: 96 };

/**
 * The list setting arrives as plain JSON from the user's layout file: keep only usable items.
 * @param {unknown} value
 * @returns {Shortcut[]}
 */
function readShortcuts(value) {
  if (!Array.isArray(value)) return [];
  /** @type {Shortcut[]} */
  const out = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const label = "label" in item && typeof item.label === "string" ? item.label.trim() : "";
    const target = "target" in item && typeof item.target === "string" ? item.target.trim() : "";
    if (target) out.push({ label: label || target, target });
  }
  return out;
}

/**
 * First letter of the label, or of the target's file or host name.
 * @param {Shortcut} shortcut
 */
function monogram({ label, target }) {
  const fromTarget = target.replace(/^[a-z]+:\/\/(www\.)?/i, "").split(/[\\/]/).filter(Boolean).pop() ?? "";
  return Array.from(label || fromTarget)[0] ?? "?";
}

/**
 * How many tiles of this size fit in the card (the card pads by 6% of its short side, 8 to 14 px).
 * @param {number} tileW
 * @param {number} tileH
 */
function capacity(tileW, tileH) {
  const pad = clamp(0.06 * Math.min(size.w, size.h), 8, 14);
  const cols = Math.floor((size.w - 2 * pad + GAP) / (tileW + GAP));
  const rows = Math.floor((size.h - 2 * pad + GAP) / (tileH + GAP));
  return { cols: Math.max(1, cols), rows: Math.max(1, rows) };
}

/** The largest icon-only tile size at which every shortcut fits, or 0 if none does. */
function compactSizeFittingAll() {
  for (let s = COMPACT_MAX; s >= COMPACT_MIN; s -= 2) {
    const fit = capacity(s, s);
    if (fit.cols * fit.rows >= shortcuts.length) return s;
  }
  return 0;
}

/**
 * "auto": labels if every shortcut fits with labels; else icon-only if every one fits that way;
 * else whichever shows more (the rest scrolls).
 * @returns {{ mode: "labels" | "compact", cols: number, s: number }}
 */
function pickLayout() {
  const labeled = capacity(LABEL_TILE.w, LABEL_TILE.h);
  const labels = { mode: /** @type {const} */ ("labels"), cols: labeled.cols, s: 0 };
  const compact = (/** @type {number} */ s) => ({ mode: /** @type {const} */ ("compact"), cols: capacity(s, s).cols, s });

  if (settings.labels === "always") return labels;
  if (settings.labels === "never") return compact(compactSizeFittingAll() || COMPACT_MIN);
  if (labeled.cols * labeled.rows >= shortcuts.length) return labels;
  const fitting = compactSizeFittingAll();
  if (fitting) return compact(fitting);
  return labeled.cols * labeled.rows >= 4 ? labels : compact(COMPACT_MIN);
}

/** @param {HTMLElement} tile @param {string} message */
function flashFailure(tile, message) {
  tile.dataset.failed = "";
  tile.title = `Couldn't open: ${message}`;
  setTimeout(() => {
    delete tile.dataset.failed;
    tile.title = tile.dataset.name ?? "";
  }, 1600);
}

function render() {
  const { mode, cols, s } = pickLayout();
  grid.dataset.mode = mode;
  grid.style.setProperty("--cols", String(cols));
  grid.style.setProperty("--s", `${s}px`);
  empty.hidden = shortcuts.length > 0;

  grid.replaceChildren(
    ...shortcuts.map((s) => {
      const button = document.createElement("button");
      button.className = "tile";
      button.dataset.name = `${s.label}`;
      button.title = s.label;
      button.setAttribute("aria-label", `${s.label} (${s.target})`);

      const mono = document.createElement("span");
      mono.className = "mono";
      mono.textContent = monogram(s);
      const label = document.createElement("span");
      label.className = "label";
      label.textContent = s.label;
      button.append(mono, label);

      button.addEventListener("click", () => {
        mypaper.launch(s.target).catch((err) => flashFailure(button, err instanceof Error ? err.message : String(err)));
      });
      return button;
    }),
  );
}

trackSize((s) => {
  size = { w: s.px[0], h: s.px[1] };
  render();
});

trackSettings(defaults, (next) => {
  settings = next;
  shortcuts = readShortcuts(next.shortcuts);
  render();
});
