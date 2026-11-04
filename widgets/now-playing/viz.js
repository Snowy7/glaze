// @ts-check
// Pure geometry and smoothing for the Visualizer look (spectrum.js draws). No DOM and no canvas
// here: tested in packages/widget-sdk/test/viz.test.ts.
//
// The spectrum the host sends is already log-spaced (about 40 Hz to 16 kHz), so a bin index is a
// position on a log-frequency axis: laying bins out evenly *is* the log mapping.

/** One tick of the 30 Hz spectrum, in ms: smoothing rates are per tick and scaled by the real dt. */
const TICK_MS = 1000 / 30;

/** @param {number} n */
export const clamp01 = (n) => Math.min(1, Math.max(0, n));

/**
 * True when every bin is (nearly) zero: the host's silence.
 * @param {readonly number[]} bins
 */
export const isSilent = (bins) => bins.every((v) => !(v > 0.002));

/**
 * `bins` resampled to `count` values on the same axis. Fewer bars than bins: each bar takes the
 * loudest bin it covers (a mean would hide transients). More bars than bins: linear interpolation.
 * @param {readonly number[]} bins
 * @param {number} count
 * @returns {number[]}
 */
export function sampleBars(bins, count) {
  const n = bins.length;
  if (count <= 0 || n === 0) return [];
  const step = n / count;
  /** @type {number[]} */
  const out = [];
  for (let i = 0; i < count; i++) {
    if (step > 1) {
      const from = Math.floor(i * step);
      const to = Math.max(from + 1, Math.floor((i + 1) * step));
      let loudest = 0;
      for (let j = from; j < to && j < n; j++) loudest = Math.max(loudest, bins[j] ?? 0);
      out.push(loudest);
    } else {
      const pos = count === 1 ? 0 : (i / (count - 1)) * (n - 1);
      const lo = Math.floor(pos);
      const f = pos - lo;
      out.push((bins[lo] ?? 0) * (1 - f) + (bins[Math.min(n - 1, lo + 1)] ?? 0) * f);
    }
  }
  return out;
}

/**
 * The level a bar shows: the bin times the user's sensitivity, tilted up toward the highs (music
 * carries far less energy there, so a flat mapping leaves the right half of the widget dead).
 * @param {number} value 0..1
 * @param {number} pos 0 (lowest bar) .. 1 (highest bar)
 * @param {number} sensitivity 1 is neutral
 */
export const shape = (value, pos, sensitivity) => clamp01(value * sensitivity * (0.8 + 0.5 * pos));

/**
 * A calm static profile (a low arch with a ripple) shown when nothing plays.
 * @param {number} count
 * @returns {number[]}
 */
export function idleLevels(count) {
  return Array.from({ length: count }, (_, i) => {
    const pos = count <= 1 ? 0.5 : i / (count - 1);
    return 0.035 + 0.05 * Math.sin(pos * Math.PI) + 0.012 * Math.sin(pos * 31);
  });
}

/**
 * Moves `levels` toward `targets` in place: quickly up, slowly down. `smoothing` is the user's
 * 0..1 knob (0: follow the host as it is, 1: very syrupy). Rates are per 30 Hz tick, so the result
 * doesn't depend on the frame rate.
 * @param {number[]} levels
 * @param {readonly number[]} targets same length
 * @param {number} dtMs
 * @param {number} smoothing
 */
export function smoothInto(levels, targets, dtMs, smoothing) {
  const ticks = Math.max(0, dtMs) / TICK_MS;
  const s = clamp01(smoothing);
  const rise = 1 - Math.pow(0.5 * s, ticks);
  const fall = 1 - Math.pow(0.4 + 0.55 * s, ticks);
  for (let i = 0; i < levels.length; i++) {
    const target = targets[i] ?? 0;
    const level = levels[i] ?? 0;
    const next = level + (target - level) * (target > level ? rise : fall);
    levels[i] = Math.abs(target - next) < 0.003 ? target : next;
  }
}

/**
 * True when `levels` has reached `targets` (so nothing moves and the loop can stop).
 * @param {readonly number[]} levels
 * @param {readonly number[]} targets
 */
