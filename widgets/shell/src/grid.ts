/*
 * Grid math for the widget shell. Pure: no DOM, no state. Units are grid cells unless a name says px.
 *
 * Geometry (docs/widgets.md, "Layout"): cell [c, r] sits at `origin + [c, r] × (cell + gap)`; a widget
 * `[w, h]` cells large is `w × cell + (w − 1) × gap` wide. Cells may be rectangular (`cell: [cw, ch]`).
 *
 * THE NO-OVERLAP RULE
 * Widgets never overlap and never leave the grid. When a move or resize would overlap other
 * widgets, `resolve()` applies, in order:
 *   1. Swap (moves only). If the target overlaps exactly one widget B, and B fits at the moved
 *      widget's ORIGINAL cell (inside the grid, overlapping neither the target nor any other
 *      widget), B goes there. The two trade places.
 *   2. Shift. Otherwise every overlapped widget is displaced, in reading order (top→bottom, then
 *      left→right). Each goes to the nearest free spot: first in its own column below its current
 *      row (scanning down), else in the columns to its right (per column, scanning down from its
 *      current row; columns left→right). A spot is free if it is inside the grid and overlaps
 *      neither the moved widget's target, nor a widget that stays, nor a widget displaced earlier.
 *      Widgets that don't overlap the target never move, so displacement doesn't cascade.
 *   3. Refuse. If any displaced widget finds no spot, the whole move/resize is refused: `resolve()`
 *      returns `{ ok: false }` and the caller shakes the widget and reverts it.
 * On success `resolve()` returns the full new layout and the changed instances, so the shell can send
 * one `layout` message.
 *
 * Snapping rounds to the nearest cell; exactly half a cell rounds up (right/down). Moves clamp to the
 * grid; sizes clamp to the manifest `min`/`max` (`resizable: false` pins them to `default`) and to the
 * grid edge.
 */

import type { Vec2, WidgetManifest } from "./protocol";

export type { Vec2 };

export interface Grid {
  origin: Vec2; // px of cell [0, 0]'s top-left, relative to the monitor
  cell: Vec2; // px size of one cell
  gap: number; // px between cells
  cols: number;
  rows: number;
}

/** A cell position and a size in cells. */
export interface Rect {
  cell: Vec2;
  size: Vec2;
}

export interface Item extends Rect {
  instance: string;
}

export interface PxRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Size bounds in cells. `max` entries are `Infinity` when the manifest has no `size.max`. */
export interface SizeLimits {
  min: Vec2;
  max: Vec2;
}

export type Handle = "n" | "ne" | "e" | "se" | "s" | "sw" | "w" | "nw";

export type Resolution = { ok: true; layout: Item[]; changes: Item[] } | { ok: false };

// ---- cells ⇄ px ----

/** px distance between the starts of neighbouring cells. */
export function pitch(grid: Grid): Vec2 {
  return [grid.cell[0] + grid.gap, grid.cell[1] + grid.gap];
}

/** Top-left px of a cell. Fractional cells give the matching fractional position. */
export function cellToPx(grid: Grid, cell: Vec2): Vec2 {
  const p = pitch(grid);
  return [grid.origin[0] + cell[0] * p[0], grid.origin[1] + cell[1] * p[1]];
}

/** Inverse of `cellToPx`: a fractional cell (integers are cell top-left corners). */
export function pxToCell(grid: Grid, px: Vec2): Vec2 {
  const p = pitch(grid);
  return [(px[0] - grid.origin[0]) / p[0], (px[1] - grid.origin[1]) / p[1]];
}

/** px extent of `size` cells, including the gaps between them. */
export function sizeToPx(grid: Grid, size: Vec2): Vec2 {
  return [size[0] * grid.cell[0] + (size[0] - 1) * grid.gap, size[1] * grid.cell[1] + (size[1] - 1) * grid.gap];
}

/** Inverse of `sizeToPx`: fractional cells covered by a px extent. */
export function pxToSize(grid: Grid, px: Vec2): Vec2 {
  const p = pitch(grid);
  return [(px[0] + grid.gap) / p[0], (px[1] + grid.gap) / p[1]];
}

