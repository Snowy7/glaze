import { describe, expect, it } from "vitest";
import { indexForPeriod, nextPeriodStart, otherIndex, periodOf } from "../../../widgets/quote/pick.js";
import { QUOTES } from "../../../widgets/quote/quotes.js";

describe("quote rotation", () => {
  it("daily: the period is the local date, whatever the time of day", () => {
    const morning = new Date(2026, 10, 4, 0, 0, 1);
    const night = new Date(2026, 10, 4, 23, 59, 59);
    expect(periodOf("daily", morning)).toBe(periodOf("daily", night));
    expect(periodOf("daily", new Date(2026, 10, 5, 0, 0, 0))).toBe(periodOf("daily", morning) + 1);
  });

  it("hourly: changes on the hour; click never rotates", () => {
    const a = new Date(2026, 10, 4, 9, 59, 59);
    expect(periodOf("hourly", new Date(2026, 10, 4, 10, 0, 0))).toBe(periodOf("hourly", a) + 1);
    expect(periodOf("click", a)).toBe(periodOf("click", new Date(2030, 0, 1)));
    expect(nextPeriodStart("click", a)).toBeNull();
  });

  it("the next period starts at the next local midnight or hour", () => {
    const now = new Date(2026, 11, 31, 22, 15);
    expect(nextPeriodStart("daily", now)).toEqual(new Date(2027, 0, 1));
    expect(nextPeriodStart("hourly", now)).toEqual(new Date(2026, 11, 31, 23));
  });

  it("walks the list without repeating until it is used up", () => {
    const count = QUOTES.length;
    const seen = new Set(Array.from({ length: count }, (_, i) => indexForPeriod(20_000 + i, count)));
    expect(seen.size).toBe(count);
    expect(indexForPeriod(-1, 5)).toBe(4);
  });

  it("otherIndex never repeats the current one", () => {
    for (let i = 0; i < 50; i++) expect(otherIndex(3, 7)).not.toBe(3);
    expect(otherIndex(0, 1)).toBe(0);
  });

  it("ships plenty of short, attributed quotes", () => {
    expect(QUOTES.length).toBeGreaterThanOrEqual(60);
    for (const q of QUOTES) {
      expect(q.author).not.toBe("");
      expect(q.text.length).toBeLessThan(130);
    }
  });
});
