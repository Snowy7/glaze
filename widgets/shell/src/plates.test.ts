import { describe, expect, it } from "vitest";
import { drawsPlate, plateShape, platesOf } from "./plates";

it("keeps a surface until this monitor has a snapshot, and restores it when that goes away", () => {
  const backdrop = { url: "https://shell.mypaper/backdrop/DISPLAY2.bmp", width: 2560, height: 1440 };
  expect(drawsPlate(undefined)).toBe(false);
  expect(drawsPlate({ layer: "desktop" })).toBe(false);
  expect(drawsPlate({ layer: "desktop", backdrop: null })).toBe(false);
  expect(drawsPlate({ layer: "desktop", backdrop })).toBe(true);
  expect(drawsPlate({ layer: "overlay", backdrop })).toBe(false);
  expect(drawsPlate({ layer: "desktop", backdrop: null })).toBe(false);
  expect(drawsPlate({ layer: "desktop", backdrop })).toBe(true);
});

describe("plateShape", () => {
  it("takes a box and a radius, and nothing else", () => {
    expect(plateShape({ rect: [8, 4, 200, 80], radius: 18 })).toEqual({ rect: [8, 4, 200, 80], radius: 18 });
    expect(plateShape({ rect: [0, 0, 10, 10], radius: -3 })?.radius).toBe(0);
    for (const bad of [null, "x", { rect: [0, 0, 10], radius: 1 }, { rect: [0, 0, 0, 10], radius: 1 }, { rect: [0, 0, 10, 10] }, { rect: [0, 0, Number.NaN, 10], radius: 1 }]) {
      expect(plateShape(bad)).toBeNull();
    }
  });
});

describe("platesOf", () => {
  it("places each plate on the monitor by its slot, clipped to the slot", () => {
    const slot = { x: 100, y: 50, width: 300, height: 100 };
    expect(
      platesOf([
        { instance: "a", plate: { rect: [6, 6, 288, 88], radius: 18 }, rect: slot },
        { instance: "b", plate: null, rect: slot },
        { instance: "c", plate: { rect: [-20, 10, 100, 200], radius: 60 }, rect: slot },
        { instance: "d", plate: { rect: [0, 0, 10, 10], radius: 2 }, rect: undefined },
      ]),
    ).toEqual([
      { instance: "a", rect: [106, 56, 288, 88], radius: 18 },
      { instance: "c", rect: [100, 60, 80, 90], radius: 40 },
    ]);
  });
});
