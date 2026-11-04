// @ts-check
// Pure helpers for the system monitor: history, ring and sparkline geometry, grid choice.
// Tested in packages/widget-sdk/test/system-monitor.test.ts.

/** Samples kept per metric: 2 minutes at the default 2 s interval. */
export const HISTORY = 60;

/** @param {number} n @param {number} lo @param {number} hi */
export const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n));

/**
 * A whole percent, 0..100 (NaN counts as 0).
 * @param {number} n
 */
export const percent = (n) => (Number.isFinite(n) ? Math.round(clamp(n, 0, 100)) : 0);

/**
 * Used over total as a percentage (0 when the total is unknown).
 * @param {number} used
 * @param {number} total
 */
export const percentOf = (used, total) => (total > 0 ? clamp((used / total) * 100, 0, 100) : 0);

/**
 * Appends to a history in place, dropping the oldest beyond `max`.
 * @param {number[]} values
 * @param {number} value
 * @param {number} [max]
 */
export function pushSample(values, value, max = HISTORY) {
  values.push(value);
  if (values.length > max) values.splice(0, values.length - max);
}

/**
 * `stroke-dashoffset` that leaves `pct` percent of a ring of `circumference` drawn
 * (with `stroke-dasharray: circumference`).
 * @param {number} circumference
 * @param {number} pct
 */
export const ringOffset = (circumference, pct) => circumference * (1 - clamp(pct, 0, 100) / 100);

/**
 * Circumference of a circle of radius `r`.
 * @param {number} r
 */
export const circumference = (r) => 2 * Math.PI * r;

/**
 * The top of a series' axis: its peak, never below `floor`, so idle noise stays flat.
 * @param {readonly number[]} series
 * @param {number} floor
 */
export const axisMax = (series, floor) => Math.max(floor, ...series);

/**
 * A sparkline in a `width` x `height` box, newest sample on the right, as an SVG line path and a
 * closed area path under it. A short history sits at the right edge; `pad` keeps the stroke off the
 * top and bottom of the box.
 * @param {readonly number[]} values
 * @param {{ points?: number, width: number, height: number, max: number, pad?: number }} o
 * @returns {{ line: string, area: string, last: { x: number, y: number } | null }}
 */
export function sparkPaths(values, { points = HISTORY, width, height, max, pad = 2 }) {
  if (values.length === 0) return { line: "", area: "", last: null };
  const offset = points - values.length;
  const pts = values.map((v, i) => ({
    x: ((offset + i) / (points - 1)) * width,
    y: height - pad - clamp(v / (max || 1), 0, 1) * (height - 2 * pad),
  }));
  const line = pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(2)} ${p.y.toFixed(2)}`).join("");
  const first = pts[0];
  const last = pts[pts.length - 1] ?? null;
  const area = first && last ? `${line}L${last.x.toFixed(2)} ${height}L${first.x.toFixed(2)} ${height}Z` : "";
  return { line, area, last };
}

/**
 * Columns and rows for `count` equal cells in a box, favouring cells a bit wider than tall (`ratio`)
 * and never leaving a ragged last row.
 * @param {number} count
 * @param {number} width
 * @param {number} height
 * @param {number} [ratio] wanted cell width / height
 */
export function bestGrid(count, width, height, ratio = 1.1) {
  let best = { cols: count, rows: 1, score: Infinity };
  for (let cols = 1; cols <= count; cols++) {
    if (count % cols !== 0) continue;
    const rows = count / cols;
    const cell = width / cols / (height / rows);
    const score = Math.abs(Math.log(cell / ratio));
    if (score < best.score) best = { cols, rows, score };
  }
  return { cols: best.cols, rows: best.rows };
}

/** @typedef {"cpu" | "ram" | "gpu" | "net"} Metric */
/** Display order, and the setting that switches each on. */
export const METRICS = /** @type {const} */ (["cpu", "ram", "gpu", "net"]);

/**
 * The metrics the user switched on, in display order.
 * @param {Record<Metric, boolean>} on
 * @returns {Metric[]}
 */
export const enabledMetrics = (on) => METRICS.filter((m) => on[m]);

/**
 * How many tiles a box can show before they get cramped (`minCell` px per cell edge on average).
 * @param {number} width
 * @param {number} height
 * @param {number} wanted
 * @param {number} [minCell]
 */
export function fitCount(width, height, wanted, minCell = 70) {
  const fit = Math.max(1, Math.floor((width * height) / (minCell * minCell)));
  return clamp(wanted, 1, fit);
}

/**
 * The grid that shows the most of `count` cells of at least `minW` x `minH` px in a box, then the
 * biggest cells, closest to `ratio` (width / height). The last row may be short (the widget
 * centres it); no row is ever empty. When nothing fits, one cell.
 * @param {number} count
 * @param {number} width
 * @param {number} height
 * @param {{ minW: number, minH: number, ratio?: number, gap?: number }} o
 * @returns {{ cols: number, rows: number, shown: number }}
 */
export function fitGrid(count, width, height, { minW, minH, ratio = 1, gap = 8 }) {
  let best = { cols: 1, rows: 1, shown: Math.min(1, count), score: -Infinity };
  for (let cols = 1; cols <= count; cols++) {
    for (let rows = 1; (rows - 1) * cols < count; rows++) {
      const cw = (width - (cols - 1) * gap) / cols;
      const ch = (height - (rows - 1) * gap) / rows;
      if (cw < minW || ch < minH) continue;
      const shown = Math.min(count, cols * rows);
      const score = shown * 1000 + Math.min(cw / ratio, ch) - Math.abs(Math.log(cw / ch / ratio)) * 20 - (cols * rows - shown) * 30;
      if (score > best.score) best = { cols, rows, shown, score };
    }
  }
  return { cols: best.cols, rows: best.rows, shown: best.shown };
}
