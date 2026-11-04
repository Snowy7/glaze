// @ts-check
// The System widget: CPU, memory, GPU, network, drives and battery, as rings, bars or graphs.
// meters.js turns each stats sample into meters; this file draws them. The DOM is rebuilt only
// when what is shown changes (the meters, the look, how many fit); a sample only updates text
// and geometry. Stats come from one host subscription, paused while the widget is hidden.
import { $, formatDuration, onVisible, setText, trackSettings } from "./kit.js";
import { circumference, fitGrid, ringOffset, sparkPaths } from "./gauge.js";
import { ICONS } from "./icons.js";
import { buildMeters, createHistory } from "./meters.js";

/** @typedef {import("./meters.js").Meter} Meter */
/** @typedef {import("./meters.js").Stats} Stats */
/** @typedef {{ el: HTMLElement, paint(m: Meter): void }} View */

const LOOKS = /** @type {const} */ (["rings", "bars", "spark"]);
const DEFAULTS = { variant: "rings", cpu: true, ram: true, gpu: true, net: true, disk: false, battery: true, drives: "", interval: "2000", vary: true };
const RING_R = 42;
const RING_C = circumference(RING_R);

const root = $("#root");
const stage = $("#stage");
let config = DEFAULTS;
const history = createHistory();
/** @type {Stats | null} */
let last = null;
/** The meters of the last sample (built once per sample: building records the history). */
/** @type {Meter[]} */
let meters = [];
/** @type {Map<string, View>} */
let views = new Map();
let shape = "";

/**
 * An element from static markup (icons and structure only; text goes in through setText).
 * @param {string} className
 * @param {string} html
 */
function el(className, html = "") {
  const node = document.createElement("div");
  node.className = className;
  node.innerHTML = html;
  return node;
}

/** @param {ParentNode} parent @param {string} selector */
function part(parent, selector) {
  const found = parent.querySelector(selector);
  if (!(found instanceof Element)) throw new Error(`missing ${selector}`);
  return /** @type {HTMLElement} */ (found);
}

/** A safe id for SVG gradients ("disk-C:" is "disk-C-"). @param {string} id */
const svgId = (id) => `g-${id.replace(/[^a-zA-Z0-9-]/g, "-")}`;

const valueMarkup = '<b class="v num"><span class="n">-</span><small class="u"></small></b>';

/** @param {HTMLElement} node @param {Meter} m */
function paintCommon(node, m) {
  setText(part(node, ".n"), m.value);
  setText(part(node, ".u"), m.unit);
  // A ring has two: inside it (short cells) and under it.
  for (const label of node.querySelectorAll(".lbl-text")) setText(label, m.label);
  const detail = node.querySelector(".det");
  if (detail) setText(detail, m.detail);
  node.dataset.state = m.state;
  const icon = node.querySelector(".ico");
  const want = m.state === "charging" ? "bolt" : m.icon;
  if (icon instanceof HTMLElement && icon.dataset.icon !== want) {
    icon.dataset.icon = want;
    icon.innerHTML = ICONS[want] ?? "";
  }
}

// ---------------------------------------------------------------- rings

/** @param {Meter} m @returns {View} */
function ringView(m) {
  const id = svgId(m.id);
  const node = el(
    `m ring-m m-${m.kind}`,
    `<div class="ring">
       <svg viewBox="0 0 100 100" aria-hidden="true">
         <defs><linearGradient id="${id}" x1="0" y1="1" x2="1" y2="0"><stop offset="0" class="s1"/><stop offset="1" class="s2"/></linearGradient></defs>
         <circle class="trk" cx="50" cy="50" r="${RING_R}"/>
         <circle class="arc" cx="50" cy="50" r="${RING_R}" transform="rotate(-90 50 50)" stroke="url(#${id})"
                 stroke-dasharray="${RING_C}" stroke-dashoffset="${RING_C}"/>
       </svg>
       <div class="center"><span class="ico"></span>${valueMarkup}<span class="lbl-in lbl-text"></span></div>
     </div>
     <div class="cap"><span class="lbl lbl-text ellipsis"></span><span class="det ellipsis"></span></div>`,
  );
  const arc = part(node, ".arc");
  return {
    el: node,
    paint(meter) {
      paintCommon(node, meter);
      arc.style.strokeDashoffset = String(ringOffset(RING_C, meter.fraction * 100));
    },
  };
}

// ---------------------------------------------------------------- bars

/** @param {Meter} m @returns {View} */
function barView(m) {
  const graph = m.kind === "net";
  const node = el(
    `m bar-m m-${m.kind}`,
    `<span class="ico"></span>
     <div class="row-main">
       <div class="row-top"><span class="lbl lbl-text ellipsis"></span><span class="det ellipsis"></span>${valueMarkup}</div>
       ${graph ? '<svg class="mini" viewBox="0 0 100 12" preserveAspectRatio="none" aria-hidden="true"><path class="mini-line"/></svg>' : '<div class="track"><i></i></div>'}
     </div>`,
  );
  const fill = node.querySelector(".track > i");
  const line = node.querySelector(".mini-line");
  return {
    el: node,
    paint(meter) {
      paintCommon(node, meter);
      if (fill instanceof HTMLElement) fill.style.transform = `scaleX(${meter.fraction})`;
      if (line) line.setAttribute("d", sparkPaths(meter.history, { width: 100, height: 12, max: 1, pad: 1.5 }).line);
    },
  };
}

