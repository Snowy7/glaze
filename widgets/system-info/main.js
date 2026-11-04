// @ts-check
import { $, clamp, every, formatBytes, formatDuration, onVisible, setText, trackSettings } from "./kit.js";
import { availableRows, fitRows, ORDER, primaryDisk } from "./rows.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */
/** @typedef {import("./rows.js").RowId} RowId */

const GB = 1024 ** 3;

const defaults = { showGpu: true, showDisk: true, showNetwork: true, showBattery: true };
let settings = defaults;
/** @type {Stats | undefined} */
let stats;
/** The uptime of the last sample, and when it arrived: the row counts on from there between samples. */
let uptimeAt = { seconds: 0, at: performance.now() };
let box = { w: 200, h: 200 };
/** How many rows the box has room for (see layout). */
let capacity = 4;

// Row geometry, matching style.css (.row height and the rows' gap): used to count what fits.
const ROW_H = 24;
const GAP = 9;
const HEAD_H = 22 + 10; // header plus the space under it
const TWO_COLUMNS_FROM = 340;
const HEADER_FROM = 140; // box height from which the "This PC" header shows

/** @type {Record<RowId, { label: string; bar: boolean }>} */
const META = {
  uptime: { label: "Uptime", bar: false },
  cpu: { label: "CPU", bar: true },
  ram: { label: "RAM", bar: true },
  gpu: { label: "GPU", bar: false },
  disk: { label: "Disk", bar: false }, // renders as the drive's letter once there is a sample
  network: { label: "Network", bar: false },
  battery: { label: "Battery", bar: false },
};

const card = $("#card");
const head = $("#head");
const rowsEl = $("#rows");

/**
 * One label/value row; CPU and RAM carry a thin bar, the others a hairline, so every row is as tall.
 * @param {RowId} id
 */
function createRow(id) {
  const meta = META[id];
  const el = document.createElement("div");
  el.className = "row";
  el.toggleAttribute("data-gauge", meta.bar);
  el.innerHTML = `
    <span class="ico tile"><svg viewBox="0 0 24 24" aria-hidden="true"><use href="#i-${id}"></use></svg></span>
    <span class="key caps"></span>
    <span class="val num"><b></b><small></small></span>
    ${meta.bar ? '<div class="track bar"><i></i></div>' : '<div class="rule"></div>'}`;
  const part = (/** @type {string} */ selector) => {
    const found = el.querySelector(selector);
    if (!(found instanceof HTMLElement)) throw new Error(`missing ${selector}`);
    return found;
  };
  setText(part(".key"), meta.label);
  return { el, key: part(".key"), main: part("b"), extra: part("small"), fill: meta.bar ? part(".track > i") : undefined };
}

const rows = new Map(ORDER.map((id) => [id, createRow(id)]));
rowsEl.append(...[...rows.values()].map((r) => r.el));

/**
 * @param {RowId} id
 * @param {string} main
 * @param {string} [extra]
 * @param {number} [fraction] 0..1, for rows with a bar
 */
function setRow(id, main, extra = "", fraction) {
  const row = rows.get(id);
  if (!row) return;
  setText(row.main, main);
  setText(row.extra, extra);
  if (row.fill && fraction !== undefined) row.fill.style.width = `${(clamp(fraction, 0, 1) * 100).toFixed(1)}%`;
}

/** @param {number} bytes */
const size = (bytes) => {
  const { n, unit } = formatBytes(bytes);
  return `${n} ${unit}`;
};

function renderUptime() {
  setRow("uptime", formatDuration(uptimeAt.seconds + (performance.now() - uptimeAt.at) / 1000));
}

/** Shows the rows that have data and fit, with fresh values. */
function render() {
  if (!stats) return;
  const s = stats;
  const shown = new Set(fitRows(availableRows(s, settings), capacity));
  for (const [id, row] of rows) row.el.hidden = !shown.has(id);

  renderUptime();
  const cpu = Math.round(clamp(s.cpu, 0, 100));
  setRow("cpu", `${cpu}%`, "", cpu / 100);
  const ramFraction = s.memory.total > 0 ? s.memory.used / s.memory.total : 0;
  setRow("ram", `${Math.round(ramFraction * 100)}%`, `${(s.memory.used / GB).toFixed(1)} / ${(s.memory.total / GB).toFixed(0)} GB`, ramFraction);
  if (s.gpu !== undefined) setRow("gpu", `${Math.round(clamp(s.gpu, 0, 100))}%`);
  const disk = primaryDisk(s.disks ?? []);
  if (disk) {
    const label = rows.get("disk")?.key;
    if (label) setText(label, disk.name); // the glyph says "disk"; the letter leaves room for the figures
    setRow("disk", size(disk.used), `/ ${size(disk.total)}`);
  }
  if (s.network) {
    const { kind, name, signal } = s.network;
    setRow(
      "network",
      kind === "none" ? "Offline" : name || (kind === "wifi" ? "Wi-Fi" : "Ethernet"),
      kind === "wifi" && signal !== undefined ? `${Math.round(signal)}%` : "",
    );
  }
  if (s.battery) setRow("battery", `${Math.round(s.battery.percent)}%`, s.battery.charging ? "Charging" : "On battery");

  // Fill the columns top to bottom.
  const cols = box.w >= TWO_COLUMNS_FROM ? 2 : 1;
  rowsEl.style.setProperty("--cols", String(cols));
  rowsEl.style.setProperty("--n", String(Math.max(1, Math.ceil(shown.size / cols))));
}

/** How many rows the box holds: whole rows per column, below the header when there is one. */
function layout() {
  // The card's own box, in the (scale-corrected) pixels the layout lives in.
  box = { w: card.clientWidth, h: card.clientHeight };
  const showHead = box.h >= HEADER_FROM;
  head.hidden = !showHead;
  const style = getComputedStyle(card);
  const room = box.h - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom) - (showHead ? HEAD_H : 0);
  const perColumn = Math.max(1, Math.floor((room + GAP) / (ROW_H + GAP)));
  capacity = perColumn * (box.w >= TWO_COLUMNS_FROM ? 2 : 1);
  render();
}

// A ResizeObserver rather than `trackSize`: it fires after the page has taken its new size, padding
// and scale, which is what the measurements above need.
new ResizeObserver(layout).observe(card);

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
      uptimeAt = { seconds: next.uptime, at: performance.now() };
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

// The uptime moves a minute at a time; recheck it a few times per minute, only while visible.
every(15_000, renderUptime);
