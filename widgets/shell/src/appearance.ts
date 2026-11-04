/*
 * Appearance layers (docs/widgets.md, "Appearance"). Pure: no DOM, no state.
 *
 * The host sends the layers below an instance's own look (`base`: built-in defaults and the theme;
 * `user`: the global choice) and each instance's own layer, plus the merged result. The editing
 * panel needs the same merge locally so a slider can preview before anything is saved.
 */

import type { AppearanceValues, Layer, LayerPatch, WidgetStyle } from "./protocol";

/** The design languages, in the order the style control shows them. */
export const STYLES = ["minimal", "depth", "neon", "paper", "aero", "liquid"] as const satisfies readonly WidgetStyle[];

export interface Range {
  min: number;
  max: number;
  step: number;
}

/** The numeric fields' ranges: the same limits the host validates. */
export const RANGES = {
  opacity: { min: 0, max: 1, step: 0.01 },
  blur: { min: 0, max: 64, step: 1 },
  radius: { min: 0, max: 48, step: 1 },
  scale: { min: 0.5, max: 2, step: 0.05 },
} as const satisfies Partial<Record<keyof AppearanceValues, Range>>;

/** Font presets offered next to the free-text field. */
export const FONT_PRESETS = [
  "Segoe UI Variable",
  "Bahnschrift",
  "Cascadia Mono, Consolas, monospace",
  "Georgia, serif",
] as const;

export const HEX = /^#[0-9a-f]{6}$/i;

/** `layer` with `patch` applied: a value sets a field, `null` clears it. */
export function applyPatch(layer: Layer, patch: LayerPatch): Layer {
  const next: Layer = { ...layer };
  for (const key of Object.keys(patch) as (keyof AppearanceValues)[]) {
    const value = patch[key];
    if (value === null) delete next[key];
    else if (value !== undefined) Object.assign(next, { [key]: value });
  }
  return next;
}

/** `top` over `below`: every field `top` sets wins. */
export function over(below: AppearanceValues, top: Layer): AppearanceValues {
  const merged: AppearanceValues = { ...below };
  for (const key of Object.keys(top) as (keyof AppearanceValues)[]) {
    const value = top[key];
    if (value !== undefined) Object.assign(merged, { [key]: value });
  }
  return merged;
}

// The built-in tint and accent, and what they become with dark text (a near-black tint would be dark
// on dark, a white accent would vanish). Mirrors `look.rs` in the host, which sends final values;
// this copy only exists so a live preview matches what the host will send.
const DEFAULT_TINT = "#343a42";
const DEFAULT_ACCENT = "#ffffff";
const DARK_TEXT_TINT = "#f2f2f2";
const DARK_TEXT_ACCENT = "#0a0a0c";

/**
 * What a widget shows: `own` over `user` over `base`, with `accent: auto` replaced by `autoAccent`
 * (the wallpaper palette's accent, resolved by the host). With dark text, a tint or accent that is
 * still the built-in default becomes the dark-text default. A `base` without a `style` (a host
 * older than v2) shows `minimal`.
 */
export function resolve(base: AppearanceValues, user: Layer, own: Layer, autoAccent: string): AppearanceValues {
  const merged = over(over(base, user), own);
  if (!STYLES.includes(merged.style)) merged.style = "minimal";
  if (merged.accent === "auto") merged.accent = autoAccent;
  if (merged.text === "dark") {
    if (merged.tint.toLowerCase() === DEFAULT_TINT) merged.tint = DARK_TEXT_TINT;
    if (merged.accent.toLowerCase() === DEFAULT_ACCENT) merged.accent = DARK_TEXT_ACCENT;
  }
  return merged;
}

export type Scope = { kind: "user" } | { kind: "instance"; instance: string };

export interface PreviewContext {
  base: AppearanceValues;
  user: Layer;
  autoAccent: string;
  instances: readonly { instance: string; own: Layer }[];
}

/**
 * Every instance's look if `layer` were the edited layer of `scope`. A user-layer edit reaches every
 * instance that doesn't set the field itself; an instance edit changes only that instance.
 */
export function preview(context: PreviewContext, scope: Scope, layer: Layer): Map<string, AppearanceValues> {
  const { base, user, autoAccent, instances } = context;
  const looks = new Map<string, AppearanceValues>();
  for (const { instance, own } of instances) {
    if (scope.kind === "instance" && scope.instance !== instance) continue;
    looks.set(
      instance,
      scope.kind === "user" ? resolve(base, layer, own, autoAccent) : resolve(base, user, layer, autoAccent),
    );
  }
  return looks;
}

/** The layer an edited field falls back to when cleared: the values showing through from below. */
export function below(scope: Scope, base: AppearanceValues, user: Layer): AppearanceValues {
  return scope.kind === "user" ? base : over(base, user);
}
