// @ts-check
import { $, clamp, formatBytes, onVisible, setText, trackSettings } from "./kit.js";
import { driveUsage, pickDrives } from "./drives.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */

const VARIANTS = /** @type {const} */ (["bars", "rings"]);
const defaults = { drives: "", showFree: false, variant: "bars" };
let settings = defaults;
/** @type {Stats | undefined} */
let stats;

const card = $("main");
const list = $("#list");
const empty = $("#empty");
const emptyText = $("#empty-text");

const COMPACT_BELOW = 120; // box height under which drives drop their "612 GB of 1.0 TB" line (2x1)
const ROOMY_FROM = 220; // box height from which rings also print that line
const TWO_COLUMNS_FROM = 330; // box width from which many bars use two columns
const RING_CELL = { s: 54, m: 54, l: 88 }; // narrowest ring tile per size, in px
const FADE = 14; // px of fade at the edges of a scrolling list
const MIN_FADE = 8; // the room under the last row below which a row is given up for the fade
let fadePx = FADE; // the bottom fade of the current layout (the room left under the last whole row)
const SLACK = 2; // px of rounding we forgive before calling a list too tall

/** @param {unknown} value @returns {typeof VARIANTS[number]} */
const variantOf = (value) => VARIANTS.find((v) => v === value) ?? "bars";

/**
 * @param {ParentNode} root
 * @param {string} selector
 */
function part(root, selector) {
  const found = root.querySelector(selector);
  if (!(found instanceof HTMLElement || found instanceof SVGElement)) throw new Error(`missing ${selector}`);
  return found;
}

const DRIVE_ICON = '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><use href="#i-drive"></use></svg>';
const PCT = '<span class="pct num"><b></b><small>%</small></span>';

/**
 * One drive: built once for the current variant, then updated.
 * @param {string} name
 * @param {typeof VARIANTS[number]} variant
 */
function createRow(name, variant) {
  const el = document.createElement("div");
  if (variant === "rings") {
    el.className = "drive tile";
    el.innerHTML = `
      <div class="dial"><svg class="ring" aria-hidden="true"><circle class="back" pathLength="100"></circle><circle class="arc" pathLength="100"></circle></svg>${PCT}</div>
      <div class="label">${DRIVE_ICON}<span class="name"></span></div>
      <div class="sub num ellipsis"></div>`;
  } else {
    el.className = "drive";
    el.innerHTML = `
      <div class="top">${DRIVE_ICON}<span class="name"></span>${PCT}</div>
      <div class="track"><i></i></div>
      <div class="sub num ellipsis"></div>`;
  }
  part(el, ".name").textContent = name;
  const pct = part(el, ".pct b");
  const sub = part(el, ".sub");
  const arc = variant === "rings" ? part(el, ".arc") : undefined;
  const fill = variant === "bars" ? part(el, ".track > i") : undefined;
  return {
    el,
    pct,
    sub,
    /** @param {number} fraction 0..1 */
    paint(fraction) {
      const p = clamp(fraction, 0, 1) * 100;
      if (fill) fill.style.width = `${p.toFixed(1)}%`;
      if (arc) {
        arc.style.strokeDasharray = `${p.toFixed(1)} ${(100 - p).toFixed(1)}`;
        arc.style.visibility = p > 0 ? "visible" : "hidden"; // a 0 arc would draw a round-cap dot
      }
    },
  };
}

/** @type {Map<string, ReturnType<typeof createRow>>} */
const rows = new Map();
let rowsVariant = variantOf(defaults.variant);

/** @param {import("./drives.js").Disk} disk @returns {string} */
const total = (disk) => {
  const t = formatBytes(disk.total);
  return `${t.n} ${t.unit}`;
};

