import { describe, expect, it } from "vitest";
import { batteryState, ringDash, statusText } from "../../../widgets/battery/state.js";

describe("battery state", () => {
  it("maps stats to a state", () => {
    expect(batteryState(undefined)).toEqual({ kind: "none" });
    expect(batteryState({ percent: 64.4, charging: false })).toEqual({ kind: "discharging", percent: 64 });
    expect(batteryState({ percent: 64, charging: true })).toEqual({ kind: "charging", percent: 64 });
    expect(batteryState({ percent: 100, charging: true })).toEqual({ kind: "full", percent: 100 });
  });

  it("is low at 20% or less, but only on battery", () => {
    expect(batteryState({ percent: 21, charging: false }).kind).toBe("discharging");
    expect(batteryState({ percent: 20, charging: false }).kind).toBe("low");
    expect(batteryState({ percent: 5, charging: true }).kind).toBe("charging");
  });

  it("clamps and survives junk", () => {
    expect(batteryState({ percent: 140, charging: false })).toEqual({ kind: "discharging", percent: 100 });
    expect(batteryState({ percent: Number.NaN, charging: false })).toEqual({ kind: "low", percent: 0 });
  });

  it("never claims a time estimate", () => {
    expect(statusText("low")).toBe("On battery");
    expect(statusText("discharging")).toBe("On battery");
  });
});

describe("ring dash", () => {
  /** Total length of a dasharray: it must cover the whole circle exactly once. */
  const total = (dash: string) => dash.split(" ").reduce((sum, n) => sum + Number(n), 0);

  it("draws a solid arc", () => {
    expect(ringDash(30, false)).toBe("30 70");
  });

  it("breaks the arc into dashes that stop at the percent", () => {
    const dash = ringDash(15, true);
    expect(total(dash)).toBeCloseTo(100);
    expect(dash.split(" ").length % 2).toBe(0);
    expect(ringDash(0, true)).toBe("0 100");
  });
});
