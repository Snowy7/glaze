// @ts-check
import { $, clamp, formatRate, onVisible, setText, trackSettings } from "./kit.js";
import { describeConnection, signalBars } from "./connection.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */

const POINTS = 60; // 2 minutes at the 2 s sample rate
const FLOOR = 256 * 1024; // the graph never zooms in below this, so idle noise stays flat

const defaults = { showSparkline: true };

const card = $("#card");
const els = {
  name: $("#name"),
  sub: $("#sub"),
  downN: $("#down-n"),
  downU: $("#down-u"),
  upN: $("#up-n"),
  upU: $("#up-u"),
  area: $("#area", SVGPathElement),
  downLine: $("#down-line", SVGPathElement),
  upLine: $("#up-line", SVGPathElement),
};

/** @type {number[]} */
const down = [];
/** @type {number[]} */
const up = [];

/** @param {number[]} values @param {number} v */
function push(values, v) {
  values.push(v);
  if (values.length > POINTS) values.shift();
}

/**
 * Path in a 100x40 box, newest on the right.
 * @param {number[]} values
 * @param {number} max
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

/** @param {Stats} s */
function render(s) {
  const network = s.network;
  // Without `network` (older hosts) only the rates are shown.
  card.dataset.kind = network?.kind ?? "unknown";
  if (network) {
    const { name, sub } = describeConnection(network);
    setText(els.name, name);
    setText(els.sub, sub);
    card.dataset.bars = String(network.kind === "wifi" ? signalBars(network.signal) : 4);
  }

  const d = formatRate(s.net?.down ?? 0);
  const u = formatRate(s.net?.up ?? 0);
  setText(els.downN, d.n);
  setText(els.downU, d.unit);
  setText(els.upN, u.n);
  setText(els.upU, u.unit);

  push(down, s.net?.down ?? 0);
  push(up, s.net?.up ?? 0);
  if (card.dataset.spark === "off") return;
  const max = Math.max(FLOOR, ...down, ...up);
  const line = linePath(down, max);
  els.downLine.setAttribute("d", line);
  els.upLine.setAttribute("d", linePath(up, max));
  els.area.setAttribute("d", `${line}L100 40L${((POINTS - down.length) / (POINTS - 1)) * 100} 40Z`);
}

trackSettings(defaults, (settings) => {
  card.dataset.spark = settings.showSparkline ? "on" : "off";
});

/** @type {(() => void) | undefined} */
let unsubscribe;
onVisible(
  () => {
    void mypaper.system.stats().then(render, () => {});
    unsubscribe = mypaper.system.subscribe(2000, render);
  },
  () => {
    unsubscribe?.();
    unsubscribe = undefined;
  },
);
