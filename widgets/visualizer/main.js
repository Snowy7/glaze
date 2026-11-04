// @ts-check
// Audio visualizer: one canvas, three looks (bars, wave, ring) fed by mypaper.audio.spectrum.
//
// Cost model: the host sends ~30 frames/s only while sound plays and the widget is visible. We draw
// on our own rAF loop (capped at 30 fps) only while something moves: when the spectrum is silent the
// bars ease down to a calm idle profile, one last frame is drawn, and the loop stops until sound
// returns. Hidden: no subscription and no loop. Geometry and smoothing live in viz.js.
import { $, clamp, fitCanvas, onVisible, trackSettings } from "./kit.js";
import {
  barLayout,
  barSpan,
  bassEnergy,
  clamp01,
  createPeaks,
  curveSegments,
  hasSettled,
  idleLevels,
  isSilent,
  paletteStops,
  pingPong,
  radialBars,
  sampleBars,
  shape,
  smoothInto,
  stepPeaks,
  wavePoints,
} from "./viz.js";

const VARIANTS = /** @type {const} */ (["bars", "wave", "circle"]);
const COLOR_MODES = /** @type {const} */ (["accent", "gradient", "rainbow"]);
const FRAME_MS = 1000 / 30;

const DEFAULTS = {
  variant: /** @type {(typeof VARIANTS)[number]} */ ("bars"),
  colorMode: /** @type {(typeof COLOR_MODES)[number]} */ ("gradient"),
  mirror: true,
  peaks: true,
  barWidth: 6,
  gap: 3,
  bins: 64,
  smoothing: 0.5,
  sensitivity: 1.2,
};
let config = { ...DEFAULTS };

const root = document.documentElement;
const canvas = $("#cv", HTMLCanvasElement);
const ctx = canvas.getContext("2d");
if (!ctx) throw new Error("no 2d context");

// A tiny canvas the glow is blurred through (neon only): the picture is shrunk onto it and
// stretched back underneath, which costs two drawImage calls instead of a shadowBlur per shape.
const glowCanvas = document.createElement("canvas");
const glowCtx = glowCanvas.getContext("2d");
const GLOW_SCALE = 5;

/** @type {number} */ let width = 0;
/** @type {number} */ let height = 0;
let visible = false;

// What the spectrum says (targets), what is on screen (levels) and the peak-hold caps.
/** @type {number[]} */ let latest = [];
let silent = true;
/** @type {number[]} */ let targets = [];
/** @type {number[]} */ let levels = [];
let peaks = createPeaks(0);
let bass = 0;
// Nothing has played yet: the caption shows until the first sound.
root.toggleAttribute("data-idle", true);

// Resolved CSS colors (re-read whenever the appearance changes) and the gradients built from them.
const look = { hi: "#fff", hi2: "#fff", ink: "#fff", dark: false, style: "minimal" };
/** @type {CanvasGradient | string} */ let fill = "#fff";
/** @type {CanvasGradient | string} */ let fade = "#000";

// ---- colors

/** One probe element resolves CSS colors (var(), color-mix()) into something canvas can parse. */
const probe = document.createElement("i");
probe.style.display = "none";
document.body.append(probe);

/**
 * @param {string} expression a CSS color expression, usually a var()
 * @param {string} fallback
 */
function resolveColor(expression, fallback) {
  probe.style.color = "";
  probe.style.color = expression;
  return getComputedStyle(probe).color || fallback;
}

function readLook() {
  look.dark = root.dataset["text"] === "dark";
  // On a light wallpaper (dark text) the style's bright highlights wash out: pull them toward the ink.
  const toward = (/** @type {string} */ v) =>
    look.dark ? `color-mix(in srgb, var(${v}) 60%, rgb(var(--ink)))` : `var(${v})`;
  look.hi = resolveColor(toward("--hi"), "#fff");
  look.hi2 = resolveColor(toward("--hi-2"), look.hi);
  look.ink = resolveColor("var(--t0)", "#fff");
  look.style = root.dataset["style"] ?? "minimal";
  rebuild();
  redraw();
}

// ---- geometry for the current size and settings

