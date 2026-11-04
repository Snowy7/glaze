// How widgets and bars look: the built-in layer, layer merging, and the CSS
// variables and data attributes the preview paints with. The variables mirror
// docs/widgets.md ("Appearance") and packages/widget-sdk/widget-kit/base.css,
// so the preview and the real thing read the same.

import type { CSSProperties } from "react";

import type { Appearance, SkinKind } from "./api";

/** An appearance with every field decided. */
export type Resolved = Required<Appearance>;

/** Built-in defaults for widgets (docs/widgets.md). */
export const WIDGET_DEFAULTS: Resolved = {
  style: "minimal",
  surface: "none",
  tint: "#101010",
  opacity: 0.55,
  blur: 24,
  radius: 18,
  border: true,
  text: "light",
  accent: "#ffffff",
  font: "Segoe UI Variable",
  scale: 1,
  shadow: true,
};

/** Built-in defaults for the top bar (docs/bars.md). The dock's radius is `DOCK_RADIUS`. */
export const BAR_DEFAULTS: Resolved = {
  ...WIDGET_DEFAULTS,
  surface: "glass",
  tint: "#0e0e0e",
  opacity: 0.6,
  blur: 30,
  radius: 0,
};

export const DOCK_RADIUS = 16;

export const BUILT_IN: Record<SkinKind, Resolved> = { widgets: WIDGET_DEFAULTS, bars: BAR_DEFAULTS };

/** `layers` over `base`, lowest first: each set field wins over the ones before it. */
export function resolve(base: Resolved, ...layers: Appearance[]): Resolved {
  const merged: Resolved = { ...base };
  for (const layer of layers) {
    for (const key of APPEARANCE_KEYS) {
      const value = layer[key];
      if (value !== undefined) Object.assign(merged, { [key]: value });
    }
  }
  return merged;
}

export const APPEARANCE_KEYS = [
  "style",
  "surface",
  "tint",
  "opacity",
  "blur",
  "radius",
  "border",
  "text",
  "accent",
  "font",
  "scale",
  "shadow",
] as const satisfies readonly (keyof Appearance)[];

/** How many fields the layer sets. */
export function setCount(appearance: Appearance): number {
  return APPEARANCE_KEYS.filter((key) => appearance[key] !== undefined).length;
}

/** `appearance` without one field. */
export function without(appearance: Appearance, key: keyof Appearance): Appearance {
  const { [key]: _removed, ...rest } = appearance;
  return rest;
}

export const FONT_PRESETS = [
  { label: "Segoe UI", value: "Segoe UI Variable" },
  { label: "Bahnschrift", value: "Bahnschrift, Segoe UI, sans-serif" },
  { label: "Georgia", value: "Georgia, Times New Roman, serif" },
  { label: "Cascadia", value: "Cascadia Mono, Consolas, monospace" },
] as const;

/** What `accent: auto` stands for in a preview: the wallpaper's own color isn't known here. */
export const AUTO_ACCENT_PREVIEW = "#c8c8c8";

/** `#rrggbb` at an opacity, as a CSS color. */
export function withAlpha(hex: string, alpha: number): string {
  const n = Number.parseInt(hex.slice(1), 16);
  return `rgb(${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255} / ${alpha})`;
}

type Vars = CSSProperties & Record<`--${string}`, string>;

/** The data attributes the real pages put on `<html>`. */
export function appearanceData(a: Resolved) {
  return {
    "data-style": a.style,
    "data-surface": a.surface,
    "data-text": a.text,
    "data-shadow": a.shadow ? "on" : "off",
  } as const;
}

/** The CSS variables the real pages set on `<html>`, as an inline style. */
export function appearanceVars(a: Resolved): Vars {
  const light = a.text === "light";
  const ink = light ? "255 255 255" : "10 10 10";
  const over = (alpha: number) => `rgb(${ink} / ${alpha})`;
  return {
    "--surface": withAlpha(a.tint, a.opacity),
    "--blur": `${a.blur}px`,
    "--radius": `${a.radius}px`,
    "--line": a.border ? over(0.1) : "transparent",
    "--t0": light ? "#fff" : "#0a0a0a",
    "--t1": over(0.74),
    "--t2": over(0.46),
    "--t3": over(0.24),
    "--fill-1": over(light ? 0.06 : 0.05),
    "--fill-2": over(light ? 0.11 : 0.09),
    "--fill-3": over(light ? 0.2 : 0.16),
    "--accent": a.accent === "auto" ? AUTO_ACCENT_PREVIEW : a.accent,
    "--font": `${a.font}, "Segoe UI", system-ui, sans-serif`,
    "--scale": String(a.scale),
  };
}
