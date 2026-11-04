import { describe, expect, it } from "vitest";
import {
  DEFAULT_APPEARANCE,
  appearanceAttributes,
  appearanceRule,
  appearanceVars,
  audioBins,
  normalizeAppearance,
  sparkColors,
  withToneDefaults,
  type Appearance,
} from "../src";

const look = (over: Partial<Appearance> = {}): Appearance => ({ ...DEFAULT_APPEARANCE, ...over });

describe("attributes", () => {
  it("maps surface, text and shadow", () => {
    expect(appearanceAttributes(look())).toEqual({
      "data-surface": "glass",
      "data-text": "light",
      "data-shadow": "on",
      "data-style": "minimal",
      "data-border": "on",
    });
    expect(appearanceAttributes(look({ surface: "glass", text: "dark", shadow: false, style: "neon" }))).toEqual({
      "data-surface": "glass",
      "data-text": "dark",
      "data-shadow": "off",
      "data-style": "neon",
      "data-border": "on",
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
    expect(appearanceVars(look({ surface: "solid", opacity: 0.2 }))["--surface"]).toBe("rgb(52 58 66 / 1)");
  });

  it("flips the default near-black tint (host side) with dark text, but keeps a chosen tint", () => {
    expect(appearanceVars(withToneDefaults(look({ text: "dark", opacity: 0.5 })))["--surface"]).toBe("rgb(242 242 242 / 0.5)");
    expect(appearanceVars(look({ text: "dark", tint: "#203040", opacity: 0.5 }))["--surface"]).toBe("rgb(32 48 64 / 0.5)");
  });

  it("derives the border line from `border`, and drops it when off", () => {
    expect(appearanceVars(look())["--line"]).toBe("rgb(255 255 255 / 0.1)");
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

  it("uses tint and accent exactly as sent; tone-aware defaults are the host's job", () => {
    // An explicit near-black tint with dark text stays near-black (no double flip).
    expect(appearanceVars(look({ text: "dark", surface: "solid" }))["--surface"]).toBe("rgb(52 58 66 / 1)");
    const hosted = withToneDefaults(look({ text: "dark" }));
    expect(hosted.tint).toBe("#f2f2f2");
    expect(appearanceVars(hosted)["--accent"]).toBe("rgb(10 10 12)");
    expect(withToneDefaults(look({ text: "dark", accent: "#2266dd" })).accent).toBe("#2266dd");
    expect(withToneDefaults(look()).tint).toBe(DEFAULT_APPEARANCE.tint);
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

  it("defaults to the calm tile: glass, minimal", () => {
    expect(DEFAULT_APPEARANCE).toMatchObject({ surface: "glass", style: "minimal", tint: "#343a42" });
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
      style: "glossy",
    });
    expect(a).toMatchObject({
      surface: "glass",
      tint: DEFAULT_APPEARANCE.tint,
      opacity: 1,
      blur: 0,
      radius: 48,
      scale: 0.5,
      text: "light",
      accent: DEFAULT_APPEARANCE.accent,
      border: true,
      shadow: true,
      style: "minimal",
    });
    expect(normalizeAppearance({ style: "aero" }).style).toBe("aero");
    expect(normalizeAppearance({ style: "liquid" }).style).toBe("liquid");
    expect(normalizeAppearance({ scale: 9 }).scale).toBe(2);
    expect(normalizeAppearance({ opacity: Number.NaN }).opacity).toBe(DEFAULT_APPEARANCE.opacity);
  });
});

describe("style colors", () => {
  it("gives a colorless accent the style's signature pair, per style", () => {
    const white = appearanceVars(look({ style: "neon" }));
    expect(white["--spark-rgb"]).toBe("0 229 255");
    expect(white["--accent-2-rgb"]).toBe("255 43 214");
    expect(appearanceVars(look({ style: "aero" }))["--spark-rgb"]).toBe("92 200 255");
    // The ink accent of dark text is colorless too.
    expect(appearanceVars(look({ style: "paper", accent: "#0a0a0c" }))["--spark-rgb"]).toBe("217 130 91");
  });

  it("keeps a chromatic accent and turns its hue for the companion", () => {
    const [spark, spark2] = sparkColors([20, 209, 127], "depth");
    expect(spark).toEqual([20, 209, 127]);
    expect(spark2).not.toEqual(spark);
    // 48 degrees from green (~155) is cyan-blue (~203): blue grows past red.
    expect(spark2[2]).toBeGreaterThan(spark2[0]);
    const v = appearanceVars(look({ style: "neon", accent: "#14d17f" }));
    expect(v["--spark-rgb"]).toBe("20 209 127");
    expect(v["--accent-rgb"]).toBe("20 209 127");
  });
});

describe("style colors on light surfaces", () => {
  it("deepens the signature pair for dark text, but never a chromatic accent", () => {
    const light = appearanceVars(look({ style: "neon" }));
    const dark = appearanceVars(look({ style: "neon", text: "dark", accent: "#0a0a0c" }));
    const lightness = (rgb: string | undefined) => Math.max(...(rgb ?? "").split(" ").map(Number));
    expect(lightness(dark["--spark-rgb"])).toBeLessThan(lightness(light["--spark-rgb"]));
    expect(appearanceVars(look({ style: "neon", text: "dark", accent: "#14d17f" }))["--spark-rgb"]).toBe("20 209 127");
  });
});

describe("audioBins", () => {
  it("defaults to 64 and keeps requests inside the host's 64 to 128", () => {
    expect(audioBins(undefined)).toBe(64);
    expect(audioBins(Number.NaN)).toBe(64);
    expect(audioBins(10)).toBe(64);
    expect(audioBins(100.4)).toBe(100);
    expect(audioBins(4096)).toBe(128);
  });
});
