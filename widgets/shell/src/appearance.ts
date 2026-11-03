/*
 * Appearance layers (docs/widgets.md, "Appearance"). Pure: no DOM, no state.
 *
 * The host sends the layers below an instance's own look (`base`: built-in defaults and the theme;
 * `user`: the global choice) and each instance's own layer, plus the merged result. The editing
 * panel needs the same merge locally so a slider can preview before anything is saved.
 */

import type { AppearanceValues, Layer, LayerPatch } from "./protocol";

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

/**
 * What a widget shows: `own` over `user` over `base`, with `accent: auto` replaced by `autoAccent`
 * (the wallpaper palette's accent, resolved by the host).
 */
export function resolve(base: AppearanceValues, user: Layer, own: Layer, autoAccent: string): AppearanceValues {
  const merged = over(over(base, user), own);
  return merged.accent === "auto" ? { ...merged, accent: autoAccent } : merged;
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
