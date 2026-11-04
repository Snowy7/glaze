import { describe, expect, it } from "vitest";
import { DOCK_BUNDLES, deepMerge, mockConfig, PRESETS, TOP_BUNDLES } from "../mock-config";
import { DOCK_DESIGNS, PRESETS as PRESET_NAMES, TOP_DESIGNS, type Mode, type Preset } from "../protocol";
import { dockBand, topBand } from "../protocol";
import { effectiveLook, pageTokens } from "./apply";

const mode: Mode = { reduceMotion: false, performance: false, blur: true, liquid: true };

describe("designs and presets", () => {
  it("merges objects deeply, replaces lists, and switches tagged variants whole", () => {
    expect(deepMerge({ top: { height: 28, left: ["start"] } }, { top: { left: ["clock"] }, dock: { size: 40 } })).toEqual({
      top: { height: 28, left: ["clock"] },
      dock: { size: 40 },
    });
    expect(deepMerge({ background: { type: "wallpaper", brightness: 0.9 } }, { background: { type: "solid", color: "#000" } })).toEqual({
      background: { type: "solid", color: "#000" },
    });
  });

  it("knows every bundle the host has", () => {
    expect(Object.keys(TOP_BUNDLES).sort()).toEqual([...TOP_DESIGNS].sort());
    expect(Object.keys(DOCK_BUNDLES).sort()).toEqual([...DOCK_DESIGNS].sort());
    expect(Object.keys(PRESETS).sort()).toEqual([...PRESET_NAMES].sort());
  });

  it("lays each design out as the host does", () => {
    const win = mockConfig("windows11");
    expect(win.top.enabled).toBe(false);
    expect(win.dock).toMatchObject({ design: "windows11", style: "taskbar", size: 24, padding: 12, margin: 0, button: 40 });
    expect(dockBand(win.dock)).toBe(48);
    expect(dockBand(mockConfig("windows10").dock)).toBe(40);
    const islands = mockConfig("islands");
    expect(islands.top.layout).toBe("islands");
    expect(islands.top.right).toContain("|");
    expect(topBand(islands.top)).toBe(34 + 12);
    expect(mockConfig("macos").top.height).toBe(24);
  });

  it("mixes a top design with any dock design, and layers looks like the host", () => {
    const mixed = mockConfig("monochrome", {
      top: { design: "macos", look: { tint: 0.9 } },
      dock: { design: "windows11" },
      look: { tint: 0.2, noise: 0.3 },
    });
    expect(mixed.top.look.family).toBe("macos");
    expect(mixed.dock.look.family).toBe("fluent");
    expect(mixed.top.look.tint).toBe(0.9);
    expect(mixed.dock.look.tint).toBe(0.2);
    expect(mixed.top.look.noise).toBe(0.3);
    expect(mixed.dock.clock.format).toBeNull();
  });

  it("turns an auto-hiding dock's wallpaper glass into live blur", () => {
    const hiding = mockConfig("monochrome", { dock: { autohide: true } });
    expect(effectiveLook("dock", hiding, mode).background?.type).toBe("blur");
    expect(effectiveLook("dock", hiding, { ...mode, blur: false }).background?.type).toBe("wallpaper");
    expect(effectiveLook("top", hiding, mode).background?.type).toBe("wallpaper");
  });

  it("gives every pairing a look, and the user the last word", () => {
    for (const name of Object.keys(PRESETS) as Preset[]) {
      const config = mockConfig(name, { dock: { look: { noise: 0 } } });
      expect(config.preset).toBe(name);
      expect(config.dock.look.noise).toBe(0);
      const { attrs, vars } = pageTokens({
        kind: "dock",
        config,
        look: {
          appearance: {
            surface: "glass",
            tint: "#0e0e0e",
            opacity: 0.6,
            blur: 30,
            radius: 16,
            border: true,
            text: "light",
            accent: "#fff",
            font: "Segoe UI",
            scale: 1,
            shadow: false,
          },
          themeCss: null,
          system: { accent: ["#1", "#2", "#3", "#4", "#5", "#6", "#7"], scheme: "dark" },
          vars: { "--theme-glow": "#ff00ff" },
          iconPack: null,
          assets: {},
        },
        mode,
        backdrop: null,
      });
      expect(attrs["data-preset"]).toBe(name);
      expect(attrs["data-design"]).toBe(config.dock.design);
      expect(attrs["data-style"]).toBe(config.dock.style);
      expect(attrs["data-live"]).toBe("on");
      expect(vars["--theme-glow"]).toBe("#ff00ff");
      expect(vars["--m-show"]).toMatch(/ms$/);
    }
  });
});