export function rectToPx(grid: Grid, rect: Rect): PxRect {
  const [x, y] = cellToPx(grid, rect.cell);
  const [width, height] = sizeToPx(grid, rect.size);
  return { x, y, width, height };
}

/** A pointer delta in px as a (fractional) delta in cells. */
export function deltaToCells(grid: Grid, delta: Vec2): Vec2 {
  const p = pitch(grid);
  return [delta[0] / p[0], delta[1] / p[1]];
}

// ---- the edit-mode toolbar ----

/**
 * Which edge of the work area the toolbar sits on, centred: the top, unless a widget is under it
 * there and the bottom is clear. (When both are blocked it stays on top: it is drawn over widgets.)
 * `bar` is the toolbar's measured size; the widget rects are in px.
 */
export function toolbarEdge(work: PxRect, bar: { width: number; height: number }, widgets: readonly PxRect[], inset = 10): "top" | "bottom" {
  const x = work.x + work.width / 2 - bar.width / 2;
  const blocked = (y: number) =>
    widgets.some((r) => r.x < x + bar.width && x < r.x + r.width && r.y < y + bar.height && y < r.y + r.height);
  const top = work.y + inset;
  const bottom = work.y + work.height - inset - bar.height;
  return blocked(top) && !blocked(bottom) ? "bottom" : "top";
}

// ---- size limits ----

export function sizeLimits(manifest: Pick<WidgetManifest, "size" | "resizable">): SizeLimits {
  const { default: def, min, max } = manifest.size;
  if (manifest.resizable === false) return { min: def, max: def };
  return { min: min ?? [1, 1], max: max ?? [Infinity, Infinity] };
}

// ---- moving and resizing one widget (no neighbours considered) ----

function clamp(value: number, lo: number, hi: number): number {
  return Math.min(Math.max(value, lo), hi);
}

function moveAxis(pos: number, size: number, count: number, snap: boolean): number {
  return clamp(snap ? Math.round(pos) : pos, 0, Math.max(0, count - size));
}

/**
 * Where `rect` ends up when moved by `delta` cells: snapped (or exact with `snap: false`, for live
 * pointer feedback) and clamped to the grid.
 */
export function moveBy(grid: Grid, rect: Rect, delta: Vec2, snap = true): Vec2 {
  return [
    moveAxis(rect.cell[0] + delta[0], rect.size[0], grid.cols, snap),
    moveAxis(rect.cell[1] + delta[1], rect.size[1], grid.rows, snap),
  ];
}

function resizeAxis(
  pos: number,
  size: number,
  edge: "start" | "end" | undefined,
  delta: number,
  count: number,
  min: number,
  max: number,
  snap: boolean,
): [pos: number, size: number] {
  const round = (v: number) => (snap ? Math.round(v) : v);
  const lo = Math.min(min, count);
  if (edge === "end") {
    return [pos, clamp(round(size + delta), lo, Math.min(max, count - pos))];
  }
  if (edge === "start") {
    // The far edge stays put; the near edge (and so the cell) moves.
    const end = pos + size;
    const next = clamp(round(pos + delta), Math.max(0, end - max), Math.max(0, end - lo));
    return [next, end - next];
  }
  return [pos, size];
}

/**
 * `rect` after dragging `handle` by `delta` cells: snapped (or exact with `snap: false`), clamped
 * to the grid and to `limits`. Left and top edges move the cell, so the opposite edge stays fixed.
 */
export function resizeBy(grid: Grid, limits: SizeLimits, rect: Rect, handle: Handle, delta: Vec2, snap = true): Rect {
  const [x, w] = resizeAxis(
    rect.cell[0], rect.size[0],
    handle.includes("e") ? "end" : handle.includes("w") ? "start" : undefined,
    delta[0], grid.cols, limits.min[0], limits.max[0], snap,
  );
  const [y, h] = resizeAxis(
    rect.cell[1], rect.size[1],
    handle.includes("s") ? "end" : handle.includes("n") ? "start" : undefined,
    delta[1], grid.rows, limits.min[1], limits.max[1], snap,
  );
  return { cell: [x, y], size: [w, h] };
}

// ---- overlap and free space ----

