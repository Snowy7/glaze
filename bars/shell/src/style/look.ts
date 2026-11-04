// The look (docs/bars.md, "Look"): `bars.json` → `look` over the resolved appearance, as CSS
// variables and `data-*` attributes on <html>. style.css draws every surface (`.surface`: the
// top bar, each island, the dock's pill) and item (`.mod`, `.slot`) from these, so a look is data,
// not code. Pure, so it can be tested.

import type {
  Appearance,
  GlassStyle,
  BorderStyle,
  ColorRef,
  Gradient,
  GlowStyle,
  ItemStyle,
  Look,
  Paint,
  PerState,
  ShadowStyle,
  SystemLook,
} from "../protocol";

const round = (value: number, digits = 3): number => Math.round(value * 10 ** digits) / 10 ** digits;

/** A color as CSS: hex and `var()` as written, tokens as the variables the page sets. */
export function cssColor(color: ColorRef): string {
  if (color.startsWith("#") || color.startsWith("var(")) return color;
  switch (color) {
    case "accent":
      return "var(--accent)";
    case "text":
      return "var(--t0)";
    case "transparent":
      return "transparent";
    default:
      return `var(--${color})`; // system-accent, system-accent-light-1 …
  }
}

/** The host fills `kind` (linear) and `angle` (180); hand-written ones may leave them out. */
export function gradientCss({ kind = "linear", angle = 180, stops }: Gradient): string {
  const list = stops
    .map((stop) => (typeof stop === "string" ? cssColor(stop) : `${cssColor(stop.color)} ${round(stop.at * 100, 2)}%`))
    .join(", ");
  switch (kind) {
    case "linear":
      return `linear-gradient(${angle}deg, ${list})`;
    case "radial":
      return `radial-gradient(circle at 50% 50%, ${list})`;
    case "conic":
      return `conic-gradient(from ${angle}deg, ${list})`;
  }
}

/** A paint as a CSS image (a color becomes a flat gradient), for layered backgrounds and borders. */
export function paintImage(paint: Paint): string {
  return typeof paint === "string" ? `linear-gradient(${cssColor(paint)}, ${cssColor(paint)})` : gradientCss(paint);
}

/** A paint as `[background-color, background-image]`, so a plain color can fade between states. */
export function paintLayers(paint: Paint): [string, string] {
  return typeof paint === "string" ? [cssColor(paint), "none"] : ["transparent", gradientCss(paint)];
}

