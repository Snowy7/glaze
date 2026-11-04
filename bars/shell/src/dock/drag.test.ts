import { describe, expect, it } from "vitest";
import {
  distanceOutside,
  dropIndex,
  exceedsThreshold,
  insertIndex,
  resolveDrop,
  slotShifts,
  type DragItem,
} from "./drag";

const item = (path: string, pinned: boolean): DragItem => ({ path, name: path.toUpperCase(), pinned });
// a b c pinned, x y running. Pitch 50, centers at 25, 75, 125 | 185 (divider near 155), 235.
const items = [item("a", true), item("b", true), item("c", true), item("x", false), item("y", false)];
const centers = [25, 75, 125, 185, 235];
const without = (i: number) => centers.filter((_, k) => k !== i);
const BOUNDARY = 155;

describe("threshold and distance", () => {
  it("only starts a drag past the threshold", () => {
    expect(exceedsThreshold(2, 2)).toBe(false);
    expect(exceedsThreshold(4, 4)).toBe(true);
  });

  it("measures how far a value is outside a range", () => {
    expect(distanceOutside(50, 10, 90)).toBe(0);
    expect(distanceOutside(0, 10, 90)).toBe(10);
    expect(distanceOutside(200, 10, 90)).toBe(110);
  });
});

describe("dropIndex", () => {
  it("moves a pinned icon along the pinned group", () => {
    expect(insertIndex([75, 125], 100)).toBe(1);
    expect(dropIndex(items, 0, without(0), 100, BOUNDARY)).toBe(1);
    expect(dropIndex(items, 2, without(2), 10, BOUNDARY)).toBe(0);
  });

  it("keeps a pinned icon inside the pinned group", () => {
    expect(dropIndex(items, 0, without(0), 300, BOUNDARY)).toBe(2);
  });

  it("pins a running icon only when dropped before the divider", () => {
    expect(dropIndex(items, 3, without(3), 60, BOUNDARY)).toBe(1);
    expect(dropIndex(items, 3, without(3), 150, BOUNDARY)).toBe(3);
    expect(dropIndex(items, 3, without(3), 200, BOUNDARY)).toBeNull();
  });
});

describe("slotShifts", () => {
  it("slides the items the dragged one passes", () => {
    expect(slotShifts(5, 0, 2)).toEqual([0, -1, -1, 0, 0]);
    expect(slotShifts(5, 3, 1)).toEqual([0, 1, 1, 0, 0]);
    expect(slotShifts(5, 3, null)).toEqual([0, 0, 0, 0, 0]);
  });
});

describe("resolveDrop", () => {
  it("reorders pinned icons", () => {
    expect(resolveDrop(items, 0, 2, false)).toEqual({ type: "reorder", paths: ["b", "c", "a"] });
    expect(resolveDrop(items, 2, 0, false)).toEqual({ type: "reorder", paths: ["c", "a", "b"] });
  });

  it("does nothing when a pinned icon lands where it was", () => {
    expect(resolveDrop(items, 1, 1, false)).toEqual({ type: "none" });
  });

  it("unpins a pinned icon dropped far from the dock", () => {
    expect(resolveDrop(items, 1, 1, true)).toEqual({ type: "unpin", path: "b" });
  });

  it("pins a running icon at the drop index", () => {
    expect(resolveDrop(items, 3, 1, false)).toEqual({ type: "pin", path: "x", name: "X", index: 1 });
    expect(resolveDrop(items, 3, 3, false)).toEqual({ type: "pin", path: "x", name: "X", index: 3 });
  });

  it("ignores a running icon dropped outside the pinned group, far or not", () => {
    expect(resolveDrop(items, 4, null, false)).toEqual({ type: "none" });
    expect(resolveDrop(items, 4, null, true)).toEqual({ type: "none" });
  });
});
