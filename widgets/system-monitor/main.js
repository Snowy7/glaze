// @ts-check
// System monitor with three looks: glowing rings, labelled bars, sparklines. The look and the
// metrics shown come from settings; the stage is rebuilt when they change and updated in place on
// every sample (one subscription, only while visible). Math lives in gauge.js.
import { $, formatRate, onVisible, setText, trackSettings } from "./kit.js";
import {
  axisMax,
  bestGrid,
  circumference,
  clamp,
  enabledMetrics,
  fitCount,
  percent,
  percentOf,
  pushSample,
  ringOffset,
  sparkPaths,
} from "./gauge.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */
/** @typedef {import("./gauge.js").Metric} Metric */
/** @typedef {{ pct: number, value: string, sub: string }} Gauge */

const VARIANTS = /** @type {const} */ (["rings", "bars", "spark"]);
const LABEL = { cpu: "CPU", ram: "Memory", gpu: "GPU", net: "Network" };
const GB = 1024 ** 3;
const RING_R = 42;
const RING_C = circumference(RING_R);
const NET_FLOOR = 256 * 1024; // bytes/s: idle noise stays flat on the network axis

const root = $("#root");
const stage = $("#stage");

const DEFAULTS = {
  variant: /** @type {(typeof VARIANTS)[number]} */ ("rings"),
  cpu: true,
  ram: true,
  gpu: true,
  net: true,
  interval: "2000",
  vary: true,
};
let config = { ...DEFAULTS };

/** @type {{ cpu: number[], ram: number[], gpu: number[], down: number[], up: number[] }} */
const hist = { cpu: [], ram: [], gpu: [], down: [], up: [] };

/**
 * What one stats sample means for the screen.
 * @typedef {{ cpu: Gauge, ram: Gauge, gpu: Gauge | null, down: number, up: number }} Sample
 */
/** @type {Sample | null} */
let sample = null;

/** @param {Stats} s @returns {Sample} */
function toSample(s) {
  const ram = percentOf(s.memory.used, s.memory.total);
  return {
    cpu: { pct: s.cpu, value: String(percent(s.cpu)), sub: "" },
    ram: { pct: ram, value: String(percent(ram)), sub: `${(s.memory.used / GB).toFixed(1)} / ${(s.memory.total / GB).toFixed(0)} GB` },
    gpu: s.gpu === undefined ? null : { pct: s.gpu, value: String(percent(s.gpu)), sub: "" },
    down: s.net?.down ?? 0,
    up: s.net?.up ?? 0,
  };
}

/** @param {Sample} s @param {"cpu" | "ram" | "gpu"} m */
const gaugeOf = (s, m) => s[m];

// ---- building blocks

/**
 * @param {string} className
 * @param {string} html static markup only
 */
function el(className, html) {
  const node = document.createElement("section");
  node.className = className;
  node.innerHTML = html;
  return node;
}

/** @param {ParentNode} parent @param {string} selector */
const pick = (parent, selector) => {
  const found = parent.querySelector(selector);
  if (!found) throw new Error(`missing ${selector}`);
  return found;
};

/** A highlight-pair gradient for strokes, inline so it sees the metric's --m1/--m2. */
const gradient = (/** @type {string} */ id, /** @type {string} */ coords) =>
  `<linearGradient id="${id}" gradientUnits="userSpaceOnUse" ${coords}><stop offset="0" style="stop-color:var(--m1)"/><stop offset="1" style="stop-color:var(--m2)"/></linearGradient>`;

/** `<n><small>unit</small>` where only n is rewritten per sample. */
const valueHtml = (/** @type {string} */ unit) => `<span class="n">-</span><small>${unit}</small>`;

/** @param {Metric} m */
const missing = (m) => (m === "gpu" ? "not available" : "");

// ---- the three looks: each builds cells and returns the function that paints a sample into them