// ---------------------------------------------------------------- graphs

/** @param {Meter} m @returns {View} */
function sparkView(m) {
  const id = svgId(m.id);
  const plot = m.kind !== "disk" && m.kind !== "battery";
  const node = el(
    `m spark-m tile m-${m.kind}`,
    `<div class="g-head"><span class="lbl"><span class="ico"></span><span class="lbl-text ellipsis"></span></span><span class="det ellipsis"></span></div>
     ${valueMarkup}
     ${
       plot
         ? `<svg class="plot" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
              <defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="a1"/><stop offset="1" class="a2"/></linearGradient></defs>
              <path class="area" fill="url(#${id})"/><path class="line"/>
            </svg>`
         : '<div class="track"><i></i></div>'
     }`,
  );
  const area = node.querySelector(".area");
  const line = node.querySelector(".line");
  const fill = node.querySelector(".track > i");
  return {
    el: node,
    paint(meter) {
      paintCommon(node, meter);
      if (area && line) {
        const p = sparkPaths(meter.history, { width: 100, height: 40, max: 1 });
        area.setAttribute("d", p.area);
        line.setAttribute("d", p.line);
      }
      if (fill instanceof HTMLElement) fill.style.transform = `scaleX(${meter.fraction})`;
    },
  };
}

const BUILDERS = { rings: ringView, bars: barView, spark: sparkView };

// ---------------------------------------------------------------- layout

/**
 * How many meters fit, and in what grid.
 * @param {(typeof LOOKS)[number]} look
 * @param {number} count
 */
function arrange(look, count) {
  const w = stage.clientWidth;
  const h = stage.clientHeight;
  if (look === "bars") {
    const rows = Math.max(1, Math.floor(h / 40));
    const cols = count > rows && w >= 340 ? 2 : 1;
    const shown = Math.min(count, rows * cols);
    return { shown, cols, rows: Math.ceil(shown / cols), header: cols === 1 && h - shown * 40 >= 34 && shown >= 3 };
  }
  const grid =
    look === "rings"
      ? fitGrid(count, w, h, { minW: 60, minH: 60, ratio: 0.85 })
      : fitGrid(count, w, h, { minW: 104, minH: 62, ratio: 1.5 });
  return { ...grid, header: false };
}

/** @param {string} title @param {string} hint @param {"" | "loading"} [kind] */
function state(title, hint, kind = "") {
  const node = el("state", `<div class="state-icon">${ICONS.pc}</div><div class="state-title"></div><div class="state-hint"></div>`);
  node.dataset.kind = kind;
  setText(part(node, ".state-title"), title);
  setText(part(node, ".state-hint"), hint);
  return node;
}

function render() {
  const look = LOOKS.find((l) => l === config.variant) ?? "rings";
  if (!last) {
    if (shape !== "loading") stage.replaceChildren(state("Reading your PC", "", "loading"));
    shape = "loading";
    return;
  }
  if (meters.length === 0) {
    const onlyBattery = config.battery && !config.cpu && !config.ram && !config.gpu && !config.net && !config.disk;
    const [title, hint] = onlyBattery ? ["No battery", "This PC runs on mains power"] : ["Nothing to show", "Turn on a metric in Settings"];
    if (shape !== `empty|${title}`) stage.replaceChildren(state(title, hint));
    shape = `empty|${title}`;
    return;
  }
  const fit = arrange(look, meters.length);
  const shown = meters.slice(0, fit.shown);
  const nextShape = `${look}|${fit.cols}x${fit.rows}|${fit.header}|${shown.map((m) => m.id).join(",")}`;
  if (nextShape !== shape) {
    shape = nextShape;
    views = new Map(shown.map((m) => [m.id, BUILDERS[look](m)]));
    stage.dataset.look = look;
    stage.style.setProperty("--cols", String(fit.cols));
    stage.style.setProperty("--rows", String(fit.rows));
    stage.dataset.count = String(shown.length);
    const header = fit.header ? el("sys-head", `<span class="label">${ICONS.pc}<span>This PC</span></span><span class="uptime num"></span>`) : null;
    stage.replaceChildren(...(header ? [header] : []), ...[...views.values()].map((v) => v.el));
  }
  const uptime = stage.querySelector(".uptime");
  if (uptime) setText(uptime, `Up ${formatDuration(last.uptime)}`);
  for (const m of shown) views.get(m.id)?.paint(m);
}

new ResizeObserver(() => {
  shape = shape === "loading" ? shape : "";
  render();
}).observe(stage);

// ---------------------------------------------------------------- data

/** @type {(() => void) | undefined} */
let unsubscribe;
const subscribe = () => {
  unsubscribe?.();
  const ms = Math.max(1000, Number(config.interval) || 2000);
  unsubscribe = mypaper.system.subscribe(ms, (s) => {
    last = s;
    meters = buildMeters(s, config, history, { drives: config.drives });
    render();
  });
};

trackSettings(DEFAULTS, (next) => {
  const intervalChanged = next.interval !== config.interval;
  config = next;
  if (last) meters = buildMeters(last, config, history, { drives: config.drives });
  root.toggleAttribute("data-vary", config.vary);
  shape = "";
  render();
  if (intervalChanged && unsubscribe) subscribe();
});

// The host pauses subscriptions while hidden; dropping ours too keeps nothing alive.
onVisible(subscribe, () => {
  unsubscribe?.();
  unsubscribe = undefined;
});
render();