/** The ring's center and radii inside the canvas. */
function ringBox() {
  const outer = Math.max(8, Math.min(width, height) / 2 - 3);
  const inner = outer * 0.46;
  return { cx: width / 2, cy: height / 2, inner, length: outer - inner };
}

/** How many values the current look shows. */
function displayCount() {
  const { variant, mirror, barWidth, gap, bins } = config;
  if (variant === "bars") return barLayout(width, barWidth, gap).count;
  if (variant === "wave") return clamp(Math.round(width / 9), 12, bins);
  const { inner } = ringBox();
  const spokes = clamp(Math.round((2 * Math.PI * inner) / 5.5), 24, bins * 2);
  return mirror ? Math.ceil(spokes / 2) : spokes;
}

/** Gradients depend on size, look and colors; targets and state arrays on the count. */
function rebuild() {
  if (!ctx || width < 1) return;
  const stops = paletteStops(config.colorMode, look.hi, look.hi2, look.dark ? 40 : 64);
  /** @type {CanvasGradient} */
  let g;
  if (config.variant === "circle") {
    const { cx, cy } = ringBox();
    g = ctx.createConicGradient(Math.PI / 2, cx, cy);
    for (const s of pingPong(stops)) g.addColorStop(s.at, s.color);
  } else {
    g = ctx.createLinearGradient(0, 0, width, 0);
    for (const s of stops) g.addColorStop(s.at, s.color);
  }
  fill = g;
  // The wave's body fades from the line into transparency; destination-in keeps only that alpha.
  const f = ctx.createLinearGradient(0, 0, 0, height);
  if (config.variant === "wave" && config.mirror) {
    f.addColorStop(0, "rgb(0 0 0 / 0.22)");
    f.addColorStop(0.5, "rgb(0 0 0 / 0.85)");
    f.addColorStop(1, "rgb(0 0 0 / 0.22)");
  } else {
    f.addColorStop(0, "rgb(0 0 0 / 0.12)");
    f.addColorStop(1, "rgb(0 0 0 / 0.8)");
  }
  fade = f;

  const count = displayCount();
  if (levels.length !== count) {
    levels = idleLevels(count);
    peaks = createPeaks(count);
    peaks.level = [...levels];
  }
  computeTargets();
}

/** Called for every host frame and whenever the count or sensitivity changes. */
function computeTargets() {
  const count = levels.length;
  const source = silent ? idleLevels(count) : sampleBars(latest, count);
  // The ring squeezes the range (gamma): a long bass spoke against short highs reads as a lopsided medal.
  const gamma = config.variant === "circle" ? 0.7 : 1;
  targets = silent
    ? source
    : source.map((v, i) => shape(v, count <= 1 ? 0 : i / (count - 1), config.sensitivity) ** gamma);
}

// ---- drawing

function redraw() {
  if (!ctx || width < 1) return;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  const dpr = canvas.width / width;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
  if (config.variant === "bars") drawBars(ctx);
  else if (config.variant === "wave") drawWave(ctx);
  else drawRing(ctx);
  if (look.style === "neon") drawGlow(ctx);
}

/** @param {CanvasRenderingContext2D} c */
function drawBars(c) {
  const { count, pitch, bar } = barLayout(width, config.barWidth, config.gap);
  const capH = Math.max(2, bar * 0.5);
  const caps = config.peaks && !silent;
  // Leave room above (and below, mirrored) for the caps.
  const room = caps ? capH + 3 : 0;
  const boxH = height - room * (config.mirror ? 2 : 1);
  const minH = Math.min(bar, boxH);
  c.beginPath();
  for (let i = 0; i < count; i++) {
    const span = barSpan(levels[i] ?? 0, boxH, config.mirror, minH);
    c.roundRect(i * pitch, span.y + room, bar, span.h, bar / 2);
  }
  c.fillStyle = fill;
  c.fill();
  if (look.style !== "minimal" && look.style !== "paper") {
    // A soft light from above, only over the bars.
    c.globalCompositeOperation = "source-atop";
    const sheen = c.createLinearGradient(0, 0, 0, height);
    sheen.addColorStop(0, "rgb(255 255 255 / 0.32)");
    sheen.addColorStop(0.55, "rgb(255 255 255 / 0)");
    c.fillStyle = sheen;
    c.fillRect(0, 0, width, height);
    c.globalCompositeOperation = "source-over";
  }
  if (caps) {
    c.beginPath();
    for (let i = 0; i < count; i++) {
      const level = peaks.level[i] ?? 0;
      const span = barSpan(level, boxH, config.mirror, minH);
      const bar0 = barSpan(levels[i] ?? 0, boxH, config.mirror, minH);
      if (span.h - bar0.h < 2) continue; // the cap is on the bar: nothing to show
      c.roundRect(i * pitch, span.y + room - capH - 2, bar, capH, capH / 2);
      if (config.mirror) c.roundRect(i * pitch, span.y + room + span.h + 2, bar, capH, capH / 2);
    }
    c.fillStyle = look.ink;
    c.globalAlpha = 0.85;
    c.fill();
    c.globalAlpha = 1;
  }
}

