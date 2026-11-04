// The top bar: `config.top.height` DIPs at the top (or bottom) of its window, inside a band of
// `height + 2 * margin`; the rest of the window is for popovers. Three layouts (docs/bars.md,
// "Layouts"): `strip` (edge to edge), `floating` (detached, rounded, `width` full/auto/DIPs/%),
// `islands` (a transparent rail with one surface per group; `|` splits groups).

import { bindActions } from "../actions";
import type { Bar } from "../bar";
import { h, surface } from "../dom";
import { createScope, type Scope } from "../host";
import { closeMenu } from "../menu";
import { layerRects } from "../overlay";
import type { BarKind, BarsConfig, ModuleName } from "../protocol";
import { buildModule, groupsOf } from "./build";

export function createTop(root: HTMLElement, config: BarsConfig): Bar {
  const scope = createScope();
  const { layout, height, margin, padding, spacing, width, position } = config.top;
  const islands = layout === "islands";
  const strip = islands ? h("div", "top") : surface("top");
  strip.dataset.band = "";
  strip.dataset.width = typeof width === "number" ? "dips" : width.endsWith("%") ? "percent" : width;
  strip.style.setProperty("--bar-h", `${height}px`);
  strip.style.setProperty("--bar-margin", `${margin}px`);
  strip.style.setProperty("--bar-pad", `${padding}px`);
  strip.style.setProperty("--bar-gap", `${spacing}px`);
  if (typeof width === "number") strip.style.setProperty("--bar-w", `${width}px`);
  else if (width.endsWith("%")) strip.style.setProperty("--bar-w", width);
  document.documentElement.setAttribute("data-position", position);

  const seen = new Set<string>();
  for (const zone of ["left", "center", "right"] as const) {
    const el = h("div", `zone ${zone}`);
    el.dataset.zone = zone;
    for (const group of groupsOf(config.top[zone], seen)) {
      const modules = group.map((name) => mount(name, config, scope));
      if (islands) {
        const island = surface("island", ...modules);
        island.dataset.band = "";
        el.append(island);
      } else {
        if (el.childElementCount > 0) el.append(h("span", "sep"));
        el.append(...modules);
      }
    }
    strip.append(el);
  }
  root.append(strip);

  return {
    // The whole band takes the pointer: it's reserved, so nothing else is under it.
    rects: () => {
      const band = height + 2 * margin;
      const y = position === "bottom" ? window.innerHeight - band : 0;
      return [[0, y, window.innerWidth, band], ...layerRects()];
    },
    destroy: () => {
      closeMenu();
      scope.dispose();
      strip.remove();
    },
  };
}

/** Builds a module for the `kind` bar and binds the actions bars.json gives it. */
export function mount(name: ModuleName, config: BarsConfig, scope: Scope, kind: BarKind = "top"): HTMLElement {
  const el = buildModule(name, config, scope, kind);
  scope.add(bindActions(el, name, config.modules[name]));
  return el;
}
