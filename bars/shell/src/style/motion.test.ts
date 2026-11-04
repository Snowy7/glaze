import { describe, expect, it } from "vitest";
import type { Motion } from "../protocol";
import { easing, motionTokens, resolveMotion, springAt, springLinear, springSettle } from "./motion";

describe("springs", () => {
  const bouncy = { mass: 1, stiffness: 260, damping: 13 };
  const critical = { mass: 1, stiffness: 100, damping: 20 };
  const heavy = { mass: 1, stiffness: 100, damping: 60 };

  it("start at 0 and come to rest at 1", () => {
    for (const spring of [bouncy, critical, heavy]) {
      expect(springAt(0, spring)).toBeCloseTo(0, 6);
      expect(springAt(10, spring)).toBeCloseTo(1, 3);
    }
  });

  it("overshoot only when underdamped", () => {
    const peak = (spring: typeof bouncy) =>
      Math.max(...Array.from({ length: 2000 }, (_, i) => springAt(i / 1000, spring)));
    expect(peak(bouncy)).toBeGreaterThan(1.2);
    expect(peak(critical)).toBeLessThanOrEqual(1);
    expect(peak(heavy)).toBeLessThanOrEqual(1);
  });

  it("settle later the less damped they are", () => {
    expect(springSettle(bouncy)).toBeGreaterThan(springSettle({ ...bouncy, damping: 26 }));
    expect(springSettle(bouncy)).toBeLessThanOrEqual(3);
  });

  it("render as a CSS linear() that follows the curve", () => {
    const { easing: css, duration } = springLinear(bouncy);
    expect(css.startsWith("linear(0, ")).toBe(true);
    expect(css.endsWith(", 1)")).toBe(true);
    expect(duration).toBe(Math.round(springSettle(bouncy) * 1000));
    const stops = css
      .slice("linear(".length, -1)
      .split(", ")
      .slice(1, -1)
      .map((stop) => stop.split(" ").map(Number.parseFloat) as [number, number]);
    // Every kept point is on the curve, and the overshoot is in there.
    for (const [value, percent] of stops) {
      expect(value).toBeCloseTo(springAt((percent / 100) * (duration / 1000), bouncy), 1);
    }
    expect(Math.max(...stops.map(([value]) => value))).toBeGreaterThan(1.2);
    // Collinear points are dropped: far fewer stops than samples.
    expect(stops.length).toBeLessThan(Math.ceil((duration / 1000) * 60));
  });
});

describe("easing", () => {
  it("maps names, beziers and springs to CSS", () => {
    expect(easing("linear")).toEqual({ easing: "linear" });
    expect(easing("smooth").easing).toBe("cubic-bezier(0.22, 0.7, 0.2, 1)");
    expect(easing([0.1, 0.9, 0.2, 1]).easing).toBe("cubic-bezier(0.1, 0.9, 0.2, 1)");
    expect(easing("spring").duration).toBeGreaterThan(0);
    expect(easing({ mass: 1, stiffness: 200, damping: 20 }).easing).toMatch(/^linear\(/);
    expect(easing("no-such-curve").easing).toBe(easing("smooth").easing);
  });
});

describe("the animation tree", () => {
  const tree: Motion = {
    duration: 200,
    curve: "smooth",
    bar: { style: "slide", dock: { duration: 400, magnify: { duration: 60 }, hover: { enabled: false } } },
    hover: { duration: 120 },
    open: { style: "pop", curve: "snappy" },
  };

  it("takes the most specific value, leaves over nodes", () => {
    expect(resolveMotion(tree, "top", "show")).toMatchObject({ duration: 200, style: "slide" });
    expect(resolveMotion(tree, "dock", "show")).toMatchObject({ duration: 400, style: "slide" });
    expect(resolveMotion(tree, "top", "hover").duration).toBe(120);
    expect(resolveMotion(tree, "dock", "press").duration).toBe(400);
    expect(resolveMotion(tree, "top", "open")).toMatchObject({ style: "pop", easing: easing("snappy").easing });
  });

  it("turns a leaf off when anything on its path says so", () => {
    expect(resolveMotion(tree, "dock", "hover")).toMatchObject({ enabled: false, duration: 0 });
    expect(resolveMotion(tree, "top", "hover").enabled).toBe(true);
    expect(resolveMotion({ enabled: false }, "top", "open").duration).toBe(0);
    expect(resolveMotion({ bar: { enabled: false } }, "dock", "magnify").enabled).toBe(false);
  });

  it("keeps magnify, bounce and attention on their own timing", () => {
    expect(resolveMotion(tree, "dock", "magnify")).toMatchObject({ duration: 60, easing: "linear" });
    expect(resolveMotion(tree, "dock", "bounce").duration).toBe(560);
    expect(resolveMotion({}, "dock", "attention").duration).toBe(900);
  });

  it("lets a spring set its own duration", () => {
    const springy = resolveMotion({ duration: 50, curve: "spring-bouncy" }, "top", "show");
    expect(springy.duration).toBe(easing("spring-bouncy").duration);
  });

  it("becomes CSS variables, all zero when motion is reduced", () => {
    const full = motionTokens(tree, "dock", false);
    expect(full.vars["--m-show"]).toBe("400ms");
    expect(full.vars["--m-hover"]).toBe("0ms");
    expect(full.attrs["data-m-open"]).toBe("pop");
    expect(full.attrs["data-motion"]).toBe("full");
    const reduced = motionTokens(tree, "dock", true);
    expect(Object.entries(reduced.vars).filter(([k]) => !k.endsWith("-ease")).every(([, v]) => v === "0ms")).toBe(true);
    expect(reduced.attrs["data-m-show"]).toBe("none");
  });
});