export const hasSettled = (levels, targets) => levels.every((l, i) => Math.abs(l - (targets[i] ?? 0)) < 0.003);

/**
 * Peak-hold state: each bin's cap level and how long it still holds, in ms.
 * @typedef {{ level: number[], hold: number[] }} Peaks
 */

/**
 * @param {number} count
 * @returns {Peaks}
 */
export const createPeaks = (count) => ({ level: new Array(count).fill(0), hold: new Array(count).fill(0) });

/**
 * A cap jumps up with its bar, holds for `holdMs`, then falls at `fallPerSec` (levels per second),
 * never below the bar.
 * @param {Peaks} peaks
 * @param {readonly number[]} values the bars' current levels
 * @param {number} dtMs
 * @param {number} [holdMs]
 * @param {number} [fallPerSec]
 */
export function stepPeaks(peaks, values, dtMs, holdMs = 320, fallPerSec = 1.4) {
  for (let i = 0; i < peaks.level.length; i++) {
    const v = values[i] ?? 0;
    const level = peaks.level[i] ?? 0;
    const hold = peaks.hold[i] ?? 0;
    if (v >= level) {
      peaks.level[i] = v;
      peaks.hold[i] = holdMs;
    } else if (hold > 0) {
      peaks.hold[i] = hold - dtMs;
    } else {
      peaks.level[i] = Math.max(v, level - (fallPerSec * dtMs) / 1000);
    }
  }
}

/**
 * How many bars of `barWidth` with `gap` fit in `width`, and their pitch (bar plus gap) and real
 * width, stretched a little so the row fills the width exactly: bar `i` is at `x = i * pitch`.
 * @param {number} width
 * @param {number} barWidth
 * @param {number} gap
 */
export function barLayout(width, barWidth, gap) {
  const g = Math.max(0, gap);
  const count = Math.max(1, Math.floor((Math.max(1, width) + g) / (Math.max(1, barWidth) + g)));
  const pitch = (Math.max(1, width) + g) / count;
  return { count, pitch, bar: Math.max(1, pitch - g) };
}

/**
 * Vertical extent of a bar of `level` in a box `height` high: from the bottom, or centered when
 * mirrored. Never shorter than `minHeight`, so silence still shows a row of dots.
 * @param {number} level 0..1
 * @param {number} height
 * @param {boolean} mirror
 * @param {number} minHeight
 */
export function barSpan(level, height, mirror, minHeight) {
  const h = Math.min(height, Math.max(minHeight, level * height));
  return { y: mirror ? (height - h) / 2 : height - h, h };
}

/**
 * Points of a wave's upper edge: x from 0 to `width`, `amp * level` above `baseY`. The ends ease
 * down to the baseline over the first and last 8% so the shape closes softly.
 * @param {readonly number[]} levels
 * @param {number} width
 * @param {number} baseY
 * @param {number} amp
 * @returns {{ x: number, y: number }[]}
 */
export function wavePoints(levels, width, baseY, amp) {
  const n = levels.length;
  return levels.map((level, i) => {
    const pos = n <= 1 ? 0.5 : i / (n - 1);
    const edge = Math.min(pos, 1 - pos) / 0.08;
    const taper = edge >= 1 ? 1 : edge * edge * (3 - 2 * edge); // smoothstep
    return { x: pos * width, y: baseY - level * amp * taper };
  });
}

/**
 * A smooth curve through `points` as cubic Bezier segments (Catmull-Rom), to draw with
 * `bezierCurveTo` after a `moveTo(points[0])`.
 * @param {readonly { x: number, y: number }[]} points
 * @returns {{ c1x: number, c1y: number, c2x: number, c2y: number, x: number, y: number }[]}
 */