/** @param {"cpu" | "ram" | "gpu"} m */
function ringCell(m) {
  const cell = el(
    `cell m-${m}`,
    `<div class="ring">
       <svg viewBox="0 0 100 100" aria-hidden="true">
         <defs>${gradient(`rg-${m}`, 'x1="12" y1="88" x2="88" y2="12"')}</defs>
         <circle class="trk" cx="50" cy="50" r="${RING_R}" />
         <circle class="arc glow" cx="50" cy="50" r="${RING_R}" transform="rotate(-90 50 50)" stroke="url(#rg-${m})"
                 stroke-dasharray="${RING_C}" stroke-dashoffset="${RING_C}" />
       </svg>
       <div class="val"><b class="hero num">${valueHtml("%")}</b><span class="nm caps">${LABEL[m]}</span></div>
     </div>
     <div class="sub num"></div>`,
  );
  const n = pick(cell, ".n");
  const arc = /** @type {SVGElement} */ (pick(cell, ".arc"));
  const sub = pick(cell, ".sub");
  return {
    cell,
    paint(/** @type {Sample} */ s) {
      const g = gaugeOf(s, m);
      setText(n, g ? g.value : "-");
      arc.style.strokeDashoffset = String(ringOffset(RING_C, g?.pct ?? 0));
      setText(sub, g ? g.sub : missing(m));
    },
  };
}

/** @param {"cpu" | "ram" | "gpu"} m */
function barRow(m) {
  const cell = el(
    `cell row m-${m}`,
    `<div class="top"><span class="caps">${LABEL[m]}</span><span class="sub num"></span><b class="hero num">${valueHtml("%")}</b></div>
     <div class="track"><i></i></div>`,
  );
  const n = pick(cell, ".n");
  const fill = /** @type {HTMLElement} */ (pick(cell, ".track > i"));
  const sub = pick(cell, ".sub");
  return {
    cell,
    paint(/** @type {Sample} */ s) {
      const g = gaugeOf(s, m);
      setText(n, g ? g.value : "-");
      fill.style.transform = `scaleX(${clamp((g?.pct ?? 0) / 100, 0, 1)})`;
      setText(sub, g ? g.sub : missing(m));
    },
  };
}

/** @param {Metric} m */
function sparkTile(m) {
  const net = m === "net";
  const cell = el(
    `cell tile m-${m}`,
    `<div class="head"><span class="caps">${LABEL[m]}</span>
       <b class="hero num">${net ? '<span class="n">-</span><small class="u"></small>' : valueHtml("%")}</b>
       <span class="sub num"></span></div>
     <div class="spark">
       <svg viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
         <defs>
           <linearGradient id="sa-${m}" x1="0" y1="0" x2="0" y2="1">
             <stop offset="0" style="stop-color:var(--m1);stop-opacity:0.42"/><stop offset="1" style="stop-color:var(--m1);stop-opacity:0"/>
           </linearGradient>
           ${gradient(`sl-${m}`, 'x1="0" y1="0" x2="100" y2="0"')}
         </defs>
         <path class="area" fill="url(#sa-${m})" /><path class="line" stroke="url(#sl-${m})" />
         ${net ? '<path class="line second" />' : ""}
       </svg>
       <i class="dot"></i>
     </div>`,
  );
  const n = pick(cell, ".n");
  const unit = cell.querySelector(".u");
  const sub = pick(cell, ".sub");
  const area = pick(cell, ".area");
  const line = pick(cell, ".line");
  const line2 = cell.querySelector(".line.second");
  const dot = /** @type {HTMLElement} */ (pick(cell, ".dot"));
  return {
    cell,
    paint(/** @type {Sample} */ s) {
      /** @type {readonly number[]} */
      const a = net ? hist.down : hist[/** @type {"cpu" | "ram" | "gpu"} */ (m)];
      const max = net ? axisMax([...hist.down, ...hist.up], NET_FLOOR) : 100;
      const p = sparkPaths(a, { width: 100, height: 40, max });
      line.setAttribute("d", p.line);
      area.setAttribute("d", p.area);
      if (p.last) dot.style.top = `${(p.last.y / 40) * 100}%`;
      if (net) {
        const d = formatRate(s.down);
        setText(n, d.n);
        if (unit) setText(unit, d.unit);
        const u = formatRate(s.up);
        setText(sub, `up ${u.n} ${u.unit}`);
        line2?.setAttribute("d", sparkPaths(hist.up, { width: 100, height: 40, max }).line);
        return;
      }
      const g = gaugeOf(s, /** @type {"cpu" | "ram" | "gpu"} */ (m));
      setText(n, g ? g.value : "-");
      setText(sub, g ? g.sub : missing(m));
    },
  };
}

