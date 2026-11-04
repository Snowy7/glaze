import { describe, expect, it } from "vitest";
import {
  barLayout,
  barSpan,
  bassEnergy,
  createPeaks,
  curveSegments,
  hasSettled,
  hueAt,
  idleLevels,
  isSilent,
  paletteStops,
  pingPong,
  radialBars,
  sampleBars,
  shape,
  smoothInto,
  stepPeaks,
  wavePoints,
} from "../../../widgets/now-playing/viz.js";

describe("sampleBars", () => {
  const ramp = Array.from({ length: 64 }, (_, i) => i / 63);

  it("takes the loudest bin a bar covers when there are fewer bars than bins", () => {
    const bins = [0, 0.9, 0, 0, 0.2, 0, 0, 0];
    expect(sampleBars(bins, 4)).toEqual([0.9, 0, 0.2, 0]);
  });

  it("interpolates when there are more bars than bins, keeping both ends", () => {
    const out = sampleBars([0, 1], 5);
    expect(out).toEqual([0, 0.25, 0.5, 0.75, 1]);
    expect(sampleBars([0.3], 3)).toEqual([0.3, 0.3, 0.3]);
  });

  it("keeps the order of a log-spaced spectrum at any count", () => {
    for (const count of [8, 33, 64, 100, 200]) {
      const out = sampleBars(ramp, count);
      expect(out).toHaveLength(count);
      expect(out.every((v, i) => i === 0 || v >= (out[i - 1] ?? 0))).toBe(true);
    }
  });

  it("handles nothing", () => {
    expect(sampleBars([], 5)).toEqual([]);
    expect(sampleBars([1, 2], 0)).toEqual([]);
  });
});

describe("shape and silence", () => {
  it("tilts toward the highs and clamps to 0..1", () => {
    expect(shape(0.5, 1, 1)).toBeGreaterThan(shape(0.5, 0, 1));
    expect(shape(0.9, 0.5, 3)).toBe(1);
    expect(shape(-1, 0.5, 1)).toBe(0);
  });

  it("calls only an all-zero spectrum silent", () => {
    expect(isSilent([0, 0, 0])).toBe(true);
    expect(isSilent([0, 0.01, 0])).toBe(false);
    expect(isSilent([])).toBe(true);
  });

  it("has an idle profile that is low and stays in range", () => {
    const idle = idleLevels(64);
    expect(idle).toHaveLength(64);
    expect(Math.max(...idle)).toBeLessThan(0.15);
    expect(Math.min(...idle)).toBeGreaterThan(0);
  });
});

describe("smoothInto", () => {
  it("rises faster than it falls", () => {
    const up = [0];
    smoothInto(up, [1], 33, 0.5);
    const down = [1];
    smoothInto(down, [0], 33, 0.5);
    expect(up[0]).toBeGreaterThan(1 - (down[0] ?? 0));
  });

  it("follows the target at once with no smoothing on the way up", () => {
    const levels = [0];
    smoothInto(levels, [0.8], 33, 0);
    expect(levels[0]).toBe(0.8);
  });

  it("falls more slowly the more smoothing there is", () => {
    const low = [1];
    const high = [1];
    smoothInto(low, [0], 33, 0.1);
    smoothInto(high, [0], 33, 0.9);
    expect(high[0]).toBeGreaterThan(low[0] ?? 1);
  });

  it("does not depend on the frame rate", () => {
    const one = [1];
    smoothInto(one, [0], 66, 0.5);
    const two = [1];
    smoothInto(two, [0], 33, 0.5);
    smoothInto(two, [0], 33, 0.5);
    expect(one[0]).toBeCloseTo(two[0] ?? NaN, 6);
  });

  it("settles on the target in finite time", () => {
    const levels = [1, 0.5];
    const targets = [0, 0];
    let ticks = 0;
    while (!hasSettled(levels, targets) && ticks < 1000) {
      smoothInto(levels, targets, 33, 1);
      ticks++;
    }
    expect(ticks).toBeLessThan(400);
    expect(levels).toEqual([0, 0]);
  });
});

