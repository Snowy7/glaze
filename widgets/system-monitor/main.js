// @ts-check
import { $, clamp, onVisible, trackSettings, trackSize, useWallpaperAccent } from "./kit.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */

const POINTS = 60; // 2 minutes at the 2 s sample rate
const SVG = "http://www.w3.org/2000/svg";
const GB = 1024 ** 3;

/** @param {number} bytes per second */
function rate(bytes) {
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let n = bytes;
  let i = 0;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return { n: n < 10 && i > 0 ? n.toFixed(1) : String(Math.round(n)), unit: units[i] ?? "B/s" };
}

/**
 * Points to an SVG polyline in a 100x40 box, newest on the right.
 * @param {number[]} values
 * @param {number} max
 * @returns {string}
 */
function linePath(values, max) {
  const offset = POINTS - values.length;
  return values
    .map((v, i) => {
      const x = ((offset + i) / (POINTS - 1)) * 100;
      const y = 39 - clamp(v / max, 0, 1) * 37;
      return `${i === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join("");
}

/** @param {number[]} values @param {number} v */
function push(values, v) {
  values.push(v);
  if (values.length > POINTS) values.shift();
}

/** One tile: a name, a big value, an optional second line and a sparkline (or two). */
class Tile {
  /**
   * @param {string} label
   * @param {{ second?: boolean }} [options] second: draw a second, dashed series
   */
  constructor(label, options = {}) {
    this.el = document.createElement("section");
    this.el.className = "tile";
    this.el.innerHTML = `
      <div class="head"><span class="name caps"></span><span class="value num"></span><span class="sub num"></span></div>
      <svg class="spark" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
        <path class="area"></path><path class="line"></path>
      </svg>`;
    this.value = this.el.querySelector(".value");
    if (this.value) this.value.textContent = "-";
    this.sub = this.el.querySelector(".sub");
    const svg = this.el.querySelector("svg");
    this.area = this.el.querySelector(".area");
    this.line = this.el.querySelector(".line");
    /** @type {SVGPathElement | null} */
    this.line2 = null;
    if (options.second && svg) {
      this.line2 = document.createElementNS(SVG, "path");
      this.line2.setAttribute("class", "line second");
      svg.append(this.line2);
    }
    this.el.querySelector(".name")?.replaceChildren(label);
    /** @type {number[]} */
    this.a = [];
    /** @type {number[]} */
    this.b = [];
  }

  /**
   * @param {{ value: string, unit?: string, sub?: string, a: number, b?: number, max: (a: number[], b: number[]) => number }} s
   */
  update(s) {
    push(this.a, s.a);
    if (s.b !== undefined) push(this.b, s.b);
    if (this.el.hidden) return;
    const max = s.max(this.a, this.b);
    const d = linePath(this.a, max);
    this.line?.setAttribute("d", d);
    this.area?.setAttribute("d", `${d}L100 40L${((POINTS - this.a.length) / (POINTS - 1)) * 100} 40Z`);
    this.line2?.setAttribute("d", linePath(this.b, max));
    if (this.value) this.value.innerHTML = s.unit ? `${s.value}<small>${s.unit}</small>` : s.value;
    if (this.sub) this.sub.textContent = s.sub ?? "";
  }
}

const tiles = {
  cpu: new Tile("CPU"),
  ram: new Tile("Memory"),
  gpu: new Tile("GPU"),
  net: new Tile("Network", { second: true }),
};
const order = [tiles.cpu, tiles.ram, tiles.gpu, tiles.net];
const grid = $("#grid");
grid.append(...order.map((t) => t.el));

/** @param {Stats} s */
function render(s) {
  const pct = (/** @type {number} */ n) => String(Math.round(clamp(n, 0, 100)));
  tiles.cpu.update({ value: pct(s.cpu), unit: "%", a: s.cpu, max: () => 100 });

  const ramPct = s.memory.total > 0 ? (s.memory.used / s.memory.total) * 100 : 0;
  tiles.ram.update({
    value: pct(ramPct),
    unit: "%",
    sub: `${(s.memory.used / GB).toFixed(1)} / ${(s.memory.total / GB).toFixed(0)} GB`,
    a: ramPct,
    max: () => 100,
  });

  tiles.gpu.update(
    s.gpu === undefined
      ? { value: "-", sub: "not available", a: 0, max: () => 100 }
      : { value: pct(s.gpu), unit: "%", a: s.gpu, max: () => 100 },
  );

  const down = s.net?.down ?? 0;
  const up = s.net?.up ?? 0;
  const d = rate(down);
  const u = rate(up);
  tiles.net.update({
    value: d.n,
    unit: d.unit,
    sub: `up ${u.n} ${u.unit}`,
    a: down,
    b: up,
    // Auto-scale to the window's peak, with a floor so idle noise stays flat.
    max: (a, b) => Math.max(256 * 1024, ...a, ...b),
  });
}

// ---- layout: pick how many tiles fit and how to arrange them

/** @param {number} w @param {number} h */
function layout(w, h) {
  const n = clamp(Math.floor((w * h) / (90 * 80)), 2, 4);
  let best = { gc: 1, score: Infinity };
  for (let gc = 1; gc <= n; gc++) {
    if (n % gc !== 0) continue; // no ragged last row
    const gr = n / gc;
    const ratio = (w - 16) / gc / ((h - 16) / gr);
    // Prefer tiles a little wider than tall.
    const score = Math.abs(Math.log(ratio / 1.4));
    if (score < best.score) best = { gc, score };
  }
  const gr = n / best.gc;
  grid.style.setProperty("--gc", String(best.gc));
  grid.style.setProperty("--gr", String(gr));
  order.forEach((t, i) => (t.el.hidden = i >= n));
}

trackSize((size) => layout(size.px[0], size.px[1]));
trackSettings({ wallpaperAccent: false }, (s) => useWallpaperAccent(s.wallpaperAccent));

// ---- data: one subscription while visible

/** @type {(() => void) | undefined} */
let unsubscribe;
onVisible(
  () => {
    // Paint now instead of waiting a full interval.
    mypaper.system.stats().then(render, () => {});
    unsubscribe = mypaper.system.subscribe(2000, render);
  },
  () => {
    unsubscribe?.();
    unsubscribe = undefined;
  },
);
