// Appearance: how the host's resolved look (docs/widgets.md, "Appearance") becomes attributes and
// CSS variables on <html>. The mapping is pure (and tested); `applyAppearance` is the DOM half.
// The `mypaper.js` drop-in calls `applyAppearance` for you; with the ES module, call it yourself.

import type { Appearance, AppearanceState } from "./types";

/** The look a widget gets before the host answers (and from hosts that send none): the spec's defaults. */
export const DEFAULT_APPEARANCE: Appearance = {
  surface: "none",
  tint: "#101010",
  opacity: 0.55,
  blur: 24,
  radius: 18,
  border: true,
  text: "light",
  accent: "#ffffff",
  font: '"Segoe UI Variable Display", "Segoe UI Variable Text", "Segoe UI"',
  scale: 1,
  shadow: true,
};

/** What the default (near-black) tint becomes with dark text. */
const LIGHT_TINT = "#f2f2f2";

/** Attributes set on <html>. */
export interface AppearanceAttributes {
  "data-surface": Appearance["surface"];
  "data-text": Appearance["text"];
  "data-shadow": "on" | "off";
}

const HEX = /^#[0-9a-f]{6}$/i;

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));
const num = (n: unknown, fallback: number, lo: number, hi: number) =>
  typeof n === "number" && Number.isFinite(n) ? clamp(n, lo, hi) : fallback;

