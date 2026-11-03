import { describe, expect, it } from "vitest";
import {
  cellToPx,
  deltaToCells,
  findFreeSpot,
  moveBy,
  overlaps,
  pxToCell,
  pxToSize,
  rectToPx,
  resizeBy,
  resolve,
  sizeLimits,
  sizeToPx,
  type Grid,
  type Item,
  type SizeLimits,
  type Vec2,
} from "./grid";

// Rectangular cells (100×50) and a gap, so x/y mix-ups show up. Pitch is [110, 60].
const grid: Grid = { origin: [20, 30], cell: [100, 50], gap: 10, cols: 6, rows: 4 };
const small: Grid = { origin: [0, 0], cell: [10, 10], gap: 0, cols: 4, rows: 2 };

const item = (instance: string, cell: Vec2, size: Vec2 = [1, 1]): Item => ({ instance, cell, size });
const at = (layout: Item[], instance: string) => layout.find((i) => i.instance === instance);

function resolved(...args: Parameters<typeof resolve>) {
  const r = resolve(...args);
  if (!r.ok) throw new Error("expected the edit to be accepted");
  return r;
}

describe("cells and px", () => {
  it("places a cell at origin + cell × (cell + gap)", () => {
    expect(cellToPx(grid, [0, 0])).toEqual([20, 30]);
    expect(cellToPx(grid, [2, 1])).toEqual([240, 90]);
  });

  it("sizes a widget as w × cell + (w − 1) × gap", () => {
    expect(sizeToPx(grid, [1, 1])).toEqual([100, 50]);
    expect(sizeToPx(grid, [3, 2])).toEqual([320, 110]);
  });

  it("converts back, fractional between cells", () => {
    expect(pxToCell(grid, [240, 90])).toEqual([2, 1]);
    expect(pxToCell(grid, [240 + 55, 90 + 30])).toEqual([2.5, 1.5]);
    expect(pxToSize(grid, [320, 110])).toEqual([3, 2]);
    expect(deltaToCells(grid, [55, -60])).toEqual([0.5, -1]);
  });

  it("gives a widget's px rect", () => {
    expect(rectToPx(grid, { cell: [1, 2], size: [2, 1] })).toEqual({ x: 130, y: 150, width: 210, height: 50 });
  });
});

describe("moveBy", () => {
  const rect = { cell: [2, 1] as Vec2, size: [2, 1] as Vec2 };

  it("snaps to the nearest cell, rounding half up", () => {
    expect(moveBy(grid, rect, [0.49, 0.49])).toEqual([2, 1]);
    expect(moveBy(grid, rect, [0.5, 0.5])).toEqual([3, 2]);
    expect(moveBy(grid, rect, [-0.51, -0.51])).toEqual([1, 0]);
  });

  it("keeps the exact position without snapping", () => {
    expect(moveBy(grid, rect, [0.3, 0.7], false)).toEqual([2.3, 1.7]);
  });

  it("clamps to the grid with the widget's size", () => {
    expect(moveBy(grid, rect, [10, 10])).toEqual([4, 3]); // 6 cols − 2 wide, 4 rows − 1 tall
    expect(moveBy(grid, rect, [-10, -10])).toEqual([0, 0]);
  });
});

