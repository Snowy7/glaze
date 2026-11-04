import { describe, expect, it } from "vitest";
import { createRotation, kenBurns, moveSeconds, readImages } from "../../../widgets/photo-frame/frame.js";

/** A repeatable stand-in for Math.random. */
function sequence(seed: number) {
  let s = seed;
  return () => ((s = (s * 16807) % 2147483647) / 2147483647);
}

describe("photo frame images", () => {
  it("keeps served, blob and image data URLs in order, once each", () => {
    const url = "https://photo-frame.widget.mypaper/__data/photo-frame-1/a.jpg";
    const settings = {
      image1: url,
      image2: "",
      image3: "  blob:http://localhost:5273/1234  ",
      image4: url,
      image5: "data:image/svg+xml,%3Csvg%3E",
      image6: "javascript:alert(1)",
    };
    expect(readImages(settings)).toEqual([url, "blob:http://localhost:5273/1234", "data:image/svg+xml,%3Csvg%3E"]);
  });

  it("drops what is not a URL of an image", () => {
    expect(readImages({ image1: "C:\\Users\\me\\a.jpg", image2: 5, image3: "data:text/html,hi", image4: "file:///etc/passwd" })).toEqual([]);
    expect(readImages({})).toEqual([]);
  });
});

describe("photo frame order", () => {
  it("goes through the pictures in order, forever", () => {
    const r = createRotation(3, false);
    expect(Array.from({ length: 7 }, () => r.next())).toEqual([0, 1, 2, 0, 1, 2, 0]);
  });

  it("peeks at what comes next without taking it", () => {
    const r = createRotation(3, false);
    expect(r.peek()).toBe(0);
    expect(r.next()).toBe(0);
    expect(r.peek()).toBe(1);
  });

  it("shuffles in rounds: every picture once per round, never the same twice in a row", () => {
    const r = createRotation(4, true, sequence(11));
    const order = Array.from({ length: 40 }, () => r.next());
    for (let round = 0; round < 10; round++) {
      expect(new Set(order.slice(round * 4, round * 4 + 4)).size).toBe(4);
    }
    for (let i = 1; i < order.length; i++) expect(order[i]).not.toBe(order[i - 1]);
  });

  it("copes with one picture and none", () => {
    expect(createRotation(1, true).next()).toBe(0);
    expect(createRotation(0, true).next()).toBe(0);
  });
});

describe("photo frame Ken Burns", () => {
  it("does not move at all at zero intensity", () => {
    expect(kenBurns(0)).toEqual({ x0: 0, y0: 0, s0: 1, x1: 0, y1: 0, s1: 1 });
    expect(kenBurns(Number.NaN)).toEqual(kenBurns(0));
  });

  it("never uncovers an edge: the shift stays within what the scale allows", () => {
    const random = sequence(5);
    for (let i = 0; i < 500; i++) {
      const k = kenBurns(i % 101, random);
      expect(k.s0).toBeGreaterThanOrEqual(1);
      expect(k.s1).toBeGreaterThanOrEqual(1);
      for (const [x, y, s] of [
        [k.x0, k.y0, k.s0],
        [k.x1, k.y1, k.s1],
      ] as const) {
        const room = (s - 1) * 50 + 1e-6;
        expect(Math.abs(x)).toBeLessThanOrEqual(room);
        expect(Math.abs(y)).toBeLessThanOrEqual(room);
      }
    }
  });

  it("zooms more at higher intensity, in or out", () => {
    const zoom = (m: number) => {
      const k = kenBurns(m, sequence(3));
      return Math.abs(k.s1 - k.s0);
    };
    expect(zoom(100)).toBeGreaterThan(zoom(50));
    expect(zoom(50)).toBeGreaterThan(zoom(10));
  });

  it("really goes somewhere", () => {
    const random = sequence(9);
    for (let i = 0; i < 100; i++) {
      const k = kenBurns(60, random);
      expect(Math.hypot(k.x1 - k.x0, k.y1 - k.y0)).toBeGreaterThan(0.5);
    }
  });

  it("keeps a slide moving through both crossfades", () => {
    expect(moveSeconds(15, 1.2)).toBeCloseTo(17.4);
  });
});