/** `#rrggbb` to its channels; anything else falls back (values come from JSON a user can edit). */
function rgb(hex: unknown, fallback: string): [number, number, number] {
  const value = typeof hex === "string" && HEX.test(hex) ? hex : fallback;
  const n = Number.parseInt(value.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** WCAG relative luminance, 0 (black) to 1 (white). */
function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** Keeps a font list from breaking out of its declaration. */
function fontStack(font: unknown): string {
  const clean = typeof font === "string" ? font.replace(/[;{}<>\\@]|\/\*/g, "").trim() : "";
  return `${clean || DEFAULT_APPEARANCE.font}, system-ui, sans-serif`;
}

/** Fills missing or invalid fields with the defaults and clamps the numbers to their ranges. */
export function normalizeAppearance(input: Partial<Record<keyof Appearance, unknown>> = {}): Appearance {
  const d = DEFAULT_APPEARANCE;
  const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T =>
    allowed.find((a) => a === value) ?? fallback;
  return {
    surface: pick(input.surface, ["none", "glass", "solid"], d.surface),
    tint: typeof input.tint === "string" && HEX.test(input.tint) ? input.tint : d.tint,
    opacity: num(input.opacity, d.opacity, 0, 1),
    blur: num(input.blur, d.blur, 0, 64),
    radius: num(input.radius, d.radius, 0, 48),
    border: typeof input.border === "boolean" ? input.border : d.border,
    text: pick(input.text, ["light", "dark"], d.text),
    accent: typeof input.accent === "string" && HEX.test(input.accent) ? input.accent : d.accent,
    font: typeof input.font === "string" && input.font.trim() ? input.font : d.font,
    scale: num(input.scale, d.scale, 0.5, 2),
    shadow: typeof input.shadow === "boolean" ? input.shadow : d.shadow,
  };
}

/** `data-surface`, `data-text` and `data-shadow`. */
export function appearanceAttributes(a: Appearance): AppearanceAttributes {
  return {
    "data-surface": a.surface,
    "data-text": a.text,
    "data-shadow": a.shadow ? "on" : "off",
  };
}

/**
 * The CSS variables, by name.
 *
 * - `--surface` is the tint at `opacity`; a `solid` surface is always opaque (use `glass` for a
 *   translucent one). The default tint (`#101010`) becomes `#f2f2f2` with dark text.
 * - `--t0`..`--t3` and `--fill-1`..`--fill-3` are the ink (white or near-black, by `text`) at fixed
 *   alphas, so contrast steps stay the same in both tones.
 * - `--ink` and `--halo` are space-separated channel triples (`rgb(var(--ink) / .3)`): the ink
 *   itself, and its opposite, used for the soft shadow under text on a bare wallpaper.
 * - `--accent` is the accent. The default (white) follows the ink in dark text, where white would vanish.
 *   `--on-accent` is the ink that reads on top of it.
 */
export function appearanceVars(a: Appearance): Record<string, string> {
  const dark = a.text === "dark";
  const ink = dark ? "10 10 12" : "255 255 255";
  const halo = dark ? "255 255 255" : "0 0 0";
  // The default tint is near-black: with dark text it would be dark on dark, so it flips with the ink.
  const tint = dark && a.tint.toLowerCase() === DEFAULT_APPEARANCE.tint ? LIGHT_TINT : a.tint;
  const [tr, tg, tb] = rgb(tint, DEFAULT_APPEARANCE.tint);
  const alpha = a.surface === "solid" ? 1 : a.opacity;

  const accentRgb = rgb(a.accent, DEFAULT_APPEARANCE.accent);
  const neutralAccent = a.accent.toLowerCase() === DEFAULT_APPEARANCE.accent && dark;
  const accent = neutralAccent ? "#0a0a0c" : `rgb(${accentRgb.join(" ")})`;
  const onAccent = neutralAccent ? "#ffffff" : luminance(accentRgb) > 0.4 ? "#0a0a0c" : "#ffffff";

  const step = (alphas: readonly number[], prefix: string) =>
    Object.fromEntries(alphas.map((al, i) => [`${prefix}${i}`, `rgb(${ink} / ${al})`]));

  return {
    "--surface": `rgb(${tr} ${tg} ${tb} / ${alpha})`,
    "--blur": `${a.blur}px`,
    "--radius": `${a.radius}px`,
    "--line": a.border ? `rgb(${ink} / 0.14)` : "transparent",
    ...step([1, 0.82, 0.6, 0.38], "--t"),
    "--fill-1": `rgb(${ink} / ${dark ? 0.07 : 0.08})`,
    "--fill-2": `rgb(${ink} / ${dark ? 0.12 : 0.14})`,
    "--fill-3": `rgb(${ink} / ${dark ? 0.2 : 0.24})`,
    "--ink": ink,
    "--halo": halo,
    "--accent": accent,
    "--on-accent": onAccent,
    "--font": fontStack(a.font),
    "--scale": String(a.scale),
  };
}

/** The `:root { ... }` rule for `appearanceVars`. */
export function appearanceRule(a: Appearance): string {
  const body = Object.entries(appearanceVars(a))
    .map(([name, value]) => `${name}:${value}`)
    .join(";");
  return `:root{${body}}`;
}

const APPEARANCE_STYLE = "mypaper-appearance";
const THEME_STYLE = "mypaper-theme";

function styleElement(doc: Document, id: string): HTMLStyleElement {
  const existing = doc.getElementById(id);
  if (existing instanceof HTMLStyleElement) return existing;
  const style = doc.createElement("style");
  style.id = id;
  return style;
}

/**
 * Applies an appearance to a document: the attributes on <html>, one `<style>` with the variables,
 * and the theme CSS in a `<style>` after it. The theme style is always the last one in the head,
 * so a theme's rules win over the widget's own CSS and can override any variable.
 */
export function applyAppearance(
  state: AppearanceState,
  widgetId: string | undefined,
  doc: Document = document,
): void {
  const a = normalizeAppearance(state.appearance);
  const root = doc.documentElement;
  for (const [name, value] of Object.entries(appearanceAttributes(a))) root.setAttribute(name, value);
  if (widgetId) root.setAttribute("data-widget", widgetId);

  const head = doc.head;
  const vars = styleElement(doc, APPEARANCE_STYLE);
  vars.textContent = appearanceRule(a);
  head.append(vars);

  const theme = styleElement(doc, THEME_STYLE);
  const css = state.themeCss ?? "";
  theme.textContent = css;
  // Appending an attached node moves it to the end: the theme CSS stays last.
  head.append(theme);
}
