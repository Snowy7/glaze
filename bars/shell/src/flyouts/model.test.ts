import { describe, expect, it } from "vitest";
import { ago, bars, firstDayOf, monthGrid, offsetLabel, splitTray, zoneLabel, zoneOffset } from "./model";

describe("monthGrid", () => {
  it("lays out six weeks starting on the chosen day", () => {
    // November 2026 starts on a Sunday.
    const today = new Date(2026, 10, 4);
    const monday = monthGrid(2026, 10, 1, today);
    expect(monday).toHaveLength(6);
    expect(monday[0]?.[0]?.date.getDate()).toBe(26); // Monday 26 October
    expect(monday[0]?.[6]?.date.getDate()).toBe(1);
    expect(monday[0]?.[0]?.inMonth).toBe(false);
    expect(monday.flat().filter((d) => d.today).map((d) => d.date.getDate())).toEqual([4]);

    const sunday = monthGrid(2026, 10, 0, today);
    expect(sunday[0]?.[0]?.date.getDate()).toBe(1);
    expect(sunday.flat().filter((d) => d.inMonth)).toHaveLength(30);
  });

  it("takes the first day from the config, else the locale", () => {
    expect(firstDayOf("sunday", "en-GB")).toBe(0);
    expect(firstDayOf(undefined, "de-DE")).toBe(1);
    expect(firstDayOf(undefined, "not a locale!")).toBe(1);
  });
});

describe("extra clocks", () => {
  it("measures zone offsets and labels the difference", () => {
    const winter = new Date(Date.UTC(2026, 0, 15, 12));
    expect(zoneOffset(winter, "UTC")).toBe(0);
    expect(zoneOffset(winter, "Asia/Kolkata")).toBe(330);
    expect(zoneOffset(winter, "America/New_York")).toBe(-300);
    expect(offsetLabel(330, 60)).toBe("+4:30");
    expect(offsetLabel(-300, 60)).toBe("−6h");
    expect(offsetLabel(60, 60)).toBe("Same time");
    expect(zoneLabel({ zone: "America/New_York" })).toBe("New York");
    expect(zoneLabel({ zone: "Asia/Tokyo", label: "Studio" })).toBe("Studio");
  });
});

describe("splitTray", () => {
  it("pins go to the bar, the rest to the overflow, the native chevron apart", () => {
    const split = splitTray([
      { id: "a", name: "A", pinned: true },
      { id: "b", name: "B", pinned: false },
      { id: "overflow", name: "Show hidden icons", pinned: true },
    ]);
    expect(split.bar.map((i) => i.id)).toEqual(["a"]);
    expect(split.overflow.map((i) => i.id)).toEqual(["b"]);
    expect(split.native?.id).toBe("overflow");
  });
});

describe("ago and bars", () => {
  it("says how long ago, briefly", () => {
    const now = new Date(2026, 10, 4, 12).getTime();
    expect(ago(now - 10_000, now)).toBe("Now");
    expect(ago(now - 5 * 60_000, now)).toBe("5m");
    expect(ago(now - 3 * 3_600_000, now)).toBe("3h");
    expect(ago(new Date(2026, 10, 3, 9).getTime(), now)).toBe("Yesterday");
  });

  it("maps signal to bars", () => {
    expect([10, 40, 70, 100].map(bars)).toEqual([1, 2, 3, 4]);
  });
});