/** The network readout under rings and bars. */
function netBar() {
  const node = document.createElement("div");
  node.className = "netbar chip num";
  node.innerHTML = `<span>&darr; <b class="dn"></b></span><span>&uarr; <b class="upr"></b></span>`;
  const dn = pick(node, ".dn");
  const up = pick(node, ".upr");
  return {
    node,
    paint(/** @type {Sample} */ s) {
      const d = formatRate(s.down);
      const u = formatRate(s.up);
      setText(dn, `${d.n} ${d.unit}`);
      setText(up, `${u.n} ${u.unit}`);
    },
  };
}

// ---- stage

/** @type {HTMLElement[]} */
let cells = [];
/** @type {((s: Sample) => void)[]} */
let painters = [];
/** @type {HTMLElement | undefined} */
let grid;

function build() {
  stage.replaceChildren();
  painters = [];
  let metrics = enabledMetrics({ cpu: config.cpu, ram: config.ram, gpu: config.gpu, net: config.net });
  if (metrics.length === 0) metrics = ["cpu"];
  const gauges = /** @type {("cpu" | "ram" | "gpu")[]} */ (metrics.filter((m) => m !== "net"));
  const withNet = metrics.includes("net");
  const parts =
    config.variant === "spark" ? metrics.map(sparkTile) : config.variant === "bars" ? gauges.map(barRow) : gauges.map(ringCell);

  grid = document.createElement("div");
  grid.className = "grid";
  grid.append(...parts.map((p) => p.cell));
  grid.toggleAttribute("data-sub", gauges.includes("ram") || gauges.includes("gpu"));
  stage.append(grid);
  painters = parts.map((p) => p.paint);
  if (withNet && config.variant !== "spark") {
    const bar = netBar();
    stage.append(bar.node);
    painters.push(bar.paint);
  }
  cells = parts.map((p) => p.cell);
  layout();
  if (sample) for (const paint of painters) paint(sample);
}

/** How many cells fit and how they are arranged, from the grid's real box. */
function layout() {
  if (!grid) return;
  const w = grid.offsetWidth;
  const h = grid.offsetHeight;
  if (w < 1 || h < 1) return;
  let cols = 1;
  let rows = 1;
  let n = cells.length;
  if (config.variant === "bars") {
    n = clamp(Math.floor(h / 40), 1, cells.length);
    rows = n;
  } else {
    n = fitCount(w, h, cells.length, config.variant === "rings" ? 76 : 84);
    ({ cols, rows } = bestGrid(n, w, h, config.variant === "rings" ? 1 : 1.35));
  }
  grid.style.setProperty("--gc", String(cols));
  grid.style.setProperty("--gr", String(rows));
  cells.forEach((c, i) => (c.hidden = i >= n));
}
new ResizeObserver(layout).observe(stage);

// ---- data

/** @param {Stats} s */
function onStats(s) {
  sample = toSample(s);
  pushSample(hist.cpu, s.cpu);
  pushSample(hist.ram, sample.ram.pct);
  pushSample(hist.gpu, s.gpu ?? 0);
  pushSample(hist.down, sample.down);
  pushSample(hist.up, sample.up);
  for (const paint of painters) paint(sample);
}

/** @type {(() => void) | undefined} */
let unsubscribe;
let visible = false;
function subscribe() {
  unsubscribe?.();
  // Paint now instead of waiting a full interval.
  mypaper.system.stats().then(onStats, () => {});
  unsubscribe = mypaper.system.subscribe(Number(config.interval), onStats);
}

/** @template {readonly string[]} L @param {string} v @param {L} list @param {L[number]} fallback @returns {L[number]} */
const oneOf = (v, list, fallback) => list.find((x) => x === v) ?? fallback;

trackSettings(DEFAULTS, (s) => {
  const prev = config;
  config = { ...s, variant: oneOf(s.variant, VARIANTS, "rings"), interval: oneOf(s.interval, ["1000", "2000", "5000"], "2000") };
  root.dataset["vary"] = config.vary ? "on" : "off";
  document.documentElement.dataset["variant"] = config.variant;
  if (
    config.variant !== prev.variant ||
    config.cpu !== prev.cpu ||
    config.ram !== prev.ram ||
    config.gpu !== prev.gpu ||
    config.net !== prev.net ||
    cells.length === 0
  ) {
    build();
  }
  if (visible && config.interval !== prev.interval) subscribe();
});

onVisible(
  () => {
    visible = true;
    subscribe();
  },
  () => {
    visible = false;
    unsubscribe?.();
    unsubscribe = undefined;
  },
);

