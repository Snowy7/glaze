// @ts-check
// The launcher: your apps with their real icons (`mypaper.appIcon`), as a dock whose icons magnify
// toward the pointer (the maths is in apps.js), an icon grid, or a list; optionally under a web
// search bar (url.js). One row tall, or with no apps, it is just the search bar. Layout and look
// live in style.css; this file builds the buttons, loads the icons, drives the dock and the search.
import { $, clamp, trackSettings } from "./kit.js";
import { capacity, dockPose, fitIconSize, iconPixels, monogram, peakScale, readShortcuts } from "./apps.js";
import { engineLabel, resolveTarget, templateFor } from "./url.js";

/** @typedef {import("./apps.js").Shortcut} Shortcut */
/** @typedef {{ button: HTMLButtonElement, img: HTMLImageElement, mono: HTMLElement, shortcut: Shortcut }} Item */

const card = $("#card");
const area = $("#area");
const input = $("#q", HTMLInputElement);
const engineEl = $("#engine");
const errorEl = $("#err");

// The dock's spacing in layout px (style.css reads it from --dgap and --dpad).
const ROOMY = { gap: 8, pad: 8 };
const TIGHT = { gap: 4, pad: 5 };
/** Headroom kept free above a grown icon for the hover label. */
const TIP_RESERVE = 30;

const VARIANTS = ["dock", "grid", "list"];
const defaults = {
  variant: "dock",
  iconSize: 48,
  labels: true,
  shortcuts: /** @type {unknown} */ ([]),
  search: true,
  engine: "duckduckgo",
  customUrl: "https://duckduckgo.com/?q={q}",
};
let settings = defaults;
/** @type {Shortcut[]} */
let shortcuts = [];
let box = { w: 0, h: 0 };
/** @type {Item[]} */
let items = [];
/** What the DOM was built for. */
let builtKey = "";

// ---------------------------------------------------------------- icons

/** Icons by target and requested size; a failed request is remembered as `null` (a letter tile). @type {Map<string, Promise<string | null>>} */
const iconCache = new Map();

/** @param {string} target @param {number} px */
function loadIcon(target, px) {
  const key = `${px}|${target}`;
  let icon = iconCache.get(key);
  if (!icon) {
    icon = mypaper.appIcon(target, px).catch(() => null);
    iconCache.set(key, icon);
  }
  return icon;
}

/** Asks for every icon at the right size and swaps it in over the letter tile when it arrives. @param {number} cssSize */
function paintIcons(cssSize) {
  const px = iconPixels(cssSize);
  for (const item of items) {
    if (item.img.dataset["px"] === String(px)) continue;
    item.img.dataset["px"] = String(px);
    void loadIcon(item.shortcut.target, px).then(async (url) => {
      // A newer size may have been asked for while this one loaded.
      if (!url || item.img.dataset["px"] !== String(px)) return;
      item.img.src = url;
      await item.img.decode().catch(() => {});
      item.img.hidden = false;
      item.mono.hidden = true;
    });
  }
}

// ---------------------------------------------------------------- items

/**
 * @param {Shortcut} shortcut
 * @param {boolean} plate whether the icon sits on a rounded plate (the grid)
 * @returns {Item}
 */
function makeItem(shortcut, plate) {
  const button = document.createElement("button");
  button.className = "app";
  button.setAttribute("aria-label", shortcut.label);
  const ico = document.createElement("span");
  ico.className = plate ? "ico tile" : "ico";
  const mono = document.createElement("b");
  mono.className = "mono";
  mono.textContent = monogram(shortcut);
  const img = document.createElement("img");
  img.alt = "";
  img.draggable = false;
  img.hidden = true;
  ico.append(mono, img);
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = shortcut.label;
  button.append(ico, name);

  button.addEventListener("click", () => {
    button.dataset["bounce"] = "";
    setTimeout(() => delete button.dataset["bounce"], 1100);
    mypaper.launch(shortcut.target).catch(() => {
      button.dataset["failed"] = "";
      setTimeout(() => delete button.dataset["failed"], 700);
    });
  });
  return { button, img, mono, shortcut };
}

