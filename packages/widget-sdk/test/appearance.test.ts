import { describe, expect, it } from "vitest";
import {
  DEFAULT_APPEARANCE,
  appearanceAttributes,
  appearanceRule,
  appearanceVars,
  normalizeAppearance,
  type Appearance,
} from "../src";

const look = (over: Partial<Appearance> = {}): Appearance => ({ ...DEFAULT_APPEARANCE, ...over });

describe("attributes", () => {
  it("maps surface, text and shadow", () => {
    expect(appearanceAttributes(look())).toEqual({
      "data-surface": "none",
      "data-text": "light",
      "data-shadow": "on",
    });
    expect(appearanceAttributes(look({ surface: "glass", text: "dark", shadow: false }))).toEqual({
      "data-surface": "glass",
      "data-text": "dark",
      "data-shadow": "off",
    });
  });
});

describe("variables", () => {
  it("tints the surface at the opacity, and in px for blur and radius", () => {
    const v = appearanceVars(look({ surface: "glass", tint: "#102030", opacity: 0.4, blur: 30, radius: 12 }));
    expect(v["--surface"]).toBe("rgb(16 32 48 / 0.4)");
    expect(v["--blur"]).toBe("30px");
    expect(v["--radius"]).toBe("12px");
  });

  it("makes a solid surface opaque whatever the opacity", () => {
    expect(appearanceVars(look({ surface: "solid", opacity: 0.2 }))["--surface"]).toBe("rgb(16 16 16 / 1)");
  });

  it("flips the default near-black tint to a light one with dark text, but keeps a chosen tint", () => {
    expect(appearanceVars(look({ text: "dark", opacity: 0.5 }))["--surface"]).toBe("rgb(242 242 242 / 0.5)");
    expect(appearanceVars(look({ text: "dark", tint: "#203040", opacity: 0.5 }))["--surface"]).toBe("rgb(32 48 64 / 0.5)");
  });

  it("derives the border line from `border`, and drops it when off", () => {
    expect(appearanceVars(look())["--line"]).toBe("rgb(255 255 255 / 0.14)");
    expect(appearanceVars(look({ border: false }))["--line"]).toBe("transparent");
  });

  it("flips every ink token for dark text", () => {
    const light = appearanceVars(look());
    const dark = appearanceVars(look({ text: "dark" }));
    for (const name of ["--t0", "--t1", "--t2", "--t3", "--fill-1", "--fill-2", "--fill-3"]) {
      expect(light[name], name).toMatch(/^rgb\(255 255 255 \//);
      expect(dark[name], name).toMatch(/^rgb\(10 10 12 \//);
    }
    expect(light["--ink"]).toBe("255 255 255");
    expect(dark["--ink"]).toBe("10 10 12");
    // The text-shadow halo is the opposite of the ink.
    expect(light["--halo"]).toBe("0 0 0");
    expect(dark["--halo"]).toBe("255 255 255");
  });

  it("keeps the text steps strictly descending in alpha", () => {
    const v = appearanceVars(look());
    const alphas = ["--t0", "--t1", "--t2", "--t3"].map((k) => Number(/\/ ([\d.]+)\)/.exec(v[k] ?? "")?.[1]));
    expect(alphas).toEqual([...alphas].sort((a, b) => b - a));
    expect(new Set(alphas).size).toBe(4);
  });

  it("passes the accent through, with a readable ink on top of it", () => {
    const v = appearanceVars(look({ accent: "#ffd400" }));
    expect(v["--accent"]).toBe("rgb(255 212 0)");
    expect(v["--on-accent"]).toBe("#0a0a0c");
    expect(appearanceVars(look({ accent: "#1030c0" }))["--on-accent"]).toBe("#ffffff");
  });

  it("turns the default white accent into ink on dark text, where white would vanish", () => {
    expect(appearanceVars(look({ text: "dark" }))["--accent"]).toBe("#0a0a0c");
    expect(appearanceVars(look({ text: "dark", accent: "#2266dd" }))["--accent"]).toBe("rgb(34 102 221)");
  });

  it("adds the system fallback to the font and strips characters that break out of a declaration", () => {
    expect(appearanceVars(look({ font: "Inter" }))["--font"]).toBe("Inter, system-ui, sans-serif");
    const evil = appearanceVars(look({ font: "Inter; } body { display: none" }))["--font"];
    expect(evil).not.toMatch(/[;{}]/);
  });

  it("passes the scale through", () => {
    expect(appearanceVars(look({ scale: 1.25 }))["--scale"]).toBe("1.25");
  });

  it("renders one :root rule", () => {
    const rule = appearanceRule(look());
    expect(rule.startsWith(":root{")).toBe(true);
    expect(rule.endsWith("}")).toBe(true);
    expect(rule).toContain("--surface:");
    expect(rule.match(/:root/g)).toHaveLength(1);
  });
});

describe("normalizeAppearance", () => {
  it("returns the defaults for nothing", () => {
    expect(normalizeAppearance()).toEqual(DEFAULT_APPEARANCE);
  });

  it("defaults the surface to none", () => {
    expect(DEFAULT_APPEARANCE.surface).toBe("none");
  });

  it("clamps numbers into their ranges and replaces invalid values", () => {
    const a = normalizeAppearance({
      surface: "frosted",
      tint: "red",
      opacity: 7,
      blur: -3,
      radius: 99,
      scale: 0.1,
      text: 1,
      accent: "#12",
      border: "yes",
      shadow: null,
    });
    expect(a).toMatchObject({
      surface: "none",
      tint: DEFAULT_APPEARANCE.tint,
      opacity: 1,
      blur: 0,
      radius: 48,
      scale: 0.5,
      text: "light",
      accent: DEFAULT_APPEARANCE.accent,
      border: true,
      shadow: true,
    });
    expect(normalizeAppearance({ scale: 9 }).scale).toBe(2);
    expect(normalizeAppearance({ opacity: Number.NaN }).opacity).toBe(DEFAULT_APPEARANCE.opacity);
  });
});
