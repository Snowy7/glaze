import { describe, expect, it } from "vitest";
import { falloff, magnify } from "./magnify";

const opts = { max: 1.5, radius: 150, size: 48 };
const centers = [0, 1, 2, 3, 4, 5, 6].map((i) => 100 + i * 54);

describe("falloff", () => {
  it("is 1 at the pointer, 0 at the radius, and symmetric", () => {
    expect(falloff(0, 150)).toBe(1);
    expect(falloff(150, 150)).toBe(0);
    expect(falloff(-200, 150)).toBe(0);
    expect(falloff(-60, 150)).toBeCloseTo(falloff(60, 150));
  });
});

describe("magnify", () => {
  it("scales the icon under the pointer the most and falls off smoothly", () => {
    const { scales } = magnify(centers, centers[3] ?? 0, opts);
    expect(scales[3]).toBeCloseTo(1.5);
    expect(scales[2]).toBeLessThan(1.5);
    expect(scales[2]).toBeGreaterThan(scales[1] ?? 0);
    expect(scales[0]).toBe(1);
    expect(scales[6]).toBe(1);
  });

  it("keeps the point under the pointer fixed and spreads neighbors symmetrically", () => {
    const { shifts } = magnify(centers, centers[3] ?? 0, opts);
    expect(shifts[3]).toBeCloseTo(0);
    expect(shifts[2] ?? 0).toBeLessThan(0);
    expect(shifts[4] ?? 0).toBeGreaterThan(0);
    expect(shifts[2] ?? 0).toBeCloseTo(-(shifts[4] ?? 0));
    expect(shifts[0] ?? 0).toBeCloseTo(-(shifts[6] ?? 0));
  });

  it("is continuous between two icons", () => {
    const mid = ((centers[2] ?? 0) + (centers[3] ?? 0)) / 2;
    const a = magnify(centers, mid - 0.01, opts).shifts;
    const b = magnify(centers, mid + 0.01, opts).shifts;
    a.forEach((s, i) => expect(s).toBeCloseTo(b[i] ?? 0, 1));
  });

  it("handles an empty dock and a pointer past the ends", () => {
    expect(magnify([], 10, opts)).toEqual({ scales: [], shifts: [] });
    expect(magnify(centers, -500, opts).scales.every((s) => s === 1)).toBe(true);
  });
});
