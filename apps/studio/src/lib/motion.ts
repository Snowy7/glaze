// Motion for Studio's previews: a curve as CSS, and the animation tree resolved the way the bars
// page does (docs/bars.md, "Animation"). The curve mapping follows bars/shell/src/style/motion.ts
// so a preview moves like the real thing; the host itself only validates the tree.

import type { Curve, Motion, MotionStyle, NamedCurve, Spring } from "./barsSchema";

const NAMED: Record<NamedCurve, string | Spring> = {
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

/** A damped spring's position at `t` seconds, released from 0 toward rest at 1. */
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

/** Seconds until the spring stays within `epsilon` of rest (sampled at 1 ms, at most 3 s). */
export function springSettle(spring: Spring, epsilon = 0.001): number {
  let last = 0;
  for (let t = 0; t <= 3; t += 0.001) if (Math.abs(springAt(t, spring) - 1) > epsilon) last = t;
  return Math.min(3, last + 0.001);
}

const round = (value: number, digits: number) => Math.round(value * 10 ** digits) / 10 ** digits;

/** The spring as a CSS `linear()` easing over its settling time, and that time in ms. */
export function springLinear(spring: Spring): { easing: string; duration: number } {
  const duration = Math.max(0.05, springSettle(spring));
  const count = Math.min(90, Math.max(12, Math.ceil(duration * 60)));
  const stops = Array.from({ length: count + 1 }, (_, i) => {
    const at = i / count;
    const value = i === count ? 1 : springAt(at * duration, spring);
    return i === 0 || i === count ? String(round(value, 4)) : `${round(value, 4)} ${round(at * 100, 2)}%`;
  });
  return { easing: `linear(${stops.join(", ")})`, duration: Math.round(duration * 1000) };
}

/** A curve as CSS; a spring brings its own duration (ms). */
export function easing(curve: Curve): { easing: string; duration?: number } {
  if (typeof curve === "string") {
    const named = NAMED[curve];
    return typeof named === "string" ? { easing: named } : springLinear(named);
  }
  if (Array.isArray(curve)) return { easing: `cubic-bezier(${curve.join(", ")})` };
  return springLinear(curve);
}

/** A short description of a curve: its name, "cubic-bezier(…)" or the spring's numbers. */
export function describeCurve(curve: Curve): string {
  if (typeof curve === "string") return curve;
  if (Array.isArray(curve)) return `bezier ${curve.join(", ")}`;
  return `spring ${curve.mass}/${curve.stiffness}/${curve.damping}`;
}

export const LEAVES = ["hover", "press", "open", "magnify", "bounce", "attention"] as const;
export type Leaf = (typeof LEAVES)[number];
export type Scope = "global" | "bar" | "top" | "dock";

/** Each leaf's own timing when nothing on its path sets it (bars/shell/src/style/motion.ts). */
const DEFAULTS: Record<Leaf | "show", { duration: number; curve: Curve; style: MotionStyle }> = {
  show: { duration: 220, curve: "smooth", style: "fade" },
  hover: { duration: 150, curve: "smooth", style: "none" },
  press: { duration: 110, curve: "smooth", style: "none" },
  open: { duration: 170, curve: "smooth", style: "pop" },
  magnify: { duration: 80, curve: "linear", style: "none" },
  bounce: { duration: 560, curve: [0.3, 0, 0.4, 1], style: "none" },
  attention: { duration: 900, curve: "ease-in-out", style: "none" },
};

/** Magnify, bounce and attention take their timing only from `bar.dock.<leaf>`. */
const OWN_TIMING: ReadonlySet<string> = new Set(["magnify", "bounce", "attention"]);

/** Where a node sits: its path of keys under `animation`. */
export const SCOPE_PATH: Record<Scope, readonly string[]> = { global: [], bar: ["bar"], top: ["bar", "top"], dock: ["bar", "dock"] };

/** Which leaves a scope may hold (docs/bars.md: magnify, bounce and attention only under `bar.dock`). */
export function leavesOf(scope: Scope): readonly Leaf[] {
  return scope === "dock" ? LEAVES : ["hover", "press", "open"];
}

export type ResolvedLeaf = { enabled: boolean; duration: number; curve: Curve; style: MotionStyle };

/**
 * What a leaf (or, with `leaf` "show", the bar's own show animation) comes to for one bar: field
 * by field, the most specific value on its path wins; any `enabled: false` on the path turns it off.
 */
export function resolveLeaf(tree: Motion, bar: "top" | "dock", leaf: Leaf | "show"): ResolvedLeaf {
  const shared = tree.bar;
  const own = shared?.[bar];
  const chain: (Motion | undefined)[] = leaf === "show" ? [tree, shared, own] : [tree, shared, own, tree[leaf], shared?.[leaf], own?.[leaf]];
  const timing = leaf !== "show" && OWN_TIMING.has(leaf) ? [own?.[leaf as "magnify"]] : chain;
  const last = <K extends keyof Motion>(nodes: (Motion | undefined)[], key: K): Motion[K] | undefined =>
    nodes.reduce<Motion[K] | undefined>((found, node) => node?.[key] ?? found, undefined);
  const fallback = DEFAULTS[leaf];
  const curve = last(timing, "curve") ?? fallback.curve;
  return {
    enabled: chain.every((node) => node?.enabled !== false),
    duration: easing(curve).duration ?? last(timing, "duration") ?? fallback.duration,
    curve,
    style: last(chain, "style") ?? fallback.style,
  };
}
