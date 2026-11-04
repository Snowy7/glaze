// Turning a bars `look` into CSS for Studio's previews. The real page does this with CSS variables
// (bars/shell/src/style/look.ts); the preview only needs to read the same way, so this is a
// plain, approximate mapping: tokens get stand-in colors, `var(--x)` reads the file's `vars`.

import type { Appearance } from "./api";
import { BAR_DEFAULTS } from "./appearance";
import type { Color, Fill, Gradient, Json, Paint, Stop } from "./barsSchema";

export type ColorContext = { text: string; accent: string; vars: Record<string, Json> };

/** Stand-ins for Windows' accent shades; the real ones come from the user's Windows. */
const SYSTEM_ACCENT: Record<string, string> = {
  "system-accent": "#4f8cff",
  "system-accent-light-1": "#6ea0ff",
  "system-accent-light-2": "#8fb6ff",
  "system-accent-light-3": "#b4cdff",
  "system-accent-dark-1": "#3a73e0",
  "system-accent-dark-2": "#2b5bb8",
  "system-accent-dark-3": "#1e4590",
};

/** A bars color as CSS, or undefined for one that can't be resolved. */
export function cssColor(color: Color | undefined, ctx: ColorContext): string | undefined {
  if (color === undefined) return undefined;
  if (color.startsWith("#")) return color;
  if (color === "accent") return ctx.accent;
  if (color === "text") return ctx.text;
  if (color === "transparent") return "transparent";
  const system = SYSTEM_ACCENT[color];
  if (system) return system;
  const variable = /^var\((--[\w-]+)\)$/.exec(color)?.[1];
  const value = variable ? ctx.vars[variable] : undefined;
  return typeof value === "string" ? value : "#888888";
}

const stopCss = (stop: Stop, ctx: ColorContext): string => {
  if (typeof stop === "string") return cssColor(stop, ctx) ?? "transparent";
  return `${cssColor(stop.color, ctx) ?? "transparent"} ${Math.round(stop.at * 100)}%`;
};

export function gradientCss(gradient: Gradient, ctx: ColorContext): string {
  const stops = gradient.stops.map((s) => stopCss(s, ctx)).join(", ");
  const angle = gradient.angle ?? 180;
  switch (gradient.kind ?? "linear") {
    case "radial":
      return `radial-gradient(circle, ${stops})`;
    case "conic":
      return `conic-gradient(from ${angle}deg, ${stops})`;
    case "linear":
      return `linear-gradient(${angle}deg, ${stops})`;
  }
}

/** A paint as a CSS `background` value. */
export function paintCss(paint: Paint | undefined, ctx: ColorContext): string | undefined {
  if (paint === undefined) return undefined;
  return typeof paint === "string" ? cssColor(paint, ctx) : gradientCss(paint, ctx);
}

/** `#rgb`…`#rrggbbaa` split into an `#rrggbb` and an alpha 0..1; null for anything else. */
export function splitHex(color: string): { rgb: string; alpha: number } | null {
  const hex = /^#([0-9a-f]{3,8})$/i.exec(color)?.[1];
  if (!hex || ![3, 4, 6, 8].includes(hex.length)) return null;
  const full = hex.length <= 4 ? [...hex].map((c) => c + c).join("") : hex;
  const alpha = full.length === 8 ? Number.parseInt(full.slice(6), 16) / 255 : 1;
  return { rgb: `#${full.slice(0, 6).toLowerCase()}`, alpha };
}

/** `#rrggbb` and an alpha as the shortest hex the host takes. */
export function joinHex(rgb: string, alpha: number): string {
  if (alpha >= 0.999) return rgb;
  return `${rgb}${Math.round(alpha * 255).toString(16).padStart(2, "0")}`;
}

/** The pieces of the appearance layer the preview needs, with the built-in defaults under them. */
export function surfaceBase(appearance: Appearance | undefined) {
  const a = { ...BAR_DEFAULTS, ...definedOnly(appearance ?? {}) };
  return { tint: a.tint, opacity: a.opacity, blur: a.blur, radius: a.radius, text: a.text, accent: a.accent === "auto" ? "#c8c8c8" : a.accent };
}

function definedOnly<T extends object>(value: T): Partial<T> {
  return Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined)) as Partial<T>;
}

/** Whether a fill shows the wallpaper through (glass) rather than painting over it. */
export function fillShowsWallpaper(fill: Fill | undefined): boolean {
  return fill === undefined || fill.type === "wallpaper";
}
