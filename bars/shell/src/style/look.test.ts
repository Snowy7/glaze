import { describe, expect, it } from "vitest";
import type { Appearance, Look, SystemLook } from "../protocol";
import { backdropTokens, cssColor, gradientCss, lookTokens, noiseImage, paintLayers, schemed, schemeOf } from "./look";

const appearance: Appearance = {
  surface: "glass",
  tint: "#0e0e0e",
  opacity: 0.6,
  blur: 30,
  radius: 16,
  border: true,
  text: "light",
  accent: "#ffffff",
  font: "Segoe UI Variable",
  scale: 1,
  shadow: false,
};
const system: SystemLook = {
  accent: ["#0078d4", "#0091f8", "#4cc2ff", "#99ebff", "#0067c0", "#003e92", "#001a68"],
  scheme: "light",
};
const empty: Look = { font: {}, glass: {}, flyouts: {}, border: {}, glow: {}, shadow: {}, items: {}, indicator: {} };
const tokens = (look: Partial<Look>, assets: Record<string, string> = {}, live = false) =>
  lookTokens({ ...empty, ...look }, appearance, "dark", system, assets, live);

describe("colors and paints", () => {
  it("map tokens to the page's variables", () => {
    expect(cssColor("#ff00aa80")).toBe("#ff00aa80");
    expect(cssColor("accent")).toBe("var(--accent)");
    expect(cssColor("text")).toBe("var(--t0)");
    expect(cssColor("system-accent-light-2")).toBe("var(--system-accent-light-2)");
    expect(cssColor("var(--glow)")).toBe("var(--glow)");
  });

  it("draw gradients of each kind", () => {
    const stops: Look["border"]["rest"] = undefined;
    expect(stops).toBeUndefined();
    expect(gradientCss({ kind: "linear", angle: 90, stops: ["#f0f", { color: "accent", at: 0.5 }] })).toBe(
      "linear-gradient(90deg, #f0f, var(--accent) 50%)",
    );
    expect(gradientCss({ kind: "conic", angle: 30, stops: ["#000", "#fff"] })).toBe("conic-gradient(from 30deg, #000, #fff)");
    expect(gradientCss({ kind: "radial", angle: 0, stops: ["#000", "#fff"] })).toMatch(/^radial-gradient\(/);
  });

  it("keep plain colors as background colors so state changes can fade", () => {
    expect(paintLayers("#fff")).toEqual(["#fff", "none"]);
    expect(paintLayers({ kind: "linear", angle: 0, stops: ["#000", "#fff"] })[0]).toBe("transparent");
  });
});

describe("scheme", () => {
  it("follows the look, then Windows for auto, then the appearance", () => {
    expect(schemeOf({ ...empty, scheme: "light" }, appearance, system)).toBe("light");
    expect(schemeOf({ ...empty, scheme: "auto" }, appearance, system)).toBe("light");
    expect(schemeOf(empty, appearance, system)).toBe("dark");
  });

  it("swaps text and a mismatched tint, keeps a matching appearance", () => {
    expect(schemed(appearance, "dark")).toBe(appearance);
    const light = schemed(appearance, "light");
    expect(light.text).toBe("dark");
    expect(light.tint).not.toBe(appearance.tint);
  });
});

describe("lookTokens", () => {
  it("falls back to the appearance's tint without a background", () => {
    const { vars, attrs } = tokens({});
    expect(attrs["data-fill"]).toBe("tint");
    expect(vars["--s-fill-color"]).toBe("var(--surface)");
    expect(vars["--s-radius"]).toBe("16px");
    expect(vars["--s-bw"]).toBe("1px");
    expect(vars["--system-accent-dark-3"]).toBe("#001a68");
  });

  it("draws solid, gradient and image fills", () => {
    expect(tokens({ background: { type: "solid", color: "#123456cc" } }).vars["--s-fill-color"]).toBe("#123456cc");
    const gradient = tokens({ background: { type: "gradient", kind: "linear", angle: 45, stops: ["#000", "#fff"] }, tint: 0.2 });
    expect(gradient.vars["--s-fill"]).toBe("linear-gradient(45deg, #000, #fff)");
    expect(gradient.vars["--s-tint"]).toBe("rgb(14 14 14 / 0.2)");
    const image = tokens(
      { background: { type: "image", path: "bg.png", fit: "tile", position: "top", brightness: 0.8, blur: 4 } },
      { "bg.png": "https://assets.bars.mypaper/x.png" },
    );
    expect(image.vars["--s-fill"]).toBe('url("https://assets.bars.mypaper/x.png")');
    expect(image.vars["--s-fill-repeat"]).toBe("repeat");
    expect(image.vars["--s-filter"]).toBe("brightness(0.8) saturate(1) blur(4px)");
    // An image that isn't served (yet) falls back to the tint.
    const missing = tokens({ background: { type: "image", path: "gone.png", fit: "cover", position: "center", brightness: 1, blur: 0 } });
    expect(missing.vars["--s-fill-color"]).toBe("var(--surface)");
  });

  it("puts wallpaper glass under the tint at the appearance's opacity", () => {
    const glass = tokens({ background: { type: "wallpaper", brightness: 0.9, blur: 0, interval: 10 }, saturation: 1.5 });
    expect(glass.attrs["data-fill"]).toBe("wallpaper");
    expect(glass.vars["--s-tint"]).toBe("rgb(14 14 14 / 0.6)");
    expect(glass.vars["--s-filter"]).toBe("brightness(0.9) saturate(1.5) blur(0px)");
  });

  it("layers hover and active over rest, per state", () => {
    const { vars } = tokens({
      border: { rest: { width: 1, color: "#fff" }, hover: { color: "accent" } },
      glow: { hover: { color: "#f0f", size: 12, opacity: 0.5 } },
      shadow: { rest: { y: 4, blur: 12, color: "#0008" } },
      items: { radius: 9, hover: { background: "#ffffff1a", scale: 1.05 } },
    });
    expect(vars["--s-bw-hover"]).toBe("1px");
    expect(vars["--s-bi-hover"]).toBe("linear-gradient(var(--accent), var(--accent))");
    expect(vars["--s-glow"]).toBe("0 0 0 0 transparent");
    expect(vars["--s-glow-hover"]).toBe("0 0 12px color-mix(in srgb, #f0f 50%, transparent)");
    expect(vars["--s-shadow-active"]).toBe("0px 4px 12px 0px #0008");
    expect(vars["--i-radius"]).toBe("9px");
    expect(vars["--i-bgc-hover"]).toBe("#ffffff1a");
    expect(vars["--i-scale-hover"]).toBe("1.05");
    expect(vars["--i-bgc-active"]).toBe("var(--fill-3)");
  });

  it("adds grain only when asked", () => {
    expect(noiseImage(0)).toBe("none");
    expect(noiseImage(0.05)).toMatch(/^url\("data:image\/svg\+xml,/);
  });
});

describe("backdropTokens", () => {
  it("sizes the snapshot to the monitor and offsets it by the window's place", () => {
    const { vars, attrs } = backdropTokens({ url: "https://backdrop.bars.mypaper/DISPLAY2.bmp?v=1", width: 2560, height: 1440, x: 0, y: 1018 });
    expect(attrs["data-backdrop"]).toBe("on");
    expect(vars["--backdrop-size"]).toBe("2560px 1440px");
    expect(vars["--backdrop-pos"]).toBe("0px -1018px");
    expect(backdropTokens(null).attrs["data-backdrop"]).toBe("off");
  });
});

describe("live blur and glass", () => {
  it("tints over live blur, and goes nearly opaque without it", () => {
    const blur = { background: { type: "blur" as const, brightness: 1.1, blur: 4 }, tint: 0.4, saturation: 1.8 };
    const live = tokens(blur, {}, true);
    expect(live.attrs["data-fill"]).toBe("blur");
    expect(live.vars["--s-tint"]).toBe("rgb(14 14 14 / 0.4)");
    expect(live.vars["--glass-sat"]).toBe("1.8");
    expect(live.vars["--glass-bright"]).toBe("1.1");
    expect(live.vars["--glass-blur"]).toBe("4");
    expect(tokens(blur).vars["--s-tint"]).toBe("rgb(14 14 14 / 0.9)");
  });

  it("carries glass highlights, type, family and the flyouts' material", () => {
    const { attrs, vars } = tokens(
      { family: "macos", glyphs: "symbols", glass: { highlight: 0.6, sheen: 0.2 }, font: { family: "SF Pro Text", size: 13, weight: 500 }, flyouts: { tint: 0.5 } },
      {},
      true,
    );
    expect(attrs).toMatchObject({ "data-family": "macos", "data-glyphs": "symbols", "data-fly-blur": "on" });
    expect(vars).toMatchObject({ "--g-highlight": "0.6", "--g-sheen": "0.2", "--g-depth": "0", "--bar-font": "SF Pro Text", "--bar-size": "13px", "--bar-weight": "500", "--fly-tint": "0.5" });
    expect(tokens({ flyouts: { blur: false } }, {}, true).attrs["data-fly-blur"]).toBe("off");
    expect(tokens({}).attrs["data-fly-blur"]).toBe("off");
  });
});
