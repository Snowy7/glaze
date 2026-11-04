import { describe, expect, it } from "vitest";
import { clockText, formatClock } from "./clock-format";

// Wednesday 4 March 2026, 14:05:09 local.
const afternoon = new Date(2026, 2, 4, 14, 5, 9);
const midnight = new Date(2026, 0, 9, 0, 7, 3);
const f = (format: string, date = afternoon) => formatClock(format, date, "en-US");

describe("the taskbar clock", () => {
  const system = { time: "2:05 PM", timeWithSeconds: "2:05:09 PM", date: "3/4/2026" };

  it("uses Windows' strings, including custom regional formats and seconds", () => {
    expect(clockText({ format: null, seconds: false }, afternoon, system)).toBe("2:05 PM\n3/4/2026");
    expect(clockText({ format: null, seconds: true }, afternoon, system)).toBe("2:05:09 PM\n3/4/2026");
    expect(clockText({ format: null, seconds: false }, afternoon, { ...system, time: "14.05", date: "2026-03-04" })).toBe("14.05\n2026-03-04");
  });

  it("keeps explicit token formats as the override", () => {
    expect(clockText({ format: "HH:mm\ndd/MM/yyyy", seconds: false }, afternoon, system)).toBe("14:05\n04/03/2026");
    expect(clockText({ format: "", seconds: false }, afternoon, system)).toBe("");
  });

  it("uses Intl in browser previews, respecting the locale's hour cycle", () => {
    expect(clockText({ format: null, seconds: false }, afternoon, undefined, "en-US")).toBe("2:05 PM\n3/4/2026");
    expect(clockText({ format: null, seconds: false }, afternoon, undefined, "en-GB")).toBe("14:05\n04/03/2026");
  });
});

describe("formatClock", () => {
  it("formats the docs example", () => {
    expect(f("ddd d MMM  HH:mm")).toBe("Wed 4 Mar  14:05");
  });

  it("formats every date token", () => {
    expect(f("dddd")).toBe("Wednesday");
    expect(f("ddd")).toBe("Wed");
    expect(f("d dd", midnight)).toBe("9 09");
    expect(f("MMMM MMM")).toBe("March Mar");
    expect(f("M MM", midnight)).toBe("1 01");
    expect(f("yyyy yy")).toBe("2026 26");
  });

  it("formats 24 and 12 hour times", () => {
    expect(f("H HH")).toBe("14 14");
    expect(f("h hh tt")).toBe("2 02 PM");
    expect(f("H HH h hh tt", midnight)).toBe("0 00 12 12 AM");
    expect(f("mm:ss")).toBe("05:09");
  });

  it("keeps literals, quoted text and escaped quotes", () => {
    expect(f("HH:mm 'o''clock' d")).toBe("14:05 o'clock 4");
    expect(f("'dddd' dddd")).toBe("dddd Wednesday");
    expect(f("W: x/y - z")).toBe("W: x/y - z");
    expect(f("''")).toBe("'");
  });

  it("treats an unterminated quote as literal to the end", () => {
    expect(f("HH 'mm")).toBe("14 mm");
  });
});
