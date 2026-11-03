import type { Rect } from "./geometry";

/** A rendered bar (top or dock), as `main` drives it. */
export interface Bar {
  /** The parts of the window that are visible and take the pointer (see region.ts). */
  rects(): Rect[];
  /** Removes the DOM and every subscription, before the bar is rebuilt or the page goes away. */
  destroy(): void;
}

export interface BarContext {
  /** Call when `rects()` may have changed. */
  invalidateRegion(): void;
}
