// Tells the host which parts of the transparent window are visible and take the pointer.

import { normalizeRects, sameRects, type Rect } from "./geometry";

export interface Region {
  /** Adds a source of rects (the bar, open popovers, ...). All sources are unioned. */
  provide(source: () => Rect[]): void;
  /** Call whenever a source's rects may have changed. */
  invalidate(): void;
}

/**
 * Collects rects from every source, normalizes them, and calls `send` only when the result differs
 * from the last one sent. Invalidations coalesce into one animation frame.
 */
export function createRegion(send: (rects: Rect[]) => void): Region {
  const sources: (() => Rect[])[] = [];
  let last: Rect[] | undefined;
  let frame = 0;

  const flush = () => {
    frame = 0;
    const rects = normalizeRects(
      sources.flatMap((source) => source()),
      { w: window.innerWidth, h: window.innerHeight },
    );
    if (last && sameRects(last, rects)) return;
    last = rects;
    send(rects);
  };
  const invalidate = () => {
    if (!frame) frame = requestAnimationFrame(flush);
  };

  window.addEventListener("resize", invalidate);
  return { provide: (source) => void sources.push(source), invalidate };
}
