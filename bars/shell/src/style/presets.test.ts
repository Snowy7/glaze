import { describe, expect, it } from "vitest";
import { deepMerge, mockConfig, PRESETS } from "../mock-config";
import type { Preset } from "../protocol";
import { dockBand, topBand } from "../protocol";
import { pageTokens } from "./apply";

describe("preset resolution", () => {
  it("merges objects deeply and replaces lists", () => {
    expect(deepMerge({ top: { height: 28, left: ["start"] } }, { top: { left: ["clock"] }, dock: { size: 40 } })).toEqual({
      top: { height: 28, left: ["clock"] },
      dock: { size: 40 },
    });
  });

  it("lays each preset out as the host does", () => {
    const win = mockConfig("windows11");
    expect(win.top.enabled).toBe(false);
    expect(win.dock).toMatchObject({ style: "taskbar", size: 24, padding: 12, margin: 0 });
    expect(dockBand(win.dock)).toBe(48);
    const islands = mockConfig("islands");
    expect(islands.top.layout).toBe("islands");
    expect(islands.top.right).toContain("|");
    expect(topBand(islands.top)).toBe(34 + 12);
    const mono = mockConfig("monochrome", { dock: { size: 56 } });
    expect(dockBand(mono.dock)).toBe(56 + 10 + 12);
    expect(mono.top.margin).toBe(0);
  });

  it("gives every preset a look, and the user the last word", () => {
    for (const name of Object.keys(PRESETS) as Preset[]) {
      const config = mockConfig(name, { look: { noise: 0 } });
      expect(config.preset).toBe(name);
      expect(config.look.noise).toBe(0);
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
        mode: { reduceMotion: false, performance: false },
        backdrop: null,
      });
      expect(attrs["data-preset"]).toBe(name);
      expect(attrs["data-style"]).toBe(config.dock.style);
      expect(vars["--theme-glow"]).toBe("#ff00ff");
      expect(vars["--m-show"]).toMatch(/ms$/);
    }
  });
});