// ---------------------------------------------------------------- the dock

/** @type {HTMLElement | undefined} */
let dock;
/** @type {HTMLElement | undefined} */
let tip;
let dockShape = { base: 48, peak: 1.5, radius: 110 };
/** Resting centres of the icons in client px, measured when the pointer enters. */
let centers = /** @type {number[]} */ ([]);
let pointerX = 0;
let frame = 0;

function measureDock() {
  if (!dock) return;
  const rect = dock.getBoundingClientRect();
  const k = rect.width / dock.offsetWidth || 1;
  centers = items.map(({ button }) => rect.left + k * (button.offsetLeft + button.offsetWidth / 2));
}

function poseDock() {
  frame = 0;
  if (!dock || !tip || centers.length !== items.length) return;
  const k = dock.getBoundingClientRect().width / dock.offsetWidth || 1;
  const pose = dockPose(centers, pointerX, { ...dockShape, base: dockShape.base * k, radius: dockShape.radius * k });
  // The capsule widens by what the icons gained, evenly on both sides (the shifts assume that).
  dock.style.setProperty("--grow", `${pose.reduce((sum, p) => sum + dockShape.base * (p.scale - 1), 0).toFixed(1)}px`);
  let nearest = 0;
  pose.forEach(({ scale, shift }, i) => {
    const item = items[i];
    if (!item) return;
    item.button.style.transform = `translateX(${(shift / k).toFixed(2)}px) scale(${scale.toFixed(3)})`;
    if (Math.abs((centers[i] ?? 0) - pointerX) < Math.abs((centers[nearest] ?? 0) - pointerX)) nearest = i;
  });

  const item = items[nearest];
  const at = pose[nearest];
  if (!item || !at || !settings.labels) return void (tip && (tip.hidden = true));
  tip.textContent = item.shortcut.label;
  tip.hidden = false;
  // Above the grown icon (it grows upward from the dock), kept inside the widget.
  const x = dock.offsetLeft + item.button.offsetLeft + item.button.offsetWidth / 2 + at.shift / k;
  const y = dock.offsetTop + item.button.offsetTop - item.button.offsetHeight * (at.scale - 1) - 6;
  const half = tip.offsetWidth / 2 + 2;
  tip.style.left = `${clamp(x, half, Math.max(half, area.clientWidth - half)).toFixed(1)}px`;
  tip.style.top = `${Math.max(tip.offsetHeight + 2, y).toFixed(1)}px`;
}

function resetDock() {
  cancelAnimationFrame(frame);
  frame = 0;
  for (const { button } of items) button.style.transform = "";
  dock?.style.removeProperty("--grow");
  if (tip) tip.hidden = true;
}

/** Sizes the dock for the widget's box: the icons shrink to fit, and the growth is what the headroom allows. */
function fitDock() {
  if (!dock || box.w === 0) return 48;
  const n = Math.max(1, items.length);
  // Roomy spacing, unless the icons would have to get small for it.
  let { gap, pad } = ROOMY;
  let byWidth = Math.floor((box.w - 2 * pad - (n - 1) * gap) / n);
  if (byWidth < 36) {
    ({ gap, pad } = TIGHT);
    byWidth = Math.floor((box.w - 2 * pad - (n - 1) * gap) / n);
  }
  const byHeight = Math.floor(box.h * 0.44);
  const base = clamp(Math.min(settings.iconSize, byWidth, byHeight), 16, 96);
  // The dock is centred and grows upward: the room above it is half of the height left over.
  const dockHeight = base + 2 * pad;
  const peak = peakScale((box.h + dockHeight) / 2, base, dockHeight, TIP_RESERVE);
  dockShape = { base, peak, radius: clamp(base * 2.4, 70, 190) };
  dock.style.setProperty("--base", `${base}px`);
  dock.style.setProperty("--dgap", `${gap}px`);
  dock.style.setProperty("--dpad", `${pad}px`);
  return base;
}

