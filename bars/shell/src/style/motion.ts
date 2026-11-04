// Motion (docs/bars.md, "Animation"): easing curves (named, cubic-bezier, springs rendered as
// CSS `linear()`), the animation tree resolved per bar and leaf, and the CSS variables the styles
// read (`--m-<leaf>`, `--m-<leaf>-ease`, `data-m-<leaf>`). Everything is CSS-driven: nothing here
// runs per frame.

import type { BarKind, Curve, Motion, MotionStyle, Spring } from "../protocol";

/** Named curves; a spring name renders like a spring object. Keep in sync with `NAMED_CURVES`. */
export const NAMED_CURVES: Readonly<Record<string, string | Spring>> = {
  linear: "linear",
  ease: "ease",
  "ease-in": "ease-in",
  "ease-out": "ease-out",
  "ease-in-out": "ease-in-out",
  smooth: "cubic-bezier(0.22, 0.7, 0.2, 1)",
  snappy: "cubic-bezier(0.2, 0.9, 0.1, 1)",
  emphasized: "cubic-bezier(0.2, 0, 0, 1)",
  overshoot: "cubic-bezier(0.34, 1.56, 0.64, 1)",
  spring: { mass: 1, stiffness: 220, damping: 22 },
  "spring-soft": { mass: 1, stiffness: 120, damping: 17 },
  "spring-bouncy": { mass: 1, stiffness: 260, damping: 13 },
};

/**
 * A damped spring's position at `t` seconds, released from 0 with no velocity toward rest at 1
 * (the solution of m·x'' + c·x' + k·(x − 1) = 0).
 */
export function springAt(t: number, { mass, stiffness, damping }: Spring): number {
  const w0 = Math.sqrt(stiffness / mass);
  const zeta = damping / (2 * Math.sqrt(stiffness * mass));
  if (zeta < 1) {
    const wd = w0 * Math.sqrt(1 - zeta * zeta);
    return 1 - Math.exp(-zeta * w0 * t) * (Math.cos(wd * t) + ((zeta * w0) / wd) * Math.sin(wd * t));
  }
  if (zeta === 1) return 1 - Math.exp(-w0 * t) * (1 + w0 * t);
  const root = Math.sqrt(zeta * zeta - 1);
  const r1 = -w0 * (zeta - root);
  const r2 = -w0 * (zeta + root);
  return 1 - (r2 * Math.exp(r1 * t) - r1 * Math.exp(r2 * t)) / (r2 - r1);
}

/** Seconds until the spring stays within `epsilon` of rest (sampled at 1 ms, capped at 3 s). */
export function springSettle(spring: Spring, epsilon = 0.001): number {
  const step = 0.001;
  let last = 0;
  for (let t = 0; t <= 3; t += step) {
    if (Math.abs(springAt(t, spring) - 1) > epsilon) last = t;
  }
  return Math.min(3, last + step);
}

const round = (value: number, digits: number): number => {
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
};

/**
 * The spring as a CSS `linear()` easing over its settling time: samples at 60 per second, with
 * points that a straight line between their neighbors already covers (within `tolerance`)
 * dropped, so the string stays short.
 */
export function springLinear(spring: Spring, tolerance = 0.002): { easing: string; duration: number } {
  const duration = Math.max(0.05, springSettle(spring));
  const count = Math.min(120, Math.max(12, Math.ceil(duration * 60)));
  const points = Array.from({ length: count + 1 }, (_, i) => {
    const at = i / count;
    return { at, value: i === count ? 1 : springAt(at * duration, spring) };
  });
  const kept = [points[0]!];
  for (let i = 1; i < points.length - 1; i++) {
    const before = kept[kept.length - 1]!;
    const here = points[i]!;
    const after = points[i + 1]!;
    const along = (here.at - before.at) / (after.at - before.at);
    const straight = before.value + along * (after.value - before.value);
    if (Math.abs(straight - here.value) > tolerance) kept.push(here);
  }
  kept.push(points[points.length - 1]!);
  const stops = kept.map((p, i) =>
    i === 0 || i === kept.length - 1 ? String(round(p.value, 4)) : `${round(p.value, 4)} ${round(p.at * 100, 2)}%`,
  );
  return { easing: `linear(${stops.join(", ")})`, duration: Math.round(duration * 1000) };
}