describe("peak hold", () => {
  it("jumps up with the bar, holds, then falls but never under the bar", () => {
    const peaks = createPeaks(1);
    stepPeaks(peaks, [0.8], 33, 400, 1);
    expect(peaks.level[0]).toBe(0.8);
    // The bar drops: the cap stays put for the hold time...
    for (let t = 0; t < 300; t += 33) stepPeaks(peaks, [0.2], 33, 400, 1);
    expect(peaks.level[0]).toBe(0.8);
    // ...then falls at 1 level per second.
    for (let t = 0; t < 330; t += 33) stepPeaks(peaks, [0.2], 33, 400, 1);
    expect(peaks.level[0]).toBeLessThan(0.8);
    expect(peaks.level[0]).toBeGreaterThan(0.2);
    for (let t = 0; t < 3000; t += 33) stepPeaks(peaks, [0.2], 33, 400, 1);
    expect(peaks.level[0]).toBe(0.2);
  });

  it("restarts the hold when the bar reaches the cap again", () => {
    const peaks = createPeaks(1);
    stepPeaks(peaks, [0.5], 33, 400, 1);
    for (let t = 0; t < 500; t += 33) stepPeaks(peaks, [0.1], 33, 400, 1);
    stepPeaks(peaks, [0.9], 33, 400, 1);
    expect(peaks.level[0]).toBe(0.9);
    expect(peaks.hold[0]).toBe(400);
  });
});

describe("bar geometry", () => {
  it("fills the width exactly, at any size", () => {
    for (const width of [120, 333, 600, 1000]) {
      const { count, pitch, bar } = barLayout(width, 6, 3);
      expect((count - 1) * pitch + bar).toBeCloseTo(width, 6);
      expect(bar).toBeGreaterThanOrEqual(6 - 1);
    }
  });

  it("fits fewer bars when they are wider or further apart, and always one", () => {
    expect(barLayout(300, 12, 4).count).toBeLessThan(barLayout(300, 6, 4).count);
    expect(barLayout(300, 6, 8).count).toBeLessThan(barLayout(300, 6, 2).count);
    expect(barLayout(2, 6, 3).count).toBe(1);
  });

  it("grows bars from the bottom, or from the middle when mirrored", () => {
    expect(barSpan(0.5, 100, false, 4)).toEqual({ y: 50, h: 50 });
    expect(barSpan(0.5, 100, true, 4)).toEqual({ y: 25, h: 50 });
  });

  it("keeps a minimum height and never overflows the box", () => {
    expect(barSpan(0, 100, false, 4)).toEqual({ y: 96, h: 4 });
    expect(barSpan(0, 100, true, 4)).toEqual({ y: 48, h: 4 });
    expect(barSpan(1.5, 100, false, 4).h).toBe(100);
  });
});

describe("wave", () => {
  it("spans the width and rises with the level", () => {
    const pts = wavePoints([0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0], 110, 100, 80);
    expect(pts[0]?.x).toBe(0);
    expect(pts.at(-1)?.x).toBe(110);
    expect(pts[5]?.y).toBeCloseTo(20, 6);
    expect(pts[0]?.y).toBe(100);
  });

  it("eases the ends down to the baseline", () => {
    const pts = wavePoints(new Array(50).fill(1), 100, 100, 100);
    expect(pts[0]?.y).toBe(100);
    expect(pts.at(-1)?.y).toBe(100);
    expect(pts[25]?.y).toBe(0);
    expect(pts[2]?.y).toBeGreaterThan(pts[3]?.y ?? NaN);
  });

  it("builds one Catmull-Rom segment per gap, passing through the points", () => {
    const pts = [
      { x: 0, y: 0 },
      { x: 10, y: 10 },
      { x: 20, y: 0 },
    ];
    const segs = curveSegments(pts);
    expect(segs).toHaveLength(2);
    expect(segs[0]).toMatchObject({ x: 10, y: 10 });
    expect(segs[1]).toMatchObject({ x: 20, y: 0 });
    // A straight line stays straight.
    const line = curveSegments([{ x: 0, y: 5 }, { x: 10, y: 5 }, { x: 20, y: 5 }]);
    expect(line.every((s) => s.c1y === 5 && s.c2y === 5)).toBe(true);
  });
});