describe("resizeBy", () => {
  const rect = { cell: [2, 1] as Vec2, size: [2, 2] as Vec2 };
  const limits: SizeLimits = { min: [1, 1], max: [3, 3] };

  it("grows and shrinks from the right and bottom edges", () => {
    expect(resizeBy(grid, limits, rect, "e", [1, 0])).toEqual({ cell: [2, 1], size: [3, 2] });
    expect(resizeBy(grid, limits, rect, "s", [0, -1])).toEqual({ cell: [2, 1], size: [2, 1] });
  });

  it("moves the cell when dragging the left and top edges", () => {
    expect(resizeBy(grid, limits, rect, "w", [-1, 0])).toEqual({ cell: [1, 1], size: [3, 2] });
    expect(resizeBy(grid, limits, rect, "n", [0, 1])).toEqual({ cell: [2, 2], size: [2, 1] });
  });

  it("handles corners, touching both axes", () => {
    expect(resizeBy(grid, limits, rect, "se", [1, -1])).toEqual({ cell: [2, 1], size: [3, 1] });
    expect(resizeBy(grid, limits, rect, "nw", [-1, -1])).toEqual({ cell: [1, 0], size: [3, 3] });
    expect(resizeBy(grid, limits, rect, "ne", [1, -1])).toEqual({ cell: [2, 0], size: [3, 3] });
    expect(resizeBy(grid, limits, rect, "sw", [-1, 1])).toEqual({ cell: [1, 1], size: [3, 3] });
  });

  it("ignores the other axis for edge handles", () => {
    expect(resizeBy(grid, limits, rect, "e", [1, 5])).toEqual({ cell: [2, 1], size: [3, 2] });
  });

  it("clamps to max on every edge, keeping the far edge fixed", () => {
    expect(resizeBy(grid, limits, rect, "e", [9, 0]).size).toEqual([3, 2]);
    expect(resizeBy(grid, limits, rect, "s", [0, 9]).size).toEqual([2, 3]);
    expect(resizeBy(grid, limits, rect, "w", [-9, 0])).toEqual({ cell: [1, 1], size: [3, 2] });
    expect(resizeBy(grid, limits, rect, "n", [0, -9])).toEqual({ cell: [2, 0], size: [2, 3] });
  });

  it("clamps to min on every edge, keeping the far edge fixed", () => {
    expect(resizeBy(grid, limits, rect, "e", [-9, 0]).size).toEqual([1, 2]);
    expect(resizeBy(grid, limits, rect, "s", [0, -9]).size).toEqual([2, 1]);
    expect(resizeBy(grid, limits, rect, "w", [9, 0])).toEqual({ cell: [3, 1], size: [1, 2] });
    expect(resizeBy(grid, limits, rect, "n", [0, 9])).toEqual({ cell: [2, 2], size: [2, 1] });
  });

  it("clamps to the grid when max is unbounded", () => {
    const free = sizeLimits({ size: { default: [2, 2] } });
    expect(resizeBy(grid, free, rect, "e", [9, 0]).size).toEqual([4, 2]); // 6 cols − cell 2
    expect(resizeBy(grid, free, rect, "w", [-9, 0])).toEqual({ cell: [0, 1], size: [4, 2] });
    expect(resizeBy(grid, free, rect, "s", [0, 9]).size).toEqual([2, 3]); // 4 rows − row 1
    expect(resizeBy(grid, free, rect, "n", [0, -9])).toEqual({ cell: [2, 0], size: [2, 3] });
  });

  it("snaps at half a cell", () => {
    expect(resizeBy(grid, limits, rect, "e", [0.49, 0]).size).toEqual([2, 2]);
    expect(resizeBy(grid, limits, rect, "e", [0.5, 0]).size).toEqual([3, 2]);
  });

  it("pins a non-resizable widget to its default size from every handle", () => {
    const locked = sizeLimits({ size: { default: [2, 2], min: [1, 1], max: [4, 4] }, resizable: false });
    for (const handle of ["n", "ne", "e", "se", "s", "sw", "w", "nw"] as const) {
      expect(resizeBy(grid, locked, rect, handle, [3, -3])).toEqual(rect);
      expect(resizeBy(grid, locked, rect, handle, [-3, 3])).toEqual(rect);
    }
  });

  it("defaults min to 1×1 and max to unbounded", () => {
    expect(sizeLimits({ size: { default: [2, 1] } })).toEqual({ min: [1, 1], max: [Infinity, Infinity] });
  });
});

describe("overlaps", () => {
  it("is true for any shared cell and false for touching edges", () => {
    const a = { cell: [0, 0] as Vec2, size: [2, 2] as Vec2 };
    expect(overlaps(a, { cell: [1, 1], size: [2, 2] })).toBe(true);
    expect(overlaps(a, { cell: [2, 0], size: [1, 2] })).toBe(false);
    expect(overlaps(a, { cell: [0, 2], size: [2, 1] })).toBe(false);
    expect(overlaps(a, a)).toBe(true);
  });
});

describe("findFreeSpot", () => {
  it("scans rows top→bottom and columns left→right", () => {
    expect(findFreeSpot(grid, [], [2, 1])).toEqual([0, 0]);
    expect(findFreeSpot(grid, [item("a", [0, 0], [2, 1])], [2, 1])).toEqual([2, 0]);
    const fullRow = [item("a", [0, 0], [3, 1]), item("b", [3, 0], [3, 1])];
    expect(findFreeSpot(grid, fullRow, [2, 1])).toEqual([0, 1]);
  });

  it("skips spots a big widget can't fit into", () => {
    const items = [item("a", [1, 0]), item("b", [3, 1])];
    expect(findFreeSpot(grid, items, [2, 2])).toEqual([4, 0]);
  });

  it("takes the first free preferred cell", () => {
    const items = [item("a", [0, 0], [2, 1])];
    expect(findFreeSpot(grid, items, [2, 1], [[1, 0], [2, 0]])).toEqual([2, 0]);
  });

  it("returns undefined when there is no room", () => {
    expect(findFreeSpot(small, [item("a", [0, 0], [4, 2])], [1, 1])).toBeUndefined();
    expect(findFreeSpot(grid, [], [7, 1])).toBeUndefined(); // wider than the grid
  });
});

