import { describe, expect, it } from "vitest";
import { dateLabels, fitScale, fontFamily, msToNextTick, timeParts } from "../../../widgets/clock/clock-format.js";

describe("clock time parts", () => {
  it("24-hour pads the hour and has no period", () => {
    expect(timeParts(new Date(2026, 10, 4, 7, 5, 9), { format24: true, padHours: false })).toEqual({ h: "07", m: "05", s: "09", period: "" });
  });

  it("12-hour maps midnight and noon to 12 and names the period", () => {
    const at = (h: number, padHours = false) => timeParts(new Date(2026, 10, 4, h, 30), { format24: false, padHours });
    expect(at(0)).toMatchObject({ h: "12", period: "AM" });
    expect(at(12)).toMatchObject({ h: "12", period: "PM" });
    expect(at(15)).toMatchObject({ h: "3", period: "PM" });
    expect(at(23)).toMatchObject({ h: "11", period: "PM" });
  });

  it("padHours keeps the leading zero in the 12-hour clock for the flip cards", () => {
    expect(timeParts(new Date(2026, 10, 4, 9, 41), { format24: false, padHours: true }).h).toBe("09");
  });
});

describe("clock date labels", () => {
  const date = new Date(2026, 10, 4);

  it("has a long and a short form per format", () => {
    expect(dateLabels(date, "full", "en-GB")).toEqual({
      weekday: { long: "Wednesday", short: "Wed" },
      date: { long: "4 November 2026", short: "4 Nov" },
    });
    expect(dateLabels(date, "short", "en-GB").date).toEqual({ long: "4 Nov", short: "4 Nov" });
    expect(dateLabels(date, "numeric", "en-GB").date).toEqual({ long: "04/11/2026", short: "04/11" });
  });

  it("follows the locale, and an unknown format is the full one", () => {
    expect(dateLabels(date, "full", "en-US").date.long).toBe("November 4, 2026");
    expect(dateLabels(date, "bogus", "en-GB")).toEqual(dateLabels(date, "full", "en-GB"));
  });
});

describe("clock wake-up", () => {
  it("lands just after the next boundary", () => {
    const noon = new Date(2026, 10, 4, 12, 0, 0, 0).getTime();
    expect(msToNextTick(noon, true)).toBe(1005);
    expect(msToNextTick(noon + 400, true)).toBe(605);
    expect(msToNextTick(noon + 59_000, false)).toBe(1005);
  });
});

describe("clock fit and font", () => {
  it("only ever shrinks, and ignores an empty box", () => {
    expect(fitScale(200, 400)).toBe(0.5);
    expect(fitScale(400, 200)).toBe(1);
    expect(fitScale(0, 200)).toBe(1);
    expect(fitScale(200, 0)).toBe(1);
  });

  it("quotes the font, falls back to the user's, and can't break out of the string", () => {
    expect(fontFamily("")).toBe("");
    expect(fontFamily("  ")).toBe("");
    expect(fontFamily("Cascadia Code")).toBe('"Cascadia Code", var(--font)');
    expect(fontFamily('Evil", red; x\'')).toBe('"Evil, red; x", var(--font)');
  });
});
