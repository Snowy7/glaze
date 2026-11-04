import { describe, expect, it } from "vitest";
import { lensMap, type LensShape } from "./liquid";
import { glassStyleOf, lookTokens } from "./style/look";
import type { Appearance, Look, SystemLook } from "./protocol";

const shape: LensShape = {
  width: 100,
  height: 40,
  radius: 20,
  bezel: 18,
  flush: { top: false, right: false, bottom: false, left: false },
  margin: 10,
};
const at = (map: ReturnType<typeof lensMap>, x: number, y: number) => {
  const i = (Math.floor(y) * map.width + Math.floor(x)) * 4;
  return [map.data[i] ?? 0, map.data[i + 1] ?? 0];
};

describe("the lens map", () => {
  it("is neutral in the clear middle and outside the pane", () => {
    const map = lensMap({ ...shape, height: 80, radius: 12 }, 1);
    expect(map.width).toBe(120);
    expect(at(map, 60, 50)).toEqual([128, 128]);
    expect(at(map, 2, 2)).toEqual([128, 128]);
  });

  it("pushes outward at the rim, strongest at the edge", () => {
    const map = lensMap(shape, 1);
    // Just inside the bottom edge, in the straight part: down (green above 128), no sideways.
    const [r, g] = at(map, 60, 10 + 39);
    expect(r).toBe(128);
    expect(g ?? 0).toBeGreaterThan(200);
    // Further in, weaker.
    expect(at(map, 60, 10 + 33)[1] ?? 0).toBeLessThan(g ?? 0);
    // Just inside the left cap: left (red below 128).
    expect(at(map, 10 + 1, 30)[0] ?? 255).toBeLessThan(60);
  });

  it("leaves edges flush with the screen without a rim", () => {
    const map = lensMap({ ...shape, flush: { ...shape.flush, top: true } }, 1);
    expect(at(map, 60, 10 + 1)).toEqual([128, 128]);
    expect(at(map, 60, 10 + 39)[1] ?? 0).toBeGreaterThan(200);
  });

  it("can be drawn smaller than the pane", () => {
    expect(lensMap(shape, 0.5).width).toBe(60);
  });
});

describe("the glass style", () => {
  const appearance = { tint: "#101010", opacity: 0.6, radius: 0, border: true, text: "light" } as Appearance;
  const look: Look = { font: {}, glass: {}, flyouts: {}, border: {}, glow: {}, shadow: {}, items: {}, indicator: {} };
  const system: SystemLook = { accent: ["#1", "#2", "#3", "#4", "#5", "#6", "#7"], scheme: "dark" };

  it("is the look's, else liquid when the theme's appearance is, else frosted", () => {
    expect(glassStyleOf(look, appearance)).toBe("frosted");
    expect(glassStyleOf(look, { ...appearance, style: "liquid" })).toBe("liquid");
    expect(glassStyleOf({ ...look, glass: { style: "frosted" } }, { ...appearance, style: "liquid" })).toBe("frosted");
  });

  it("turns liquid on only where liquid may draw, and keeps solid fills", () => {
    const liquid = { ...look, glass: { style: "liquid" as const, tint: 0.2 }, background: { type: "wallpaper" as const, brightness: 1, blur: 0, interval: 10 } };
    const on = lookTokens(liquid, appearance, "dark", system, {}, true, true);
    expect(on.attrs).toMatchObject({ "data-glass-style": "liquid", "data-liquid": "on", "data-fly-liquid": "on" });
    expect(on.vars["--s-tint"]).toBe("rgb(16 16 16 / 0.2)");
    expect(lookTokens(liquid, appearance, "dark", system, {}, true, false).attrs["data-liquid"]).toBe("off");
    const solid = { ...liquid, background: { type: "solid" as const, color: "#000" } };
    expect(lookTokens(solid, appearance, "dark", system, {}, true, true).attrs).toMatchObject({ "data-liquid": "off", "data-fly-liquid": "on" });
  });
});
