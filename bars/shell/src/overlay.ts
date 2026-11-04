// The layer for popovers, menus and HUDs. Every mounted layer is part of the window region, so
// whatever is visible here also takes the pointer.

import { inflate, type Point, type Rect, type Size } from "./geometry";
import { layoutBox } from "./dom";

/** Room around a layer for its shadow, so the region doesn't clip it. */
const SHADOW_MARGIN = 64;

const layers = new Set<HTMLElement>();
let nextLayer = 0;
let host: HTMLElement | undefined;
let onChange: () => void = () => {};

export function initOverlay(el: HTMLElement, change: () => void): void {
  host = el;
  onChange = change;
}

export interface Layer {
  /** Re-measure and re-place after the content changed. */
  reposition(): void;
  remove(): void;
}

/** Adds `el` to the overlay at the position `place` picks for its measured size. */
export function mountLayer(el: HTMLElement, place: (size: Size) => Point): Layer {
  if (!host) throw new Error("overlay not initialised");
  el.classList.add("layer");
  host.append(el);
  // A child/replacement menu must be above a parent still playing its exit animation.
  el.style.zIndex = String(++nextLayer);
  layers.add(el);
  const reposition = () => {
    const { x, y } = place({ w: el.offsetWidth, h: el.offsetHeight });
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
    onChange();
  };
  reposition();
  return {
    reposition,
    remove: () => {
      el.remove();
      layers.delete(el);
      onChange();
    },
  };
}

/** The mounted layers' boxes (with shadow room), for the window region. */
export function layerRects(): Rect[] {
  return [...layers].map((el) => inflate(layoutBox(el), SHADOW_MARGIN));
}
