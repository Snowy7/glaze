import { describe, expect, it } from "vitest";
import {
  dateKey,
  levelOf,
  mergeSources,
  monthGrid,
  monthLabels,
  parseKey,
  readCounts,
  scaleMax,
  streaks,
  weekGrid,
} from "../../../widgets/heatmap/heat.js";

// Wednesday 4 November 2026.
const today = new Date(2026, 10, 4);

describe("heatmap counts", () => {
  it("keeps real days with a positive count and drops the rest", () => {
    const raw = {
      "2026-11-03": 4,
      "2026-11-02": 2.9,
      "2026-02-30": 3,
      "11/03/2026": 1,
      "2026-11-01": 0,
      "2026-10-31": -2,
      "2026-10-30": "5",
      "2026-10-29": Number.NaN,
      "2026-10-28": 1e9,
    };
    expect(readCounts(raw)).toEqual({ "2026-11-03": 4, "2026-11-02": 2, "2026-10-28": 9999 });
    for (const bad of [null, undefined, 3, "x", [1, 2]]) expect(readCounts(bad)).toEqual({});
  });

  it("parses only calendar days", () => {
    expect(parseKey("2028-02-29")).toEqual(new Date(2028, 1, 29));
    expect(parseKey("2027-02-29")).toBeNull();
  });

  it("merges by adding the days of both sources, or picks one", () => {
    const todo = { "2026-11-03": 2, "2026-11-02": 1 };
    const timer = { "2026-11-03": 3, "2026-11-04": 1 };
    expect(mergeSources(todo, timer, "all")).toEqual({ "2026-11-03": 5, "2026-11-02": 1, "2026-11-04": 1 });
    expect(mergeSources(todo, timer, "todo")).toBe(todo);
    expect(mergeSources(todo, timer, "timer")).toBe(timer);
  });
});

describe("heatmap levels", () => {
  it("uses the count itself on a small scale", () => {
    expect([0, 1, 2, 3, 4, 7].map((n) => levelOf(n, 3))).toEqual([0, 1, 2, 3, 4, 4]);
  });

  it("spreads evenly over a larger scale, with the top level for anything above it", () => {
    expect([0, 1, 5, 6, 10, 11, 50].map((n) => levelOf(n, 10))).toEqual([0, 1, 2, 3, 4, 4, 4]);
  });

  it("scales to the busy days, not to a single outlier", () => {
    const days = [...Array.from({ length: 20 }, (_, i) => 1 + (i % 4)), 90];
    expect(scaleMax(days)).toBeLessThan(10);
    expect(scaleMax([0, 0])).toBe(1);
  });
});

describe("heatmap grids", () => {
  it("ends the week grid with the week of today and marks the days to come", () => {
    const grid = weekGrid(today, 3, 1);
    expect(grid).toHaveLength(3);
    expect(grid.every((week) => week.length === 7)).toBe(true);
    // Monday first: the last column is Mon 2 Nov .. Sun 8 Nov.
    expect(grid[2]?.[0]?.key).toBe("2026-11-02");
    expect(grid[2]?.[2]?.key).toBe("2026-11-04");
    expect(grid[2]?.map((c) => c.future)).toEqual([false, false, false, true, true, true, true]);
    expect(grid[0]?.[0]?.key).toBe("2026-10-19");
  });

  it("starts weeks on Sunday when asked", () => {
    const grid = weekGrid(today, 2, 0);
    expect(grid[1]?.[0]?.key).toBe("2026-11-01");
    expect(grid[0]?.[0]?.key).toBe("2026-10-25");
  });

  it("names a month where it starts and never crowds two names together", () => {
    const labels = monthLabels(weekGrid(today, 26, 1));
    const names = labels.map((l) => l.month);
    expect(new Set(names).size).toBe(names.length);
    for (let i = 1; i < labels.length; i++) expect((labels[i]?.col ?? 0) - (labels[i - 1]?.col ?? 0)).toBeGreaterThanOrEqual(3);
    // The partial first month is skipped when the next one starts right after it.
    const tight = monthLabels(weekGrid(new Date(2026, 10, 4), 6, 1));
    expect(tight[0]?.col).not.toBe(tight[1]?.col);
  });

  it("lays a month out as calendar weeks padded with empty cells", () => {
    const { year, month, weeks } = monthGrid(today, 1);
    expect([year, month]).toEqual([2026, 10]);
    // 1 November 2026 is a Sunday: six empty cells first on a Monday-first calendar.
    expect(weeks[0]?.slice(0, 6).every((c) => c === null)).toBe(true);
    expect(weeks[0]?.[6]?.key).toBe("2026-11-01");
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    const cells = weeks.flat().filter((c) => c !== null);
    expect(cells).toHaveLength(30);
    expect(cells.filter((c) => c.today).map((c) => c.day)).toEqual([4]);
    expect(cells.filter((c) => c.future)).toHaveLength(26);
  });

  it("formats keys with zero padding", () => {
    expect(dateKey(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("heatmap streaks", () => {
  const days = (...keys: string[]) => Object.fromEntries(keys.map((k) => [k, 1]));

  it("counts the run ending today", () => {
    expect(streaks(days("2026-11-04", "2026-11-03", "2026-11-02", "2026-10-30"), today)).toMatchObject({ current: 3, best: 3 });
  });

  it("keeps the streak alive while today is still empty, and loses it after a missed day", () => {
    expect(streaks(days("2026-11-03", "2026-11-02"), today).current).toBe(2);
    expect(streaks(days("2026-11-02", "2026-11-01"), today).current).toBe(0);
  });

  it("finds the best run anywhere, across month and year ends", () => {
    const counts = days("2025-12-30", "2025-12-31", "2026-01-01", "2026-01-02", "2026-11-04");
    expect(streaks(counts, today)).toMatchObject({ current: 1, best: 4 });
  });

  it("ignores days after today and totals the rest", () => {
    const counts = { "2026-11-04": 2, "2026-11-05": 9, "2026-11-03": 3 };
    expect(streaks(counts, today)).toEqual({ current: 2, best: 2, total: 5 });
    expect(streaks({}, today)).toEqual({ current: 0, best: 0, total: 0 });
  });
});
