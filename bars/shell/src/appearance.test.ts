import { describe, expect, it } from "vitest";
import { appearanceTokens } from "./appearance";
import type { Appearance } from "./protocol";

const base: Appearance = {
  surface: "glass",
  tint: "#102030",
  opacity: 0.55,
  blur: 24,
  radius: 16,
  border: true,
  text: "light",
  accent: "#ffffff",
  font: "Segoe UI Variable",
  scale: 1,
  shadow: true,
};

describe("appearanceTokens", () => {
  it("tints the surface at its opacity and raises popovers", () => {
    const { vars, attrs } = appearanceTokens(base);
    expect(vars["--surface"]).toBe("rgb(16 32 48 / 0.55)");
    expect(vars["--raised"]).toBe("rgb(16 32 48 / 0.92)");
    expect(vars["--blur"]).toBe("24px");
    expect(attrs).toEqual({ "data-surface": "glass", "data-text": "light", "data-shadow": "on" });
  });

  it("has no surface for none, and no line without a border", () => {
    const { vars, attrs } = appearanceTokens({ ...base, surface: "none", border: false, shadow: false });
    expect(vars["--surface"]).toBe("transparent");
    expect(vars["--line"]).toBe("transparent");
    expect(attrs["data-shadow"]).toBe("off");
  });

  it("derives text steps and fills from the text color", () => {
    const light = appearanceTokens(base).vars;
    const dark = appearanceTokens({ ...base, text: "dark" }).vars;
    expect(light["--t0"]).toBe("#fff");
    expect(light["--t1"]).toBe("rgb(255 255 255 / 0.74)");
    expect(light["--fill-3"]).toBe("rgb(255 255 255 / 0.2)");
    expect(dark["--t0"]).toBe("#0a0a0a");
    expect(dark["--fill-1"]).toBe("rgb(0 0 0 / 0.05)");
    expect(dark["--line"]).toBe("rgb(0 0 0 / 0.12)");
  });

  it("falls back to the default tint for a bad color", () => {
    expect(appearanceTokens({ ...base, tint: "nope" }).vars["--surface"]).toBe("rgb(16 16 16 / 0.55)");
    expect(appearanceTokens({ ...base, tint: "#fff" }).vars["--surface"]).toBe("rgb(255 255 255 / 0.55)");
  });
});