describe("radialBars", () => {
  const o = { cx: 100, cy: 100, inner: 20, length: 40, minLength: 2, mirror: false };

  it("spaces spokes evenly around the full circle, outward from the inner radius", () => {
    const bars = radialBars([0.5, 0.5, 0.5, 0.5], o);
    expect(bars).toHaveLength(4);
    const gaps = bars.slice(1).map((b, i) => b.angle - (bars[i]?.angle ?? 0));
    for (const g of gaps) expect(g).toBeCloseTo(Math.PI / 2, 6);
    for (const b of bars) {
      expect(Math.hypot(b.x0 - 100, b.y0 - 100)).toBeCloseTo(20, 6);
      expect(Math.hypot(b.x1 - 100, b.y1 - 100)).toBeCloseTo(40, 6);
    }
  });

  it("starts at the bottom and runs clockwise (y down)", () => {
    const [first, second] = radialBars([0, 0, 0, 0], { ...o, mirror: false });
    // Clockwise from the bottom heads to the left on screen.
    expect(first?.x0).toBeLessThan(100);
    expect(first?.y0).toBeGreaterThan(100);
    expect((second?.angle ?? 0) - (first?.angle ?? 0)).toBeGreaterThan(0);
  });

  it("mirrors across the vertical axis, doubling the spokes", () => {
    const bars = radialBars([0.2, 0.9, 0.4], { ...o, mirror: true });
    expect(bars).toHaveLength(6);
    for (let i = 0; i < bars.length; i += 2) {
      const right = bars[i];
      const left = bars[i + 1];
      expect((left?.x1 ?? 0) - 100).toBeCloseTo(-((right?.x1 ?? 0) - 100), 6);
      expect(left?.y1).toBeCloseTo(right?.y1 ?? NaN, 6);
    }
  });

  it("never draws a spoke shorter than the minimum", () => {
    const [b] = radialBars([0], o);
    expect(Math.hypot((b?.x1 ?? 0) - (b?.x0 ?? 0), (b?.y1 ?? 0) - (b?.y0 ?? 0))).toBeCloseTo(2, 6);
  });

  it("measures the bass from the lowest bins only", () => {
    const loudLows = [1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    expect(bassEnergy(loudLows)).toBe(1);
    expect(bassEnergy([0, 0, 0, 0, 0, 0, 1, 1, 1, 1])).toBe(0);
    expect(bassEnergy([])).toBe(0);
  });
});

describe("colors", () => {
  it("runs the rainbow hue across a wide arc, always in 0..360", () => {
    expect(hueAt(0)).toBe(330);
    for (let t = -0.2; t <= 1.2; t += 0.05) {
      const h = hueAt(t);
      expect(h).toBeGreaterThanOrEqual(0);
      expect(h).toBeLessThan(360);
    }
    // The two ends differ, so the rainbow doesn't close on itself.
    expect(Math.abs(hueAt(1) - hueAt(0))).toBeGreaterThan(30);
  });

  it("builds stops per mode", () => {
    expect(paletteStops("accent", "#111", "#222")).toEqual([{ at: 0, color: "#111" }, { at: 1, color: "#111" }]);
    expect(paletteStops("gradient", "#111", "#222")).toEqual([{ at: 0, color: "#111" }, { at: 1, color: "#222" }]);
    const rainbow = paletteStops("rainbow", "#111", "#222", 45);
    expect(rainbow.length).toBeGreaterThan(4);
    expect(rainbow[0]?.at).toBe(0);
    expect(rainbow.at(-1)?.at).toBe(1);
    expect(rainbow.every((s) => s.color.startsWith("hsl(") && s.color.endsWith(" 45%)"))).toBe(true);
  });

  it("ping-pongs stops so the gradient closes", () => {
    const out = pingPong([{ at: 0, color: "a" }, { at: 1, color: "b" }]);
    expect(out).toEqual([
      { at: 0, color: "a" },
      { at: 0.5, color: "b" },
      { at: 1, color: "a" },
    ]);
    const three = pingPong([{ at: 0, color: "a" }, { at: 0.5, color: "b" }, { at: 1, color: "c" }]);
    expect(three.map((s) => s.color)).toEqual(["a", "b", "c", "b", "a"]);
    expect(three.every((s, i) => i === 0 || s.at >= (three[i - 1]?.at ?? 0))).toBe(true);
  });
});
