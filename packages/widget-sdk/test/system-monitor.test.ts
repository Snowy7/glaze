import { describe, expect, it } from "vitest";
import {
  HISTORY,
  axisMax,
  bestGrid,
  circumference,
  enabledMetrics,
  fitCount,
  percent,
  percentOf,
  pushSample,
  ringOffset,
  sparkPaths,
} from "../../../widgets/system-monitor/gauge.js";

describe("percentages", () => {
  it("rounds, clamps and treats junk as zero", () => {
    expect(percent(37.6)).toBe(38);
    expect(percent(140)).toBe(100);
    expect(percent(-3)).toBe(0);
    expect(percent(Number.NaN)).toBe(0);
  });

  it("computes used over total, and 0 without a total", () => {
    expect(percentOf(8, 32)).toBe(25);
    expect(percentOf(40, 32)).toBe(100);
    expect(percentOf(5, 0)).toBe(0);
  });
});

describe("history", () => {
  it("keeps only the newest samples", () => {
    const values: number[] = [];
    for (let i = 0; i < HISTORY + 5; i++) pushSample(values, i);
    expect(values).toHaveLength(HISTORY);
    expect(values[0]).toBe(5);
    expect(values.at(-1)).toBe(HISTORY + 4);
  });
});

describe("ring", () => {
  it("leaves the wanted share of the circle drawn", () => {
    const c = circumference(40);
    expect(c).toBeCloseTo(2 * Math.PI * 40);
    expect(ringOffset(c, 0)).toBeCloseTo(c);
    expect(ringOffset(c, 100)).toBeCloseTo(0);
    expect(ringOffset(c, 25)).toBeCloseTo(c * 0.75);
    expect(ringOffset(c, 250)).toBeCloseTo(0);
  });
});

describe("sparkline", () => {
  it("keeps a flat idle series flat by flooring the axis", () => {
    expect(axisMax([1, 2, 1], 10)).toBe(10);
    expect(axisMax([1, 40, 1], 10)).toBe(40);
  });

  it("puts the newest sample at the right edge and a short history against it", () => {
    const { line, area, last } = sparkPaths([0, 50, 100], { points: 5, width: 100, height: 40, max: 100, pad: 2 });
    expect(last).toEqual({ x: 100, y: 2 });
    expect(line.startsWith("M50.00 38.00")).toBe(true);
    expect(area.endsWith("Z")).toBe(true);
    expect(sparkPaths([], { width: 100, height: 40, max: 1 })).toEqual({ line: "", area: "", last: null });
  });

  it("stays inside the box whatever the value", () => {
    const { last } = sparkPaths([500], { points: 2, width: 10, height: 20, max: 100, pad: 3 });
    expect(last?.y).toBe(3);
  });
});

describe("layout", () => {
  it("picks the grid whose cells are closest to the wanted shape, with no ragged row", () => {
    expect(bestGrid(4, 400, 100)).toEqual({ cols: 4, rows: 1 });
    expect(bestGrid(4, 200, 200)).toEqual({ cols: 2, rows: 2 });
    expect(bestGrid(3, 300, 300).cols * bestGrid(3, 300, 300).rows).toBe(3);
  });

  it("limits the tiles to what fits", () => {
    expect(fitCount(96, 96, 4)).toBe(1);
    expect(fitCount(400, 200, 4)).toBe(4);
    expect(fitCount(10, 10, 0)).toBe(1);
  });

  it("lists the switched-on metrics in display order", () => {
    expect(enabledMetrics({ cpu: true, ram: false, gpu: true, net: true })).toEqual(["cpu", "gpu", "net"]);
  });
});