/**
 * @param {Path2D} path
 * @param {{ x: number, y: number }[]} points
 * @param {boolean} [connect] continue the current subpath instead of starting a new one
 */
function trace(path, points, connect = false) {
  const first = points[0];
  if (!first) return;
  if (connect) path.lineTo(first.x, first.y);
  else path.moveTo(first.x, first.y);
  for (const s of curveSegments(points)) path.bezierCurveTo(s.c1x, s.c1y, s.c2x, s.c2y, s.x, s.y);
}

/** @param {CanvasRenderingContext2D} c */
function drawWave(c) {
  const base = config.mirror ? height / 2 : height - 2;
  const amp = (config.mirror ? height / 2 : height - 4) - 2;
  const top = wavePoints(levels, width, base, amp);
  const bottom = config.mirror ? top.map((p) => ({ x: p.x, y: 2 * base - p.y })).reverse() : [];

  const edge = new Path2D();
  const body = new Path2D();
  trace(edge, top);
  trace(body, top);
  if (config.mirror) {
    trace(body, bottom, true);
    trace(edge, bottom);
    body.closePath();
  } else {
    body.lineTo(width, base);
    body.lineTo(0, base);
    body.closePath();
  }

  c.fillStyle = fill;
  c.globalAlpha = 0.9;
  c.fill(body);
  c.globalAlpha = 1;
  c.globalCompositeOperation = "destination-in";
  c.fillStyle = fade;
  c.fillRect(0, 0, width, height);
  c.globalCompositeOperation = "source-over";

  c.lineJoin = "round";
  c.lineCap = "round";
  c.strokeStyle = fill;
  c.lineWidth = 2;
  c.stroke(edge);

  if (config.peaks && !silent) {
    // The held peaks: a faint outline over the body.
    const ghost = new Path2D();
    const g = wavePoints(peaks.level, width, base, amp);
    trace(ghost, g);
    if (config.mirror) trace(ghost, g.map((p) => ({ x: p.x, y: 2 * base - p.y })));
    c.globalAlpha = 0.35;
    c.lineWidth = 1.2;
    c.stroke(ghost);
    c.globalAlpha = 1;
  }
}

/** @param {CanvasRenderingContext2D} c */
function drawRing(c) {
  const { cx, cy, inner, length } = ringBox();
  const spokes = radialBars(levels, { cx, cy, inner: inner + 3, length: length - 3, minLength: 2, mirror: config.mirror });
  const thickness = clamp(((2 * Math.PI * inner) / spokes.length) * 0.5, 1.6, 7);

  // The pulsing core follows the kick.
  const kick = bassEnergy(levels);
  bass += (kick - bass) * 0.5;
  const coreR = inner * (0.3 + 0.4 * (silent ? 0 : bass));
  c.fillStyle = look.hi2;
  c.globalAlpha = 0.1 + 0.2 * bass;
  c.beginPath();
  c.arc(cx, cy, inner * (0.62 + 0.3 * bass), 0, Math.PI * 2);
  c.fill();
  c.globalAlpha = 0.95;
  const core = c.createRadialGradient(cx - coreR * 0.3, cy - coreR * 0.35, 0, cx, cy, coreR);
  core.addColorStop(0, look.hi);
  core.addColorStop(1, look.hi2);
  c.fillStyle = core;
  c.beginPath();
  c.arc(cx, cy, coreR, 0, Math.PI * 2);
  c.fill();
  c.globalAlpha = 1;

  c.beginPath();
  for (const s of spokes) {
    c.moveTo(s.x0, s.y0);
    c.lineTo(s.x1, s.y1);
  }
  c.strokeStyle = fill;
  c.lineWidth = thickness;
  c.lineCap = "round";
  c.stroke();
}