export function overlaps(a: Rect, b: Rect): boolean {
  return (
    a.cell[0] < b.cell[0] + b.size[0] &&
    b.cell[0] < a.cell[0] + a.size[0] &&
    a.cell[1] < b.cell[1] + b.size[1] &&
    b.cell[1] < a.cell[1] + a.size[1]
  );
}

export function inGrid(grid: Grid, rect: Rect): boolean {
  return (
    rect.cell[0] >= 0 &&
    rect.cell[1] >= 0 &&
    rect.size[0] >= 1 &&
    rect.size[1] >= 1 &&
    rect.cell[0] + rect.size[0] <= grid.cols &&
    rect.cell[1] + rect.size[1] <= grid.rows
  );
}

/** True if `rect` is inside the grid and overlaps none of `others`. */
export function isFree(grid: Grid, others: readonly Rect[], rect: Rect): boolean {
  return inGrid(grid, rect) && others.every((o) => !overlaps(o, rect));
}

/**
 * Cell for a new widget of `size`: the first of `prefer` that is free, else the first free spot
 * scanning rows top→bottom and columns left→right. `undefined` if the grid has no room.
 */
export function findFreeSpot(grid: Grid, items: readonly Rect[], size: Vec2, prefer: readonly Vec2[] = []): Vec2 | undefined {
  for (const cell of prefer) {
    if (isFree(grid, items, { cell, size })) return cell;
  }
  for (let row = 0; row + size[1] <= grid.rows; row++) {
    for (let col = 0; col + size[0] <= grid.cols; col++) {
      if (isFree(grid, items, { cell: [col, row], size })) return [col, row];
    }
  }
  return undefined;
}

// ---- resolving overlaps (see the rule at the top of this file) ----

const readingOrder = (a: Rect, b: Rect) => a.cell[1] - b.cell[1] || a.cell[0] - b.cell[0];

/** Nearest free spot for a displaced widget: down its own column, then each column to the right. */
function nearestSpot(grid: Grid, obstacles: readonly Rect[], item: Item): Vec2 | undefined {
  const [w, h] = item.size;
  for (let col = item.cell[0]; col + w <= grid.cols; col++) {
    const firstRow = col === item.cell[0] ? item.cell[1] + 1 : item.cell[1];
    for (let row = firstRow; row + h <= grid.rows; row++) {
      if (isFree(grid, obstacles, { cell: [col, row], size: item.size })) return [col, row];
    }
  }
  return undefined;
}

/**
 * Settles `target` (a widget's new cell and/or size) against the other widgets in `items`
 * (the current layout, including the widget's own old position). `mode: "move"` allows swaps.
 */
export function resolve(grid: Grid, items: readonly Item[], target: Item, mode: "move" | "resize"): Resolution {
  const original = items.find((i) => i.instance === target.instance);
  if (!original || !inGrid(grid, target)) return { ok: false };

  const others = items.filter((i) => i.instance !== target.instance);
  const hit = others.filter((o) => overlaps(o, target));
  const moved = new Map<string, Rect>();

  if (hit.length > 0) {
    const [only] = hit;
    const swapSpot: Rect | undefined =
      mode === "move" && hit.length === 1 && only ? { cell: original.cell, size: only.size } : undefined;
    if (only && swapSpot && !overlaps(swapSpot, target) && isFree(grid, others.filter((o) => o !== only), swapSpot)) {
      moved.set(only.instance, swapSpot);
    } else {
      const obstacles: Rect[] = [target, ...others.filter((o) => !hit.includes(o))];
      for (const item of [...hit].sort(readingOrder)) {
        const cell = nearestSpot(grid, obstacles, item);
        if (!cell) return { ok: false };
        const spot = { cell, size: item.size };
        obstacles.push(spot);
        moved.set(item.instance, spot);
      }
    }
  }

  const layout = items.map((i): Item => {
    if (i.instance === target.instance) return target;
    const spot = moved.get(i.instance);
    return spot ? { instance: i.instance, ...spot } : i;
  });
  const changes = layout.filter((n) => {
    const old = items.find((i) => i.instance === n.instance);
    return !old || old.cell[0] !== n.cell[0] || old.cell[1] !== n.cell[1] || old.size[0] !== n.size[0] || old.size[1] !== n.size[1];
  });
  return { ok: true, layout, changes };
}