/** A curve as CSS. Springs carry their own duration (ms). */
export function easing(curve: Curve): { easing: string; duration?: number } {
  if (typeof curve === "string") {
    const named = NAMED_CURVES[curve] ?? NAMED_CURVES.smooth!;
    return typeof named === "string" ? { easing: named } : springLinear(named);
  }
  if (Array.isArray(curve)) return { easing: `cubic-bezier(${curve.join(", ")})` };
  return springLinear(curve);
}

/** What can be animated. `show` is the bar itself appearing (the dock's auto-hide reveal too). */
export const LEAVES = ["show", "hover", "press", "open", "magnify", "bounce", "attention"] as const;
export type Leaf = (typeof LEAVES)[number];

export interface ResolvedMotion {
  enabled: boolean;
  /** Milliseconds; 0 when disabled. */
  duration: number;
  easing: string;
  style: MotionStyle;
}

/** Each leaf's own timing and style, when nothing on its path sets them. */
const DEFAULTS: Record<Leaf, { duration: number; curve: Curve; style: MotionStyle }> = {
  show: { duration: 220, curve: "smooth", style: "fade" },
  hover: { duration: 150, curve: "smooth", style: "none" },
  press: { duration: 110, curve: "smooth", style: "none" },
  open: { duration: 170, curve: "smooth", style: "pop" },
  magnify: { duration: 80, curve: "linear", style: "none" },
  bounce: { duration: 560, curve: [0.3, 0, 0.4, 1], style: "none" },
  attention: { duration: 900, curve: "ease-in-out", style: "none" },
};

/** Dock-only leaves keep their own timing: they inherit `enabled`, not duration or curve. */
const OWN_TIMING: ReadonlySet<Leaf> = new Set(["magnify", "bounce", "attention"]);

/**
 * One leaf for one bar. Field by field, the most specific value wins, in this order (least
 * specific first): `animation`, `bar`, `bar.<top|dock>`, then the leaf under each of those.
 * Any `enabled: false` on the path turns the leaf off. A spring sets its own duration.
 * `magnify`, `bounce` and `attention` take timing only from `bar.dock.<leaf>`.
 */
export function resolveMotion(tree: Motion, bar: BarKind, leaf: Leaf): ResolvedMotion {
  const shared = tree.bar;
  const own = shared?.[bar];
  const chain: (Motion | undefined)[] =
    leaf === "show" ? [tree, shared, own] : [tree, shared, own, tree[leaf], shared?.[leaf], own?.[leaf]];
  const timing = leaf !== "show" && OWN_TIMING.has(leaf) ? [own?.[leaf]] : chain;
  const last = <K extends keyof Motion>(nodes: (Motion | undefined)[], key: K): Motion[K] | undefined =>
    nodes.reduce<Motion[K] | undefined>((found, node) => node?.[key] ?? found, undefined);
  const enabled = chain.every((node) => node?.enabled !== false);
  const fallback = DEFAULTS[leaf];
  const curve = easing(last(timing, "curve") ?? fallback.curve);
  const duration = curve.duration ?? last(timing, "duration") ?? fallback.duration;
  return {
    enabled,
    duration: enabled ? duration : 0,
    easing: curve.easing,
    style: last(chain, "style") ?? fallback.style,
  };
}

/** The CSS variables and attributes for every leaf of `bar`; all durations 0 when `reduce`. */
export function motionTokens(
  tree: Motion,
  bar: BarKind,
  reduce: boolean,
): { vars: Record<string, string>; attrs: Record<string, string> } {
  const vars: Record<string, string> = {};
  const attrs: Record<string, string> = { "data-motion": reduce ? "reduce" : "full" };
  for (const leaf of LEAVES) {
    const motion = resolveMotion(tree, bar, leaf);
    vars[`--m-${leaf}`] = `${reduce ? 0 : motion.duration}ms`;
    vars[`--m-${leaf}-ease`] = motion.easing;
    attrs[`data-m-${leaf}`] = reduce || !motion.enabled ? "none" : motion.style;
  }
  return { vars, attrs };
}

/** Milliseconds a leaf lasts right now, read back from the page (for JS that waits on CSS). */
export function leafDuration(leaf: Leaf): number {
  const value = getComputedStyle(document.documentElement).getPropertyValue(`--m-${leaf}`);
  return Number.parseFloat(value) || 0;
}
