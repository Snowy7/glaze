import { describe, expect, it } from "vitest";
import { fakeAppIcon, fakeBeat, fakeSpectrum } from "../dev/fake";

describe("fakeSpectrum", () => {
  it("returns `bins` floats in 0..1 at any time", () => {
    for (const bins of [64, 100, 128]) {
      for (let t = 0; t < 30; t += 0.37) {
        const spectrum = fakeSpectrum(t, bins);
        expect(spectrum).toHaveLength(bins);
        expect(spectrum.every((v) => Number.isFinite(v) && v >= 0 && v <= 1)).toBe(true);
      }
    }
  });

  it("is a pure function of time, and moves", () => {
    expect(fakeSpectrum(3.2, 64)).toEqual(fakeSpectrum(3.2, 64));
    expect(fakeSpectrum(3.2, 64)).not.toEqual(fakeSpectrum(3.6, 64));
  });

  it("tilts: the lows carry more than the highs on average", () => {
    const mean = (a: number[]) => a.reduce((s, v) => s + v, 0) / a.length;
    let lows = 0;
    let highs = 0;
    for (let t = 0; t < 20; t += 0.5) {
      const s = fakeSpectrum(t, 64);
      lows += mean(s.slice(0, 16));
      highs += mean(s.slice(48));
    }
    expect(lows).toBeGreaterThan(highs);
  });

  it("handles no bins", () => {
    expect(fakeSpectrum(1, 0)).toEqual([]);
  });
});

describe("fakeBeat", () => {
  it("peaks on the beat, decays, and repeats every half second", () => {
    expect(fakeBeat(0)).toBe(1);
    expect(fakeBeat(0.25)).toBeLessThan(0.25);
    expect(fakeBeat(0.5)).toBeCloseTo(1);
    expect(fakeBeat(-0.5)).toBeCloseTo(1);
  });
});

describe("fakeAppIcon", () => {
  const letterOf = (url: string) => decodeURIComponent(url).match(/>(.)<\/text>/)?.[1];

  it("is an SVG data URL showing the first letter of the file name", () => {
    expect(fakeAppIcon("C:\\Windows\\notepad.exe", 64)).toMatch(/^data:image\/svg\+xml;utf8,/);
    expect(letterOf(fakeAppIcon("C:\\Windows\\notepad.exe", 64))).toBe("N");
    expect(letterOf(fakeAppIcon("C:\\Users\\ada\\Documents\\", 64))).toBe("D");
    expect(letterOf(fakeAppIcon("shell:AppsFolder\\Spotify!App", 64))).toBe("S");
    expect(letterOf(fakeAppIcon("???", 64))).toBe("?");
  });

  it("is the same for the same target, and differs by target and size", () => {
    expect(fakeAppIcon("calc.exe", 64)).toBe(fakeAppIcon("calc.exe", 64));
    expect(fakeAppIcon("calc.exe", 64)).not.toBe(fakeAppIcon("code.exe", 64));
    expect(decodeURIComponent(fakeAppIcon("calc.exe", 1000))).toContain('width="256"');
    expect(decodeURIComponent(fakeAppIcon("calc.exe", 1))).toContain('width="16"');
  });
});
