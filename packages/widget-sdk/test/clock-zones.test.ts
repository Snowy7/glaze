import { describe, expect, it } from "vitest";
import { cityName, isDaytime, offsetLabel, parseZones, worldLayout, zoneClock } from "../../../widgets/clock/zones.js";

describe("the World face's cities", () => {
  it("keeps well-formed items from the untrusted list, at most eight", () => {
    const raw = [{ label: " Oslo ", tz: "Europe/Oslo" }, null, { tz: 3 }, { label: "", tz: "" }, "x", { tz: "Asia/Tokyo" }];
    expect(parseZones(raw)).toEqual([{ label: "Oslo", tz: "Europe/Oslo" }, { label: "", tz: "Asia/Tokyo" }]);
    expect(parseZones("nope")).toEqual([]);
    expect(parseZones(Array.from({ length: 12 }, () => ({ tz: "UTC" })))).toHaveLength(8);
  });

  it("names a city by its label, else by its zone", () => {
    expect(cityName({ label: "Home", tz: "Europe/Oslo" })).toBe("Home");
    expect(cityName({ label: "", tz: "America/New_York" })).toBe("New York");
    expect(cityName({ label: "", tz: "" })).toBe("Unknown");
  });
});

describe("a city's wall clock", () => {
  const now = new Date(Date.UTC(2026, 10, 4, 23, 30, 0)); // 23:30 UTC

  it("reads the time there and how far it is from here", () => {
    const tokyo = zoneClock("Asia/Tokyo", now); // UTC+9, no DST
    expect(tokyo).not.toBeNull();
    expect([tokyo?.hours, tokyo?.minutes]).toEqual([8, 30]);
    expect(tokyo?.offset).toBe(9 * 60 + now.getTimezoneOffset());
    expect(zoneClock("Not/AZone", now)).toBeNull();
    expect(zoneClock("", now)).toBeNull();
  });

  it("says the difference, and the day when it is not today", () => {
    const day = (d: number) => (d > 0 ? "Tomorrow" : "Yesterday");
    expect(offsetLabel({ hours: 1, minutes: 0, seconds: 0, dayDiff: 0, offset: 0 }, day)).toBe("Same time");
    expect(offsetLabel({ hours: 1, minutes: 0, seconds: 0, dayDiff: 0, offset: 360 }, day)).toBe("+6h");
    expect(offsetLabel({ hours: 1, minutes: 0, seconds: 0, dayDiff: 0, offset: -210 }, day)).toBe("−3h 30m");
    expect(offsetLabel({ hours: 1, minutes: 0, seconds: 0, dayDiff: 1, offset: 480 }, day)).toBe("Tomorrow, +8h");
  });

  it("is day from 6 to 18", () => {
    expect([5, 6, 12, 17, 18, 23].map(isDaytime)).toEqual([false, true, true, true, false, false]);
  });
});

describe("the World face's layout", () => {
  it("draws dials when wide and tall, columns when short, rows otherwise", () => {
    expect(worldLayout(408, 200, 4)).toEqual({ mode: "dials", cols: 4, shown: 4 });
    expect(worldLayout(272, 168, 4)).toEqual({ mode: "dials", cols: 3, shown: 3 });
    expect(worldLayout(184, 80, 4)).toEqual({ mode: "cols", cols: 2, shown: 2 });
    expect(worldLayout(184, 184, 4)).toEqual({ mode: "rows", cols: 1, shown: 4 });
    expect(worldLayout(160, 160, 4)).toEqual({ mode: "rows", cols: 1, shown: 3 });
    expect(worldLayout(184, 184, 0)).toEqual({ mode: "rows", cols: 1, shown: 0 });
  });
});
