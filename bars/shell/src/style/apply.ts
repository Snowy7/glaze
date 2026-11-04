// Puts a bar's whole look on <html>: the appearance tokens, the look (surfaces, items, states),
// motion, wallpaper glass, Windows' accent, the theme's variables, generated CSS for the icon pack
// and per-module overrides, then the theme's CSS last (docs/bars.md, "Styling"). Runs again
// whenever one of its inputs changes; variables it set before and no longer sets are removed.

import { appearanceTokens } from "../appearance";
import type { Backdrop, BarKind, BarsConfig, LookPayload, Mode } from "../protocol";
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

/** Every attribute and variable of a bar's look. Pure, so it can be tested. */
export function pageTokens({ kind, config, look, mode, backdrop }: PageLook): {
  attrs: Record<string, string>;
  vars: Record<string, string>;
} {
  const scheme = schemeOf(config.look, look.appearance, look.system);
  const appearance = schemed(look.appearance, scheme);
  const base = appearanceTokens(appearance);
  const surface = lookTokens(config.look, appearance, scheme, look.system, look.assets);
  const motion = motionTokens(config.animation, kind, mode.reduceMotion || mode.performance);
  const glass = backdropTokens(backdrop);
  const attrs: Record<string, string> = {
    ...base.attrs,
    ...surface.attrs,
    ...motion.attrs,
    ...glass.attrs,
    "data-bar": kind,
    "data-preset": config.preset,
    "data-edge": edgeOf(kind, config),
    "data-perf": mode.performance ? "on" : "off",
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
  const generated = [iconPack ? packGlyphCss(iconPack) : "", moduleCss(state.config.modules, iconPack, assets)]
    .filter(Boolean)
    .join("\n");
  setStyle("look-css", generated || null);
  setStyle("theme-css", themeCss);
  // The theme's CSS goes last, so it can restyle everything above.
  const theme = document.getElementById("theme-css");
  if (theme?.nextElementSibling) document.head.append(theme);
}