/** A tile of fine monochrome grain at `opacity` (an SVG turbulence, rendered once). */
export function noiseImage(opacity: number): string {
  if (opacity <= 0) return "none";
  const alpha = round(Math.min(1, opacity * 2.2), 3);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="180" height="180">` +
    `<filter id="n"><feTurbulence type="fractalNoise" baseFrequency=".9" numOctaves="3" stitchTiles="stitch"/>` +
    `<feColorMatrix values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  ${alpha} 0 0 0 0"/></filter>` +
    `<rect width="100%" height="100%" filter="url(#n)"/></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

/** `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` → `[r, g, b]` (black when it isn't one). */
export function rgbOf(hex: string): [number, number, number] {
  const digits = /^#([0-9a-f]{3,8})$/i.exec(hex.trim())?.[1] ?? "000";
  const full = digits.length <= 4 ? [...digits].map((c) => c + c).join("") : digits;
  const n = Number.parseInt(full.slice(0, 6).padEnd(6, "0"), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** `tint` (a hex color) at `alpha`, as CSS. */
const tintOver = (tint: string, alpha: number): string => {
  const [r, g, b] = rgbOf(tint);
  return `rgb(${r} ${g} ${b} / ${round(alpha)})`;
};

export type Scheme = "light" | "dark";

/** Light or dark: the look's choice, Windows' for `auto`, else what the appearance's text implies. */
export function schemeOf(look: Look, appearance: Appearance, system: SystemLook): Scheme {
  if (look.scheme === "light" || look.scheme === "dark") return look.scheme;
  if (look.scheme === "auto") return system.scheme;
  return appearance.text === "light" ? "dark" : "light";
}

/**
 * The appearance as the scheme sees it: text follows the scheme, and a tint made for the other
 * scheme gives way to a neutral of this one.
 */
export function schemed(appearance: Appearance, scheme: Scheme): Appearance {
  const text = scheme === "dark" ? "light" : "dark";
  if (appearance.text === text) return appearance;
  return { ...appearance, text, tint: scheme === "dark" ? "#18181b" : "#f3f3f5" };
}

const NONE_SHADOW = "0 0 0 0 transparent";

function glowCss(glow: GlowStyle | undefined): string {
  if (!glow?.color || !glow.size) return NONE_SHADOW;
  const opacity = Math.round((glow.opacity ?? 0.6) * 100);
  return `0 0 ${glow.size}px color-mix(in srgb, ${cssColor(glow.color)} ${opacity}%, transparent)`;
}

function shadowCss(shadow: ShadowStyle | undefined): string {
  if (!shadow?.color) return NONE_SHADOW;
  const { x = 0, y = 0, blur = 0, spread = 0 } = shadow;
  return `${x}px ${y}px ${blur}px ${spread}px ${cssColor(shadow.color)}`;
}

/** The glass material a look draws: its own, else liquid when the theme's appearance style is. */
export function glassStyleOf(look: Look, appearance: Appearance): GlassStyle {
  return look.glass.style ?? (appearance.style === "liquid" ? "liquid" : "frosted");
}

/** Fills liquid glass can stand in for (a solid, gradient or picture fill stays as it is). */
const LIQUID_FILLS = new Set(["tint", "wallpaper", "blur"]);

/** Glass highlights, the liquid knobs, the bar's type, and its flyouts' tint. */
function materialTokens(look: Look, liquid: boolean): Record<string, string> {
  const { glass } = look;
  const intensity = glass.intensity ?? 1;
  const vars: Record<string, string> = {
    "--g-highlight": String(glass.highlight ?? 0),
    "--g-sheen": String(glass.sheen ?? 0),
    "--g-depth": String(glass.depth ?? (liquid ? 0.2 : 0)),
    "--lq-intensity": String(intensity),
    "--lq-refraction": String(glass.refraction ?? 0.6),
    "--lq-highlight": String(round((glass.highlight ?? 0.75) * intensity)),
    "--lq-blur": String(glass.blur ?? 1),
  };
  if (look.font.family) vars["--bar-font"] = look.font.family;
  if (look.font.size !== undefined) vars["--bar-size"] = `${look.font.size}px`;
  if (look.font.weight !== undefined) vars["--bar-weight"] = String(look.font.weight);
  if (look.flyouts.tint !== undefined) vars["--fly-tint"] = String(look.flyouts.tint);
  return vars;
}

/** Rest, then hover and active each layered over rest. */
function states<T extends object>(per: PerState<T>): { rest: T; hover: T; active: T } {
  const rest = (per.rest ?? {}) as T;
  return { rest, hover: { ...rest, ...per.hover }, active: { ...rest, ...per.active } };
}

/**
 * What draws the surface's background layer (`.surface > .bg`). `live`: live blur draws under
 * the page (Windows does the blur; the page only tints).
 */
function fillTokens(
  look: Look,
  assets: Record<string, string>,
  tint: string,
  opacity: number,
  live: boolean,
): Record<string, string> {
  const tinted = (alpha: number) => {
    const [r, g, b] = rgbOf(tint);
    return `rgb(${r} ${g} ${b} / ${round(alpha)})`;
  };
  const saturation = look.saturation ?? 1;
  const vars: Record<string, string> = {
    "--s-fill": "none",
    "--s-fill-color": "transparent",
    "--s-fill-size": "cover",
    "--s-fill-pos": "center",
    "--s-fill-repeat": "no-repeat",
    "--s-filter": "none",
    "--s-tint": "transparent",
    "--glass-sat": String(saturation),
    "--glass-bright": "1",
    "--glass-blur": "0",
  };
  const background = look.background;
  switch (background?.type) {
    case undefined:
      vars["--s-fill-color"] = "var(--surface)";
      break;
    case "none":
      break;
    case "solid":
      vars["--s-fill-color"] = cssColor(background.color);
      break;
    case "gradient":
      vars["--s-fill"] = gradientCss(background);
      vars["--s-tint"] = tinted(look.tint ?? 0);
      break;
    case "image": {
      const url = assets[background.path];
      if (!url) {
        vars["--s-fill-color"] = "var(--surface)";
        break;
      }
      vars["--s-fill"] = `url("${url}")`;
      vars["--s-fill-size"] =
        background.fit === "fill" ? "100% 100%" : background.fit === "tile" ? "auto" : background.fit;
      vars["--s-fill-repeat"] = background.fit === "tile" ? "repeat" : "no-repeat";
      vars["--s-fill-pos"] = background.position;
      vars["--s-filter"] = `brightness(${background.brightness}) saturate(${saturation}) blur(${background.blur}px)`;
      vars["--s-tint"] = tinted(look.tint ?? 0.25);
      break;
    }
    case "wallpaper":
      // The image and its alignment come with the `backdrop` event (backdropTokens).
      vars["--s-filter"] = `brightness(${background.brightness}) saturate(${saturation}) blur(${background.blur}px)`;
      vars["--s-tint"] = tinted(look.tint ?? opacity);
      vars["--s-fill-color"] = "var(--surface)"; // until the first snapshot arrives
      break;
    case "blur":
      vars["--glass-bright"] = String(background.brightness);
      vars["--glass-blur"] = String(background.blur);
      // Without live blur, the tint alone has to carry it: nearly opaque.
      vars["--s-tint"] = tinted(live ? (look.tint ?? opacity) : Math.max(0.9, look.tint ?? opacity));
      break;
  }
  return vars;
}

function borderTokens(prefix: string, per: PerState<BorderStyle>, fallback: { width: number; color: string }) {
  const vars: Record<string, string> = {};
  const all = states(per);
  for (const [state, border] of Object.entries(all)) {
    const suffix = state === "rest" ? "" : `-${state}`;
    vars[`--${prefix}-bw${suffix}`] = `${border.width ?? fallback.width}px`;
    vars[`--${prefix}-bi${suffix}`] = border.color ? paintImage(border.color) : fallback.color;
  }
  return vars;
}

function itemTokens(items: Look["items"]): Record<string, string> {
  const all = states<ItemStyle>(items);
  const defaults = {
    rest: { bg: "transparent", fg: "var(--t1)" },
    hover: { bg: "var(--fill-2)", fg: "var(--t0)" },
    active: { bg: "var(--fill-3)", fg: "var(--t0)" },
  };
  const vars: Record<string, string> = { "--i-radius": `${items.radius ?? 6}px` };
  for (const [state, item] of Object.entries(all) as [keyof typeof defaults, ItemStyle][]) {
    const suffix = state === "rest" ? "" : `-${state}`;
    const [color, image] = paintLayers(item.background ?? defaults[state].bg);
    vars[`--i-bgc${suffix}`] = color;
    vars[`--i-bgi${suffix}`] = image;
    vars[`--i-fg${suffix}`] = item.color ? cssColor(item.color) : defaults[state].fg;
    vars[`--i-bw${suffix}`] = `${item.border?.width ?? 0}px`;
    vars[`--i-bi${suffix}`] = item.border?.color ? paintImage(item.border.color) : "none";
    vars[`--i-glow${suffix}`] = glowCss(item.glow);
    vars[`--i-scale${suffix}`] = String(item.scale ?? 1);
  }
  return vars;
}

export interface LookTokens {
  attrs: Record<string, string>;
  vars: Record<string, string>;
}

/**
 * Everything the look sets on <html>. `appearance` is already schemed (see `schemed`); `assets`
 * maps image paths to their URLs.
 */
export function lookTokens(
  look: Look,
  appearance: Appearance,
  scheme: Scheme,
  system: SystemLook,
  assets: Record<string, string>,
  live = false,
  liquidOn = false,
): LookTokens {
  const style = glassStyleOf(look, appearance);
  const liquid = liquidOn && style === "liquid";
  const fill = look.background?.type ?? "tint";
  const [base, light1, light2, light3, dark1, dark2, dark3] = system.accent;
  const glow = states(look.glow);
  const shadow = states(look.shadow);
  const vars: Record<string, string> = {
    "--system-accent": base,
    "--system-accent-light-1": light1,
    "--system-accent-light-2": light2,
    "--system-accent-light-3": light3,
    "--system-accent-dark-1": dark1,
    "--system-accent-dark-2": dark2,
    "--system-accent-dark-3": dark3,
    ...fillTokens(look, assets, appearance.tint, appearance.opacity, live),
    ...materialTokens(look, liquid),
    "--s-noise": noiseImage(look.noise ?? 0),
    "--s-radius": `${look.radius ?? appearance.radius}px`,
    ...borderTokens("s", look.border, {
      width: appearance.border ? 1 : 0,
      color: "linear-gradient(var(--line), var(--line))",
    }),
    "--s-glow": glowCss(glow.rest),
    "--s-glow-hover": glowCss(glow.hover),
    "--s-glow-active": glowCss(glow.active),
    "--s-shadow": shadowCss(shadow.rest),
    "--s-shadow-hover": shadowCss(shadow.hover),
    "--s-shadow-active": shadowCss(shadow.active),
    ...itemTokens(look.items),
    "--ind-running": cssColor(look.indicator.running ?? "var(--t2)"),
    "--ind-active": cssColor(look.indicator.active ?? "var(--t0)"),
    "--ind-attention": cssColor(look.indicator.attention ?? "#ffb340"),
  };
  return {
    attrs: {
      "data-scheme": scheme,
      "data-system-scheme": system.scheme,
      "data-fill": look.background?.type ?? "tint",
      "data-corners": look.corners ?? "round",
      "data-family": look.family ?? "mono",
      "data-glyphs": look.glyphs ?? "default",
      // Flyouts get live blur unless the look says no (and live blur is on at all).
      "data-fly-blur": live && look.flyouts.blur !== false ? "on" : "off",
      "data-glass-style": style,
      "data-liquid": liquid && LIQUID_FILLS.has(fill) ? "on" : "off",
      "data-fly-liquid": liquid && look.flyouts.blur !== false ? "on" : "off",
    },
    vars: liquid ? { ...vars, "--s-tint": tintOver(appearance.tint, look.glass.tint ?? 0.12) } : vars,
  };
}

/** Wallpaper glass: the monitor's snapshot, sized to the monitor and offset by the window's place. */
export function backdropTokens(
  backdrop: { url: string; width: number; height: number; x: number; y: number } | null,
): LookTokens {
  if (!backdrop) return { attrs: { "data-backdrop": "off" }, vars: { "--backdrop": "none" } };
  return {
    attrs: { "data-backdrop": "on" },
    vars: {
      "--backdrop": `url("${backdrop.url}")`,
      "--backdrop-size": `${round(backdrop.width, 2)}px ${round(backdrop.height, 2)}px`,
      "--backdrop-pos": `${round(-backdrop.x, 2)}px ${round(-backdrop.y, 2)}px`,
    },
  };
}
