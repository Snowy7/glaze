// Puts a bar's whole look on <html>: the appearance tokens, the bar's own look (surfaces, items,
// states, glass, type), motion, wallpaper glass and live blur, Windows' accent, the theme's
// variables, generated CSS for the glyph set, the icon pack and per-module overrides, then the
// theme's CSS last (docs/bars.md, "Styling"). Runs again whenever one of its inputs changes;
// variables it set before and no longer sets are removed.

import { appearanceTokens } from "../appearance";
import type { Backdrop, BarKind, BarsConfig, Look, LookPayload, Mode, Motion } from "../protocol";
import { glyphSetCss } from "./glyph-sets";
import { packGlyphCss } from "./icon-pack";
import { backdropTokens, lookTokens, schemed, schemeOf } from "./look";
import { moduleCss } from "./modules";
import { motionTokens } from "./motion";

export interface PageLook {
  kind: BarKind;
  config: BarsConfig;
  look: LookPayload;
  mode: Mode;
  backdrop: Backdrop | null;
}

let previous = new Set<string>();

/** The bar's edge: where its band sits on the screen. */
export function edgeOf(kind: BarKind, config: BarsConfig): "top" | "bottom" | "left" | "right" {
  return kind === "top" ? config.top.position : config.dock.position;
}

/** The look and motion a bar draws with (already layered over its design's by the host). */
export function barStyle(kind: BarKind, config: BarsConfig): { look: Look; animation: Motion; design: string } {
  const bar = kind === "top" ? config.top : config.dock;
  return { look: bar.look, animation: bar.animation, design: bar.design };
}

/**
 * The look as drawn: an auto-hiding dock slides in over windows, so wallpaper glass (which only
 * knows the wallpaper) becomes live blur there whenever live blur is on.
 */
export function effectiveLook(kind: BarKind, config: BarsConfig, mode: Mode): Look {
  const { look } = barStyle(kind, config);
  const overWindows = kind === "dock" && config.dock.autohide;
  if (overWindows && mode.blur && look.background?.type === "wallpaper") {
    const { brightness, blur } = look.background;
    return { ...look, background: { type: "blur", brightness, blur } };
  }
  return look;
}

/** Every attribute and variable of a bar's look. Pure, so it can be tested. */
export function pageTokens({ kind, config, look, mode, backdrop }: PageLook): {
  attrs: Record<string, string>;
  vars: Record<string, string>;
} {
  const style = barStyle(kind, config);
  const barLook = effectiveLook(kind, config, mode);
  const scheme = schemeOf(barLook, look.appearance, look.system);
  const appearance = schemed(look.appearance, scheme);
  const base = appearanceTokens(appearance);
  const live = mode.blur && !mode.performance;
  const surface = lookTokens(barLook, appearance, scheme, look.system, look.assets, live);
  const motion = motionTokens(style.animation, kind, mode.reduceMotion || mode.performance);
  const glass = backdropTokens(backdrop);
  const attrs: Record<string, string> = {
    ...base.attrs,
    ...surface.attrs,
    ...motion.attrs,
    ...glass.attrs,
    "data-bar": kind,
    "data-preset": config.preset,
    "data-design": style.design,
    "data-edge": edgeOf(kind, config),
    "data-perf": mode.performance ? "on" : "off",
    "data-live": live ? "on" : "off",
  };
  if (kind === "top") attrs["data-layout"] = config.top.layout;
  else {
    attrs["data-style"] = config.dock.style;
    attrs["data-indicator"] = config.dock.indicator;
    attrs["data-attention"] = config.dock.attention;
  }
  return { attrs, vars: { ...base.vars, ...surface.vars, ...motion.vars, ...glass.vars, ...look.vars } };
}

function setStyle(id: string, css: string | null): void {
  const existing = document.getElementById(id);
  if (!css) return void existing?.remove();
  if (existing?.textContent === css) return;
  existing?.remove();
  const style = document.createElement("style");
  style.id = id;
  style.textContent = css;
  document.head.append(style);
}

export function applyLook(state: PageLook): void {
  const root = document.documentElement;
  const { attrs, vars } = pageTokens(state);
  for (const [name, value] of Object.entries(attrs)) root.setAttribute(name, value);
  if (state.kind === "top") root.removeAttribute("data-style");
  const next = new Set(Object.keys(vars));
  for (const name of previous) if (!next.has(name)) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value);
  previous = next;

  const { iconPack, assets, themeCss } = state.look;
  const covered = new Set(iconPack?.entries.flatMap((e) => (e.key ? [e.key] : [])) ?? []);
  const glyphs = barStyle(state.kind, state.config).look.glyphs ?? "default";
  const generated = [
    glyphSetCss(glyphs, covered),
    iconPack ? packGlyphCss(iconPack) : "",
    moduleCss(state.config.modules, iconPack, assets),
  ]
    .filter(Boolean)
    .join("\n");
  setStyle("look-css", generated || null);
  setStyle("theme-css", themeCss);
  // The theme's CSS goes last, so it can restyle everything above.
  const theme = document.getElementById("theme-css");
  if (theme?.nextElementSibling) document.head.append(theme);
}
