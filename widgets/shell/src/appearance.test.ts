import { describe, expect, it } from "vitest";
import { applyPatch, below, over, preview, resolve, type PreviewContext } from "./appearance";
import type { AppearanceValues } from "./protocol";

// Built-in defaults under a theme that has a glass surface and an `auto` accent.
const base: AppearanceValues = {
  surface: "glass",
  tint: "#101010",
  opacity: 0.4,
  blur: 24,
  radius: 18,
  border: true,
  text: "light",
  accent: "auto",
  font: "Segoe UI Variable",
  scale: 1,
  shadow: true,
  style: "minimal",
};

describe("layers", () => {
  it("stacks the style like any other field", () => {
    expect(resolve(base, {}, {}, "#fff").style).toBe("minimal");
    expect(resolve(base, { style: "depth" }, {}, "#fff").style).toBe("depth");
    expect(resolve(base, { style: "depth" }, { style: "neon" }, "#fff").style).toBe("neon");
    expect(applyPatch({ style: "neon" }, { style: null })).toEqual({});
  });

  it("shows minimal when a host older than v2 sends no style", () => {
    const old = { ...base };
    Reflect.deleteProperty(old, "style"); // what a pre-v2 `base` looks like on the wire
    expect(resolve(old, {}, {}, "#fff").style).toBe("minimal");
  });

  it("stack as defaults+theme < user < instance, and auto takes the palette accent", () => {
    const got = resolve(base, { opacity: 0.9, radius: 30 }, { radius: 2 }, "#14d17f");
    expect(got.radius).toBe(2); // the instance beats the user
    expect(got.opacity).toBe(0.9); // the user beats the theme
    expect(got.surface).toBe("glass"); // untouched fields show through
    expect(got.accent).toBe("#14d17f"); // auto resolved
    expect(resolve(base, {}, { accent: "#ff0000" }, "#14d17f").accent).toBe("#ff0000"); // a color is kept
  });

  it("dark text flips a default tint and accent, like the host does, but not chosen ones", () => {
    const plain: AppearanceValues = { ...base, tint: "#101010", accent: "#ffffff" };
    const dark = resolve(plain, { text: "dark" }, {}, "#14d17f");
    expect(dark.tint).toBe("#f2f2f2");
    expect(dark.accent).toBe("#0a0a0c");
    const chosen = resolve(plain, { text: "dark", tint: "#303040", accent: "#ff0000" }, {}, "#14d17f");
    expect([chosen.tint, chosen.accent]).toEqual(["#303040", "#ff0000"]);
    expect(resolve(plain, { text: "dark", accent: "auto" }, {}, "#14d17f").accent).toBe("#14d17f");
    expect(resolve(plain, {}, {}, "#14d17f").tint).toBe("#101010"); // light text: unchanged
  });

  it("clears a field with null and keeps the rest", () => {
    const layer = applyPatch({ blur: 10, border: false }, { blur: null, radius: 12, surface: "solid" });
    expect(layer).toEqual({ border: false, radius: 12, surface: "solid" });
    expect(over(base, layer).blur).toBe(24); // cleared: the layer below shows through again
  });

  it("does not mutate what it is given", () => {
    const own = { blur: 10 };
    applyPatch(own, { blur: null });
    expect(own).toEqual({ blur: 10 });
  });
});

describe("live preview", () => {
  const context: PreviewContext = {
    base,
    user: { opacity: 0.7 },
    autoAccent: "#14d17f",
    instances: [
      { instance: "clock-1", own: { radius: 4 } },
      { instance: "clock-2", own: {} },
    ],
  };

  it("a user-layer edit reaches every instance that does not set the field itself", () => {
    const looks = preview(context, { kind: "user" }, { opacity: 0.2, radius: 40 });
    expect(looks.get("clock-1")?.opacity).toBe(0.2);
    expect(looks.get("clock-1")?.radius).toBe(4); // its own value still wins
    expect(looks.get("clock-2")?.radius).toBe(40);
  });

  it("an instance edit changes only that instance", () => {
    const looks = preview(context, { kind: "instance", instance: "clock-2" }, { radius: 9 });
    expect([...looks.keys()]).toEqual(["clock-2"]);
    expect(looks.get("clock-2")).toMatchObject({ radius: 9, opacity: 0.7 }); // the user layer shows through
  });

  it("clearing a field falls back to what the layer below says", () => {
    const user = below({ kind: "user" }, base, context.user);
    expect(user.opacity).toBe(0.4); // theme, not the user's 0.7
    const instance = below({ kind: "instance", instance: "clock-1" }, base, context.user);
    expect(instance.opacity).toBe(0.7); // the user's choice
  });
});
