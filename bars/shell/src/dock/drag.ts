// Pure index math for dragging dock icons. Positions are along the dock's axis, in the items
// container's coordinates; the DOM layer measures them and applies the results.

/** Pointer travel before a press becomes a drag, so plain clicks still work. */
export const DRAG_THRESHOLD = 5;
/** How far (DIPs) off the dock a pinned icon must be dropped to unpin it. */
export const FAR_DISTANCE = 80;

export function exceedsThreshold(dx: number, dy: number, threshold = DRAG_THRESHOLD): boolean {
  return Math.hypot(dx, dy) > threshold;
}

/** How far `v` is outside `[lo, hi]` (0 inside). */
export function distanceOutside(v: number, lo: number, hi: number): number {
  return v < lo ? lo - v : v > hi ? v - hi : 0;
}

/** The number of `centers` before `p`: where an item dropped at `p` would be inserted. */
export function insertIndex(centers: readonly number[], p: number): number {
  return centers.filter((c) => c < p).length;
}

interface Draggable {
  pinned: boolean;
}

/**
 * Where the dragged item would land (its index after the move), or null when it stays put.
 * `others` are the centers of every item except the dragged one. A pinned item stays in the pinned
 * group; a running one joins it only when dropped before `boundary` (the divider).
 */
export function dropIndex(
  items: readonly Draggable[],
  from: number,
  others: readonly number[],
  p: number,
  boundary: number,
): number | null {
  const dragged = items[from];
  if (!dragged) return null;
  const pinnedCount = items.filter((i) => i.pinned).length;
  const n = insertIndex(others, p);
  if (dragged.pinned) return Math.min(n, pinnedCount - 1);
  return p < boundary ? Math.min(n, pinnedCount) : null;
}

/** How many slots each item slides while `from` is dragged to `to` (0 for the dragged item). */
export function slotShifts(length: number, from: number, to: number | null): number[] {
  return Array.from({ length }, (_, i) => {
    if (to === null || i === from) return 0;
    if (from < to && i > from && i <= to) return -1;
    if (from > to && i >= to && i < from) return 1;
    return 0;
  });
}

export type DropAction =
  | { type: "none" }
  | { type: "reorder"; paths: string[] }
  | { type: "pin"; path: string; name: string; index: number }
  | { type: "unpin"; path: string };

export interface DragItem extends Draggable {
  path: string;
  name: string;
}

/**
 * What releasing the dragged item does. `to` is `dropIndex`'s result; `far` is whether the pointer
 * is more than `FAR_DISTANCE` off the dock (only pinned icons can be dragged off to unpin).
 */
export function resolveDrop(items: readonly DragItem[], from: number, to: number | null, far: boolean): DropAction {
  const item = items[from];
  if (!item) return { type: "none" };
  if (item.pinned) {
    if (far) return { type: "unpin", path: item.path };
    if (to === null || to === from) return { type: "none" };
    const paths = items.filter((i) => i.pinned).map((i) => i.path);
    paths.splice(from, 1);
    paths.splice(to, 0, item.path);
    return { type: "reorder", paths };
  }
  return to === null ? { type: "none" } : { type: "pin", path: item.path, name: item.name, index: to };
}
