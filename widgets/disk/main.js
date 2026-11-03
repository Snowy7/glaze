// @ts-check
import { $, formatBytes, onVisible, setText, trackSettings } from "./kit.js";
import { driveUsage, pickDrives } from "./drives.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */

const defaults = { drives: "", showFree: false };
let settings = defaults;
/** @type {Stats | undefined} */
let stats;

const card = $("main");
const list = $("#list");
const empty = $("#empty");

const COMPACT_BELOW = 120; // box height under which rows drop their "612 GB of 1.0 TB" line (2x1)
const TWO_COLUMNS_FROM = 330; // box width from which many drives use two columns
const FADE = 14; // px of fade at the edges of a scrolling list
const MIN_FADE = 8; // the room under the last row below which a row is given up for the fade
let fadePx = FADE; // the bottom fade of the current layout (the room left under the last whole row)
const SLACK = 2; // px of rounding we forgive before calling a list too tall

/**
 * One drive: a name, a percent, a bar and a line of text. Built once, then updated.
 * @param {string} name
 */
function createRow(name) {
  const el = document.createElement("div");
  el.className = "drive";
  el.innerHTML = `
    <div class="top"><span class="name"></span><span class="pct num"><b></b><small>%</small></span></div>
    <div class="track"><i></i></div>
    <div class="sub num ellipsis"></div>`;
  const part = (/** @type {string} */ selector) => {
    const found = el.querySelector(selector);
    if (!(found instanceof HTMLElement)) throw new Error(`missing ${selector}`);
    return found;
  };
  part(".name").textContent = name;
  return { el, pct: part(".pct b"), fill: part(".track > i"), sub: part(".sub") };
}

/** @type {Map<string, ReturnType<typeof createRow>>} */
const rows = new Map();

/** @param {import("./drives.js").Disk} disk @returns {string} */
const total = (disk) => {
  const t = formatBytes(disk.total);
  return `${t.n} ${t.unit}`;
};

function render() {
  if (!stats) return;
  const disks = stats.disks ?? [];
  const picked = pickDrives(disks, settings.drives);

  empty.hidden = picked.length > 0;
  setText(empty, disks.length === 0 ? "No drives" : "No matching drives");

  // Keep rows by name; only touch the DOM's order when the set of drives changed.
  const names = picked.map((d) => d.name);
  const changed = names.length !== rows.size || names.some((n) => !rows.has(n));
  if (changed) {
    for (const key of [...rows.keys()]) if (!names.includes(key)) rows.delete(key);
    for (const n of names) if (!rows.has(n)) rows.set(n, createRow(n));
    list.replaceChildren(...names.flatMap((n) => rows.get(n)?.el ?? []));
  }

  for (const disk of picked) {
    const row = rows.get(disk.name);
    if (!row) continue;
    const u = driveUsage(disk, settings.showFree);
    const bytes = formatBytes(u.bytes);
    setText(row.pct, String(Math.round(u.fraction * 100)));
    row.fill.style.width = `${(u.fraction * 100).toFixed(1)}%`;
    setText(row.sub, `${bytes.n} ${bytes.unit} ${settings.showFree ? "free of" : "of"} ${total(disk)}`);
  }
  layout(picked.length);
}

/**
 * Picks compact rows and the column count from the box, and cuts the list at a whole row when it
 * has to scroll (so no half row peeks out under the fade).
 * @param {number} count drives shown
 */
function layout(count) {
  // The card's own box, in the (scale-corrected) pixels the layout lives in.
  const box = { w: card.clientWidth, h: card.clientHeight };
  list.dataset.compact = String(box.h < COMPACT_BELOW);
  list.style.maxHeight = "";
  list.style.paddingBottom = "";
  list.classList.remove("scrolls");
  const first = list.firstElementChild;
  if (first instanceof HTMLElement) {
    const style = getComputedStyle(card);
    const room = card.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    const gap = parseFloat(getComputedStyle(list).rowGap) || 0;
    const rowH = first.offsetHeight;
    // Wide boxes only split in two columns when one column would not fit.
    const fitsOne = Math.floor((room + gap + SLACK) / (rowH + gap));
    list.style.setProperty("--cols", String(box.w >= TWO_COLUMNS_FROM && count > fitsOne ? 2 : 1));
    if (list.scrollHeight > room + SLACK) {
      // Scrolls: keep whole rows, and use the leftover (up to FADE) for the fade under the last one.
      /** @param {number} n */
      const spare = (n) => room - (n * (rowH + gap) - gap);
      let rows = Math.max(1, Math.floor((room + gap) / (rowH + gap)));
      if (spare(rows) < MIN_FADE && rows > 1) rows--;
      fadePx = Math.max(0, Math.min(FADE, spare(rows)));
      list.style.maxHeight = `${rows * (rowH + gap) - gap + fadePx}px`;
      list.style.paddingBottom = `${fadePx}px`;
      list.classList.add("scrolls");
    }
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
