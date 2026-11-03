// Pure rectangle and placement math shared by both bars (no DOM).

import type { DockPosition, MethodMap } from "./protocol";

/** `[x, y, width, height]` in DIPs from the window's top-left: the shape `region` takes. */
export type Rect = MethodMap["region"]["params"]["rects"][number];
export interface Size {
  w: number;
  h: number;
}
export interface Point {
  x: number;
  y: number;
}

export function clamp(value: number, lo: number, hi: number): number {
  return Math.min(Math.max(value, lo), hi);
}

export function inflate([x, y, w, h]: Rect, by: number): Rect {
  return [x - by, y - by, w + 2 * by, h + 2 * by];
}

/**
 * Cleans a region before it is sent: rounds outward to whole DIPs, clips to `bounds`, drops empty
 * and non-finite rects, removes duplicates and sorts, so equal regions compare equal.
 */
export function normalizeRects(rects: readonly Rect[], bounds?: Size): Rect[] {
  const seen = new Set<string>();
  const out: Rect[] = [];
  for (const [x, y, w, h] of rects) {
    if (![x, y, w, h].every(Number.isFinite)) continue;
    let x0 = Math.floor(x);
    let y0 = Math.floor(y);
    let x1 = Math.ceil(x + w);
    let y1 = Math.ceil(y + h);
    if (bounds) {
      x0 = Math.max(0, x0);
      y0 = Math.max(0, y0);
      x1 = Math.min(bounds.w, x1);
      y1 = Math.min(bounds.h, y1);
    }
    if (x1 <= x0 || y1 <= y0) continue;
    const key = `${x0},${y0},${x1},${y1}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push([x0, y0, x1 - x0, y1 - y0]);
  }
  return out.sort((a, b) => a[1] - b[1] || a[0] - b[0] || a[2] - b[2] || a[3] - b[3]);
}

export function sameRects(a: readonly Rect[], b: readonly Rect[]): boolean {
  return a.length === b.length && a.every((r, i) => r.every((v, k) => v === b[i]?.[k]));
}

/** Which side of its anchor a popover opens on. */
export type Side = "below" | "above" | "left" | "right";
export type Align = "start" | "center" | "end";

/**
 * Top-left of a popover of `size` next to `anchor`, kept `margin` inside `bounds`. `align` is along
 * the anchor's edge: for "below"/"above" horizontally, for "left"/"right" vertically.
 */
export function placePopover(
  anchor: Rect,
  size: Size,
  bounds: Size,
  side: Side,
  align: Align,
  gap = 6,
  margin = 8,
): Point {
  const [ax, ay, aw, ah] = anchor;
  const pick = (start: number, length: number, own: number) =>
    align === "start" ? start : align === "end" ? start + length - own : start + (length - own) / 2;
  const vertical = side === "below" || side === "above";
  const x = vertical ? pick(ax, aw, size.w) : side === "right" ? ax + aw + gap : ax - size.w - gap;
  const y = vertical ? (side === "below" ? ay + ah + gap : ay - size.h - gap) : pick(ay, ah, size.h);
  return {
    x: Math.round(clamp(x, margin, Math.max(margin, bounds.w - size.w - margin))),
    y: Math.round(clamp(y, margin, Math.max(margin, bounds.h - size.h - margin))),
  };
}

/**
 * The dock's pointer-taking extent while hovered: the pill grown toward the screen center by
 * `headroom` (magnified icons plus the name label) and along the dock by `spread` on both ends.
 */
export function hoverExtent(pill: Rect, position: DockPosition, headroom: number, spread: number): Rect {
  const [x, y, w, h] = pill;
  switch (position) {
    case "bottom":
      return [x - spread, y - headroom, w + 2 * spread, h + headroom];
    case "left":
      return [x, y - spread, w + headroom, h + 2 * spread];
    case "right":
      return [x - headroom, y - spread, w + headroom, h + 2 * spread];
  }
}
