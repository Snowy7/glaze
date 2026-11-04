// Where a popover opens from a bar item: along the bar from the item itself, across it from the
// whole band (the element marked `data-band`: the top bar's strip or island, the dock's pill), on
// the side toward the middle of the screen (`data-edge` on <html>).

import { rectOf } from "./dom";
import type { Rect, Side } from "./geometry";

export interface BarAnchor {
  rect: Rect;
  side: Side;
  /** CSS transform-origin for a pop-in toward the anchor. */
  origin: string;
}

const EDGES = ["top", "bottom", "left", "right"] as const;
type Edge = (typeof EDGES)[number];

/** Pure: the anchor for an item at `item` in a band at `band` on `edge`. */
export function anchorFor(item: Rect, band: Rect, edge: Edge): BarAnchor {
  const [ix, iy, iw, ih] = item;
  const [bx, by, bw, bh] = band;
  switch (edge) {
    case "top":
      return { rect: [ix, by, iw, bh], side: "below", origin: "50% 0" };
    case "bottom":
      return { rect: [ix, by, iw, bh], side: "above", origin: "50% 100%" };
    case "left":
      return { rect: [bx, iy, bw, ih], side: "right", origin: "0 50%" };
    case "right":
      return { rect: [bx, iy, bw, ih], side: "left", origin: "100% 50%" };
  }
}

export function barAnchor(el: Element): BarAnchor {
  const band = el.closest("[data-band]") ?? el;
  const edge = EDGES.find((e) => e === document.documentElement.dataset.edge) ?? "top";
  return anchorFor(rectOf(el), rectOf(band), edge);
}