/** Neon glow: a shrunken copy of the frame, stretched back underneath it. */
function drawGlow(/** @type {CanvasRenderingContext2D} */ c) {
  if (!glowCtx) return;
  const gw = Math.max(1, Math.round(width / GLOW_SCALE));
  const gh = Math.max(1, Math.round(height / GLOW_SCALE));
  if (glowCanvas.width !== gw || glowCanvas.height !== gh) {
    glowCanvas.width = gw;
    glowCanvas.height = gh;
  } else {
    glowCtx.clearRect(0, 0, gw, gh);
  }
  glowCtx.imageSmoothingQuality = "medium";
  glowCtx.drawImage(canvas, 0, 0, canvas.width, canvas.height, 0, 0, gw, gh);
  c.save();
  c.setTransform(1, 0, 0, 1, 0, 0);
  c.globalCompositeOperation = "destination-over";
  c.globalAlpha = 0.95;
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.drawImage(glowCanvas, 0, 0, gw, gh, 0, 0, canvas.width, canvas.height);
  c.restore();
}

// ---- the loop: runs only while something moves

let raf = 0;
let running = false;
let lastFrame = 0;

/** @param {number} now */
function tick(now) {
  raf = requestAnimationFrame(tick);
  if (now - lastFrame < FRAME_MS - 1) return;
  const dt = lastFrame === 0 ? FRAME_MS : Math.min(100, now - lastFrame);
  lastFrame = now;
  smoothInto(levels, targets, dt, config.smoothing);
  stepPeaks(peaks, levels, dt);
  redraw();
  if (silent && hasSettled(levels, targets)) {
    sleep();
    root.toggleAttribute("data-idle", true);
  }
}

function wake() {
  if (running || !visible) return;
  running = true;
  lastFrame = 0;
  raf = requestAnimationFrame(tick);
}

function sleep() {
  cancelAnimationFrame(raf);
  running = false;
}

/** @param {number[]} bins */
function onSpectrum(bins) {
  latest = bins;
  silent = isSilent(bins);
  if (!silent) root.toggleAttribute("data-idle", false);
  computeTargets();
  wake();
}

// ---- wiring

/** @type {(() => void) | undefined} */
let unsubscribe;
function subscribe() {
  unsubscribe?.();
  unsubscribe = mypaper.audio.spectrum(onSpectrum, { bins: config.bins });
}

/** @template {readonly string[]} L @param {string} v @param {L} list @param {L[number]} fallback @returns {L[number]} */
const pick = (v, list, fallback) => list.find((x) => x === v) ?? fallback;

trackSettings(DEFAULTS, (s) => {
  const binsChanged = s.bins !== config.bins;
  config = {
    ...s,
    variant: pick(s.variant, VARIANTS, "bars"),
    colorMode: pick(s.colorMode, COLOR_MODES, "gradient"),
    bins: clamp(Math.round(s.bins), 64, 128),
    smoothing: clamp01(s.smoothing),
    sensitivity: clamp(s.sensitivity, 0.2, 4),
    barWidth: clamp(s.barWidth, 1, 32),
    gap: clamp(s.gap, 0, 32),
  };
  root.dataset["variant"] = config.variant;
  levels = []; // the count may have changed with the look
  rebuild();
  if (binsChanged && visible) subscribe();
  redraw();
});

fitCanvas(canvas, (w, h) => {
  width = w;
  height = h;
  levels = [];
  rebuild();
  redraw();
});

void mypaper.appearance.get().then(() => requestAnimationFrame(readLook), () => {});
mypaper.appearance.onChange(() => requestAnimationFrame(readLook));

onVisible(
  () => {
    visible = true;
    root.toggleAttribute("data-idle", silent);
    subscribe();
    redraw();
  },
  () => {
    visible = false;
    unsubscribe?.();
    unsubscribe = undefined;
    sleep();
  },
);
