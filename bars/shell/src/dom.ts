import type { Rect, Size } from "./geometry";

/** `document.createElement` with a class name and children. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = "",
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (className) el.className = className;
  el.append(...children);
  return el;
}

export function button(className: string, ...children: (Node | string)[]): HTMLButtonElement {
  const el = h("button", className, ...children);
  el.type = "button";
  return el;
}

/**
 * A bar surface (the top bar, an island, the dock's pill): its look comes from the `--s-*`
 * variables (style/look.ts). The `.bg` layer holds the fill, and wallpaper glass, under the tint.
 */
export function surface(className: string, ...children: (Node | string)[]): HTMLDivElement {
  return h("div", `surface ${className}`, h("div", "bg"), ...children);
}

/** A glyph box holding trusted inline SVG from icons.ts. */
export function glyph(markup = "", className = ""): HTMLSpanElement {
  const el = h("span", className ? `glyph ${className}` : "glyph");
  el.innerHTML = markup;
  return el;
}

/** The element's box as shown on screen (includes transforms), in window coordinates. */
export function rectOf(el: Element): Rect {
  const r = el.getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}

/** The element's layout box, ignoring transforms and animations (offsetParent is a fixed layer). */
export function layoutBox(el: HTMLElement): Rect {
  return [el.offsetLeft, el.offsetTop, el.offsetWidth, el.offsetHeight];
}

/** The first letter of a name, for icons that don't have an image. */
export function glyphLetter(name: string): string {
  return [...name.trim()][0]?.toUpperCase() ?? "?";
}

export function viewport(): Size {
  return { w: window.innerWidth, h: window.innerHeight };
}

export const prefersReducedMotion = (): boolean => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
