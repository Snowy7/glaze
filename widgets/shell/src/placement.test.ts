import { describe, expect, it } from "vitest";
import { canAdd, inPalette, layerOf } from "./placement";

const on = (widget: string) => ({ widget });
const state = (widgets: string[], counts?: Record<string, number>) => ({
  instances: widgets.map(on),
  ...(counts && { counts }),
});

describe("canAdd", () => {
  it("always allows a widget that can have many", () => {
    expect(canAdd({ id: "clock" }, state(["clock", "clock", "clock"]))).toEqual({ ok: true });
    expect(canAdd({ id: "clock", instances: "multiple" }, state(["clock"], { clock: 5 }))).toEqual({ ok: true });
  });

  it("allows a single widget until one exists on any monitor", () => {
    const manifest = { id: "pill", instances: "single" } as const;
    expect(canAdd(manifest, state([], { pill: 0 }))).toEqual({ ok: true });
    // not on this monitor, but on another one: counts are over all monitors
    expect(canAdd(manifest, state([], { pill: 1 }))).toMatchObject({ ok: false });
    expect(canAdd(manifest, state(["pill"], { pill: 1 }))).toMatchObject({ ok: false });
  });

  it("falls back to this monitor's instances when the host sends no counts", () => {
    const manifest = { id: "pill", instances: "single" } as const;
    expect(canAdd(manifest, state([]))).toEqual({ ok: true });
    expect(canAdd(manifest, state(["pill"]))).toMatchObject({ ok: false });
  });

  it("allows a per-monitor widget until this monitor has one, whatever the other monitors have", () => {
    const manifest = { id: "dock", instances: "per-monitor" } as const;
    expect(canAdd(manifest, state([], { dock: 2 }))).toEqual({ ok: true });
    expect(canAdd(manifest, state(["clock", "dock"], { dock: 1 }))).toMatchObject({ ok: false });
  });

  it("explains why it refuses", () => {
    const check = canAdd({ id: "pill", instances: "single" }, state(["pill"]));
    expect(check.ok ? "" : check.reason).toMatch(/already added/);
  });
});

describe("palette and layer", () => {
  it("lists desktop and overlay widgets but not popups", () => {
    expect(inPalette({})).toBe(true);
    expect(inPalette({ preset: "desktop" })).toBe(true);
    expect(inPalette({ preset: "overlay" })).toBe(true);
    expect(inPalette({ preset: "popup" })).toBe(false);
  });

  it("defaults the layer to desktop for older hosts", () => {
    expect(layerOf({})).toBe("desktop");
    expect(layerOf({ layer: "desktop" })).toBe("desktop");
    expect(layerOf({ layer: "overlay" })).toBe("overlay");
  });
});