describe("resolve", () => {
  it("accepts a move into free space and reports only what changed", () => {
    const items = [item("a", [0, 0], [2, 1]), item("b", [3, 3])];
    const r = resolved(grid, items, item("a", [1, 2], [2, 1]), "move");
    expect(r.changes).toEqual([item("a", [1, 2], [2, 1])]);
    expect(r.layout).toHaveLength(2);
  });

  it("reports no changes when nothing moved", () => {
    const items = [item("a", [0, 0], [2, 1])];
    expect(resolved(grid, items, item("a", [0, 0], [2, 1]), "move").changes).toEqual([]);
  });

  it("refuses a target outside the grid", () => {
    const items = [item("a", [0, 0], [2, 1])];
    expect(resolve(grid, items, item("a", [5, 0], [2, 1]), "move")).toEqual({ ok: false });
  });

  it("swaps with the single widget it lands on", () => {
    const items = [item("a", [0, 0], [2, 1]), item("b", [2, 0], [2, 1])];
    const r = resolved(grid, items, item("a", [2, 0], [2, 1]), "move");
    expect(at(r.layout, "a")?.cell).toEqual([2, 0]);
    expect(at(r.layout, "b")?.cell).toEqual([0, 0]);
    expect(r.changes).toHaveLength(2);
  });

  it("swaps widgets of different sizes when the other one fits", () => {
    const items = [item("a", [0, 0]), item("b", [1, 0], [2, 1])];
    const r = resolved(grid, items, item("a", [2, 0]), "move");
    expect(at(r.layout, "b")).toEqual(item("b", [0, 0], [2, 1]));
  });

  it("shifts down instead when the swap would overlap the target", () => {
    const items = [item("a", [0, 0]), item("b", [1, 0], [2, 1])];
    const r = resolved(grid, items, item("a", [1, 0]), "move");
    expect(at(r.layout, "a")?.cell).toEqual([1, 0]);
    expect(at(r.layout, "b")?.cell).toEqual([1, 1]);
  });

  it("shifts down instead when the swap spot is taken by a third widget", () => {
    // b is 2 wide: at a's old cell [0,0] it would run into c.
    const items = [item("a", [0, 0]), item("b", [2, 0], [2, 1]), item("c", [1, 0])];
    const r = resolved(grid, items, item("a", [2, 0]), "move");
    expect(at(r.layout, "b")?.cell).toEqual([2, 1]);
    expect(at(r.layout, "c")?.cell).toEqual([1, 0]);
  });

  it("shifts every overlapped widget down, in reading order, onto free cells", () => {
    const items = [item("a", [0, 0], [2, 1]), item("b", [2, 0]), item("c", [3, 0]), item("d", [2, 1])];
    const r = resolved(grid, items, item("a", [2, 0], [2, 1]), "move");
    expect(at(r.layout, "b")?.cell).toEqual([2, 2]); // [2,1] is taken by d
    expect(at(r.layout, "c")?.cell).toEqual([3, 1]);
    expect(at(r.layout, "d")?.cell).toEqual([2, 1]);
    expect(r.changes.map((c) => c.instance).sort()).toEqual(["a", "b", "c"]);
  });

  it("never lets two displaced widgets share a cell", () => {
    const items = [item("a", [0, 0], [2, 1]), item("b", [2, 0]), item("c", [2, 1]), item("d", [3, 0])];
    const { layout } = resolved(grid, items, item("a", [2, 0], [2, 1]), "move");
    for (const x of layout) {
      for (const y of layout) {
        if (x !== y) expect(overlaps(x, y)).toBe(false);
      }
    }
  });

  it("shifts right when the column below is full", () => {
    // 4×2 grid: a grows right into b, which fills its column.
    const items = [item("a", [1, 0]), item("b", [2, 0], [1, 2])];
    const r = resolved(small, items, item("a", [1, 0], [2, 1]), "resize");
    expect(at(r.layout, "b")).toEqual(item("b", [3, 0], [1, 2]));
  });

  it("scans down the right-hand column from the widget's own row", () => {
    // b is on the last row, so it can't go down; it moves right on the same row, not to the top.
    const items = [item("a", [0, 1]), item("b", [1, 1])];
    const r = resolved(small, items, item("a", [0, 1], [2, 1]), "resize");
    expect(at(r.layout, "b")?.cell).toEqual([2, 1]);
  });

  it("does not swap on resize, it shifts", () => {
    const items = [item("a", [0, 0]), item("b", [1, 0])];
    const r = resolved(grid, items, item("a", [0, 0], [2, 1]), "resize");
    expect(at(r.layout, "b")?.cell).toEqual([1, 1]);
  });

  it("refuses when a displaced widget has nowhere to go", () => {
    const items = [item("a", [1, 0]), item("b", [2, 0], [1, 2]), item("c", [3, 0], [1, 2])];
    expect(resolve(small, items, item("a", [1, 0], [2, 1]), "resize")).toEqual({ ok: false });
  });

  it("refuses when one of several displaced widgets has no room", () => {
    const items = [item("a", [0, 0], [2, 1]), item("b", [2, 0], [1, 2]), item("c", [3, 0], [1, 2])];
    expect(resolve(small, items, item("a", [2, 0], [2, 1]), "move")).toEqual({ ok: false });
  });
});
