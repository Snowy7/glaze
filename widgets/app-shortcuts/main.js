// @ts-check
// App shortcuts: launcher tiles with the apps' real icons (`mypaper.appIcon`), as a dock whose icons
// magnify toward the pointer (the maths is in apps.js), an icon grid, or a list. Layout and look
// live in style.css; this file builds the buttons, loads the icons and drives the dock.
import { $, clamp, trackSettings } from "./kit.js";
import { dockPose, fitIconSize, iconPixels, monogram, peakScale, readShortcuts } from "./apps.js";

/** @typedef {import("./apps.js").Shortcut} Shortcut */
/** @typedef {{ button: HTMLButtonElement, img: HTMLImageElement, mono: HTMLElement, shortcut: Shortcut }} Item */

const area = $("#area");

// The dock's spacing in layout px (style.css reads it from --dgap and --dpad).
const ROOMY = { gap: 8, pad: 8 };
const TIGHT = { gap: 4, pad: 5 };
/** Headroom kept free above a grown icon for the hover label. */
const TIP_RESERVE = 30;

const VARIANTS = ["dock", "grid", "list"];
const defaults = { variant: "dock", iconSize: 48, labels: true, shortcuts: /** @type {unknown} */ ([]) };
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

  if (items.length === 0) {
    const hint = document.createElement("div");
    hint.className = "empty chip";
    hint.textContent = "Add apps in this widget's settings";
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
  const size =
    settings.variant === "dock"
      ? fitDock()
      : fitIconSize(settings.variant === "list" ? "list" : "grid", items.length, box.w, box.h, settings.labels, settings.iconSize);
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

trackSettings(defaults, (next) => {
  settings = { ...next, iconSize: clamp(next.iconSize, 24, 96) };
  shortcuts = readShortcuts(next.shortcuts);
  render();
});
