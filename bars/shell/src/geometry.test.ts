import { describe, expect, it } from "vitest";
import { hoverExtent, normalizeRects, placePopover, sameRects, type Rect } from "./geometry";

describe("normalizeRects", () => {
  it("rounds outward, drops empty and non-finite rects, dedupes and sorts", () => {
    const rects: Rect[] = [
      [10.4, 20.2, 5.2, 5.2],
      [10.4, 20.2, 5.2, 5.2],
      [0, 0, 0, 10],
      [Number.NaN, 0, 5, 5],
      [0, 0, 100, 10],
    ];
    expect(normalizeRects(rects)).toEqual([
      [0, 0, 100, 10],
      [10, 20, 6, 6],
    ]);
  });

  it("clips to the window", () => {
    expect(normalizeRects([[-5, -5, 30, 30], [90, 90, 50, 50], [200, 0, 10, 10]], { w: 100, h: 100 })).toEqual([
      [0, 0, 25, 25],
      [90, 90, 10, 10],
    ]);
  });

  it("makes equal regions compare equal regardless of order", () => {
    const a = normalizeRects([[0, 0, 5, 5], [10, 0, 5, 5]]);
    const b = normalizeRects([[10, 0, 5, 5], [0, 0, 5, 5]]);
    expect(sameRects(a, b)).toBe(true);
    expect(sameRects(a, [])).toBe(false);
  });
});

describe("placePopover", () => {
  const bounds = { w: 400, h: 300 };
  const size = { w: 100, h: 50 };

  it("opens below the anchor, aligned to its end", () => {
    expect(placePopover([300, 0, 40, 28], size, bounds, "below", "end")).toEqual({ x: 240, y: 34 });
  });

  it("clamps into the window", () => {
    expect(placePopover([380, 0, 40, 28], size, bounds, "below", "end").x).toBe(292);
    expect(placePopover([0, 0, 20, 28], size, bounds, "below", "center").x).toBe(8);
  });

  it("opens above and beside the anchor", () => {
    expect(placePopover([150, 250, 40, 40], size, bounds, "above", "center", 8)).toEqual({ x: 120, y: 192 });
    expect(placePopover([10, 100, 58, 40], size, bounds, "right", "center", 10)).toEqual({ x: 78, y: 95 });
  });
});

describe("hoverExtent", () => {
  it("grows toward the screen center", () => {
    const pill: Rect = [100, 300, 200, 58];
    expect(hoverExtent(pill, "bottom", 60, 40)).toEqual([60, 240, 280, 118]);
    expect(hoverExtent([6, 100, 58, 200], "left", 60, 40)).toEqual([6, 60, 118, 280]);
    expect(hoverExtent([300, 100, 58, 200], "right", 60, 40)).toEqual([240, 60, 118, 280]);
  });
});