function render() {
  if (!stats) return;
  const disks = stats.disks ?? [];
  const picked = pickDrives(disks, settings.drives);
  const variant = variantOf(settings.variant);

  empty.hidden = picked.length > 0;
  setText(emptyText, disks.length === 0 ? "No drives" : "No matching drives");
  list.dataset.variant = variant;

  // Keep rows by name; only touch the DOM's order when the set of drives (or the variant) changed.
  const names = picked.map((d) => d.name);
  const changed = variant !== rowsVariant || names.length !== rows.size || names.some((n) => !rows.has(n));
  if (changed) {
    if (variant !== rowsVariant) rows.clear();
    rowsVariant = variant;
    for (const key of [...rows.keys()]) if (!names.includes(key)) rows.delete(key);
    for (const n of names) if (!rows.has(n)) rows.set(n, createRow(n, variant));
    list.replaceChildren(...names.flatMap((n) => rows.get(n)?.el ?? []));
  }

  for (const disk of picked) {
    const row = rows.get(disk.name);
    if (!row) continue;
    const u = driveUsage(disk, settings.showFree);
    const bytes = formatBytes(u.bytes);
    setText(row.pct, String(Math.round(u.fraction * 100)));
    row.paint(u.fraction);
    row.el.toggleAttribute("data-full", u.full);
    setText(row.sub, `${bytes.n} ${bytes.unit} ${settings.showFree ? "free of" : "of"} ${total(disk)}`);
  }
  layout(picked.length);
}

/**
 * Picks the size class and the column count from the box, and cuts the list at a whole row when it
 * has to scroll (so no half row peeks out under the fade).
 * @param {number} count drives shown
 */
function layout(count) {
  // The card's own box, in the (scale-corrected) pixels the layout lives in.
  const box = { w: card.clientWidth, h: card.clientHeight };
  const size = box.h < COMPACT_BELOW ? "s" : box.h < ROOMY_FROM ? "m" : "l";
  list.dataset.size = size;
  list.style.maxHeight = "";
  list.style.paddingBottom = "";
  list.classList.remove("scrolls");
  const first = list.firstElementChild;
  if (!(first instanceof HTMLElement)) return fades();

  const style = getComputedStyle(card);
  const room = box.h - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
  const gap = parseFloat(getComputedStyle(list).rowGap) || 0;
  if (variantOf(settings.variant) === "rings") {
    // As many tiles per row as the width allows; a few drives that don't fit one row split evenly
    // (2 + 2, not 3 + 1).
    const colGap = parseFloat(getComputedStyle(list).columnGap) || 0;
    const wide = box.w - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const fits = Math.max(1, Math.floor((wide + colGap) / (RING_CELL[size] + colGap)));
    const cols = count <= fits ? count : count <= 4 && fits >= 2 ? 2 : fits;
    list.style.setProperty("--cols", String(Math.max(1, cols)));
  }
  const rowH = first.offsetHeight;
  if (variantOf(settings.variant) === "bars") {
    // Wide boxes only split in two columns when one column would not fit.
    const fitsOne = Math.floor((room + gap + SLACK) / (rowH + gap));
    list.style.setProperty("--cols", String(box.w >= TWO_COLUMNS_FROM && count > fitsOne ? 2 : 1));
  }
  if (list.scrollHeight > room + SLACK) {
    // Scrolls: keep whole rows, and use the leftover (up to FADE) for the fade under the last one.
    /** @param {number} n */
    const spare = (n) => room - (n * (rowH + gap) - gap);
    let whole = Math.max(1, Math.floor((room + gap) / (rowH + gap)));
    if (spare(whole) < MIN_FADE && whole > 1) whole--;
    fadePx = Math.max(0, Math.min(FADE, spare(whole)));
    list.style.maxHeight = `${whole * (rowH + gap) - gap + fadePx}px`;
    list.style.paddingBottom = `${fadePx}px`;
    list.classList.add("scrolls");
  }
  fades();
}

/** Soft fades at the edges of the list that has more behind them. */
function fades() {
  const more = list.classList.contains("scrolls");
  list.style.setProperty("--fade-t", more && list.scrollTop > 1 ? `${FADE}px` : "0px");
  list.style.setProperty("--fade-b", more && list.scrollTop + list.clientHeight < list.scrollHeight - 1 ? `${fadePx}px` : "0px");
}
list.addEventListener("scroll", fades, { passive: true });

// A ResizeObserver rather than `trackSize`: it fires after the page has taken its new size, padding
// and scale, which is what `layout` measures.
new ResizeObserver(() => layout(rows.size)).observe(card);

trackSettings(defaults, (next) => {
  settings = next;
  render();
});

/** @type {(() => void) | undefined} */
let unsubscribe;
onVisible(
  () => {
    /** @param {Stats} next */
    const take = (next) => {
      stats = next;
      render();
    };
    void mypaper.system.stats().then(take, () => {});
    unsubscribe = mypaper.system.subscribe(5000, take);
  },
  () => {
    unsubscribe?.();
    unsubscribe = undefined;
  },
);
