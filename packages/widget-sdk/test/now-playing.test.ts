import { describe, expect, it } from "vitest";
import {
  accentSaturation,
  clock,
  dominantHue,
  fraction,
  positionNow,
  remaining,
  rgbToHsl,
} from "../../../widgets/now-playing/media.js";

/** RGBA pixels from a list of [r, g, b] colors. */
const pixels = (...colors: [number, number, number][]) => colors.flatMap(([r, g, b]) => [r, g, b, 255]);
const many = (n: number, color: [number, number, number]) => Array.from({ length: n }, () => color);

describe("clock and remaining", () => {
  it("formats minutes and seconds, hours when needed, and survives garbage", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(187.9)).toBe("3:07");
    expect(clock(3723)).toBe("1:02:03");
    expect(clock(-5)).toBe("0:00");
    expect(clock(Number.NaN)).toBe("0:00");
  });

  it("shows the time left, never below zero", () => {
    expect(remaining(57, 243)).toBe("-3:06");
    expect(remaining(300, 243)).toBe("-0:00");
  });
});

describe("positionNow", () => {
  const playing = { position: 57, duration: 243, playing: true };

  it("advances while playing and holds while paused", () => {
    expect(positionNow(playing, 3000)).toBe(60);
    expect(positionNow({ ...playing, playing: false }, 3000)).toBe(57);
  });

  it("stays inside the track", () => {
    expect(positionNow(playing, 10_000_000)).toBe(243);
    expect(positionNow({ ...playing, position: -4 }, 0)).toBe(0);
  });

  it("is 0 without a duration, and treats a missing position as the start", () => {
    expect(positionNow({ playing: true }, 5000)).toBe(0);
    expect(positionNow({ playing: true, duration: 100 }, 5000)).toBe(5);
  });

  it("turns into a fraction", () => {
    expect(fraction(60, 240)).toBe(0.25);
    expect(fraction(500, 240)).toBe(1);
    expect(fraction(10, undefined)).toBe(0);
    expect(fraction(10, 0)).toBe(0);
  });
});

describe("rgbToHsl", () => {
  it("gets the primaries and grays", () => {
    expect(rgbToHsl(255, 0, 0)).toMatchObject({ h: 0, s: 1, l: 0.5 });
    expect(rgbToHsl(0, 255, 0).h).toBeCloseTo(120);
    expect(rgbToHsl(0, 0, 255).h).toBeCloseTo(240);
    expect(rgbToHsl(128, 128, 128)).toMatchObject({ h: 0, s: 0 });
  });
});

describe("dominantHue", () => {
  it("finds the hue of a single-color cover", () => {
    expect(dominantHue(pixels(...many(50, [40, 90, 220])))?.h).toBeCloseTo(rgbToHsl(40, 90, 220).h, 0);
  });

  it("lets a vivid minority beat a large muted area", () => {
    const muted: [number, number, number] = [120, 110, 118];
    const vivid: [number, number, number] = [230, 40, 60];
    const found = dominantHue(pixels(...many(80, muted), ...many(20, vivid)));
    expect(found).not.toBeNull();
    expect(Math.abs((found?.h ?? 180) - rgbToHsl(...vivid).h)).toBeLessThan(10);
  });

  it("takes the larger of two vivid hues", () => {
    const found = dominantHue(pixels(...many(30, [220, 30, 30]), ...many(10, [30, 30, 220])));
    expect(found?.h ?? 999).toBeLessThan(15);
  });

  it("averages a hue that straddles the 0/360 seam", () => {
    const found = dominantHue(pixels(...many(20, [255, 20, 40]), ...many(20, [255, 40, 20])));
    const h = found?.h ?? 180;
    expect(Math.min(h, 360 - h)).toBeLessThan(15);
  });

  it("returns null for grayscale, near-black, near-white and empty covers", () => {
    expect(dominantHue(pixels(...many(40, [128, 128, 128])))).toBeNull();
    expect(dominantHue(pixels(...many(40, [4, 2, 3])))).toBeNull();
    expect(dominantHue(pixels(...many(40, [253, 252, 254])))).toBeNull();
    expect(dominantHue([])).toBeNull();
  });

  it("ignores transparent pixels", () => {
    const px = [...pixels(...many(30, [220, 30, 30])).map((v, i) => (i % 4 === 3 ? 0 : v)), ...pixels(...many(3, [30, 200, 60]))];
    expect(Math.abs((dominantHue(px)?.h ?? 0) - rgbToHsl(30, 200, 60).h)).toBeLessThan(10);
  });
});

describe("accentSaturation", () => {
  it("keeps accents vivid but not neon or gray", () => {
    expect(accentSaturation(0.05)).toBe(0.5);
    expect(accentSaturation(0.65)).toBe(0.65);
    expect(accentSaturation(1)).toBe(0.82);
  });
});