// ---------------------------------------------------------------- build and fit

function build() {
  const variant = settings.variant;
  resetDock();
  area.className = `area v-${variant}`;
  area.dataset["labels"] = String(settings.labels);
  items = shortcuts.map((s) => makeItem(s, variant === "grid"));
  dock = undefined;
  tip = undefined;

  card.toggleAttribute("data-no-apps", items.length === 0);
  if (items.length === 0) {
    const hint = document.createElement("div");
    hint.className = "state";
    hint.innerHTML = '<div class="state-title">No apps yet</div><div class="state-hint">Add some in this widget\'s Settings</div>';
    area.replaceChildren(hint);
    return;
  }
  if (variant === "dock") {
    dock = document.createElement("div");
    dock.className = "dock tile";
    dock.append(...items.map((i) => i.button));
    tip = document.createElement("div");
    tip.className = "tip chip";
    tip.hidden = true;
    area.replaceChildren(dock, tip);
    dock.addEventListener("pointerenter", measureDock);
    dock.addEventListener("pointermove", (e) => {
      pointerX = e.clientX;
      if (!frame) frame = requestAnimationFrame(poseDock);
    });
    dock.addEventListener("pointerleave", resetDock);
  } else {
    area.replaceChildren(...items.map((i) => i.button));
  }
  fit();
}

/** Sets the icon size and asks for icons that are sharp at it. */
function fit() {
  if (items.length === 0) return;
  const variant = settings.variant === "list" ? "list" : "grid";
  const size = settings.variant === "dock" ? fitDock() : fitIconSize(variant, items.length, box.w, box.h, settings.labels, settings.iconSize);
  // A box too small for every app shows as many as fit, never a clipped row.
  const room = settings.variant === "dock" ? items.length : capacity(variant, size, box.w, box.h, settings.labels);
  items.forEach((item, i) => (item.button.hidden = i >= room));
  area.style.setProperty("--icon", `${size}px`);
  paintIcons(size);
}

function render() {
  const variant = VARIANTS.includes(settings.variant) ? settings.variant : "dock";
  settings = { ...settings, variant };
  const key = JSON.stringify([variant, settings.labels, shortcuts]);
  if (key !== builtKey) {
    builtKey = key;
    build();
  } else {
    fit();
  }
}

new ResizeObserver(([entry]) => {
  if (!entry) return;
  box = { w: entry.contentRect.width, h: entry.contentRect.height };
  fit();
}).observe(area);

// ---------------------------------------------------------------- search

const ERROR_MS = 3500;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let errorTimer;

/** @param {string} message */
function showError(message) {
  errorEl.textContent = message;
  errorEl.hidden = false;
  clearTimeout(errorTimer);
  errorTimer = setTimeout(clearError, ERROR_MS);
}

function clearError() {
  clearTimeout(errorTimer);
  errorEl.hidden = true;
}

// The iframe has no `allow-forms`, so there is no form to submit: Enter is the search.
input.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (input.value) input.value = "";
    else input.blur();
    clearError();
    return;
  }
  if (e.key !== "Enter") return;
  const target = resolveTarget({ query: input.value, template: templateFor(settings.engine, settings.customUrl), openUrls: true });
  if ("error" in target) return showError(target.error);
  mypaper.launch(target.url).then(
    () => {
      clearError();
      input.value = "";
      input.blur();
    },
    () => showError("Couldn't open the browser"),
  );
});
input.addEventListener("input", clearError);
// Edit mode (moving the widget) takes the keyboard away from the box.
mypaper.on("editmode", (editing) => {
  if (editing) input.blur();
});

trackSettings(defaults, (next) => {
  settings = { ...next, iconSize: clamp(next.iconSize, 24, 96) };
  shortcuts = readShortcuts(next.shortcuts);
  card.toggleAttribute("data-search", settings.search);
  engineEl.textContent = engineLabel(settings.engine, settings.customUrl);
  input.placeholder = "Search";
  render();
});