export function curveSegments(points) {
  /** @type {{ c1x: number, c1y: number, c2x: number, c2y: number, x: number, y: number }[]} */
  const out = [];
  for (let i = 0; i < points.length - 1; i++) {
    const p1 = points[i];
    const p2 = points[i + 1];
    if (!p1 || !p2) continue;
    const p0 = points[i - 1] ?? p1;
    const p3 = points[i + 2] ?? p2;
    out.push({
      c1x: p1.x + (p2.x - p0.x) / 6,
      c1y: p1.y + (p2.y - p0.y) / 6,
      c2x: p2.x - (p3.x - p1.x) / 6,
      c2y: p2.y - (p3.y - p1.y) / 6,
      x: p2.x,
      y: p2.y,
    });
  }
  return out;
}

/**
 * Radial bars: each value is a spoke from the circle of radius `inner` outward. Spokes start at
 * `start` (radians, canvas convention: 0 is east, y down) and run clockwise around the circle. With
 * `mirror` the values cover half the circle and are reflected across the vertical axis, so the
 * result has twice as many spokes and is left-right symmetric.
 * @param {readonly number[]} levels
 * @param {{ cx: number, cy: number, inner: number, length: number, minLength: number, mirror: boolean, start?: number }} o
 * @returns {{ angle: number, x0: number, y0: number, x1: number, y1: number }[]}
 */
export function radialBars(levels, { cx, cy, inner, length, minLength, mirror, start = Math.PI / 2 }) {
  const n = levels.length;
  const span = mirror ? Math.PI : Math.PI * 2;
  /** @type {{ angle: number, x0: number, y0: number, x1: number, y1: number }[]} */
  const out = [];
  /** @param {number} angle @param {number} level */
  const spoke = (angle, level) => {
    const len = Math.max(minLength, level * length);
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    out.push({ angle, x0: cx + cos * inner, y0: cy + sin * inner, x1: cx + cos * (inner + len), y1: cy + sin * (inner + len) });
  };
  levels.forEach((level, i) => {
    const angle = start + ((i + 0.5) / n) * span;
    spoke(angle, level);
    // Reflection across the vertical axis through the start angle (the axis the spokes begin on).
    if (mirror) spoke(2 * start - angle, level);
  });
  return out;
}

/**
 * The energy of the lowest part of the spectrum (the kick), 0..1.
 * @param {readonly number[]} levels
 */
export function bassEnergy(levels) {
  const n = Math.max(1, Math.ceil(levels.length * 0.15));
  let sum = 0;
  for (let i = 0; i < n; i++) sum += levels[i] ?? 0;
  return clamp01(sum / n);
}

/**
 * A hue in degrees for position `t` (0..1) across the rainbow: `spread` degrees wide from `start`.
 * @param {number} t
 * @param {number} [start]
 * @param {number} [spread]
 */
export const hueAt = (t, start = 330, spread = 290) => (((start + clamp01(t) * spread) % 360) + 360) % 360;

/**
 * @typedef {{ at: number, color: string }} ColorStop
 */

/**
 * Gradient stops for a color mode. `hi` and `hi-2` are the style's highlight pair (CSS colors the
 * canvas can parse); the rainbow ignores them. `lightness` (percent) keeps the rainbow readable
 * on light wallpapers.
 * @param {"accent" | "gradient" | "rainbow"} mode
 * @param {string} hi
 * @param {string} hi2
 * @param {number} [lightness]
 * @returns {ColorStop[]}
 */
export function paletteStops(mode, hi, hi2, lightness = 62) {
  if (mode === "accent") return [{ at: 0, color: hi }, { at: 1, color: hi }];
  if (mode === "gradient") return [{ at: 0, color: hi }, { at: 1, color: hi2 }];
  const steps = 8;
  return Array.from({ length: steps }, (_, i) => {
    const at = i / (steps - 1);
    return { at, color: `hsl(${hueAt(at).toFixed(0)} 88% ${lightness}%)` };
  });
}

/**
 * Stops that run forward then back, so a gradient closes on itself (a conic gradient around a
 * mirrored circle has no seam).
 * @param {readonly ColorStop[]} stops
 * @returns {ColorStop[]}
 */
export function pingPong(stops) {
  return [
    ...stops.map((s) => ({ at: s.at / 2, color: s.color })),
    ...[...stops].reverse().slice(1).map((s) => ({ at: 1 - s.at / 2, color: s.color })),
  ];
}
