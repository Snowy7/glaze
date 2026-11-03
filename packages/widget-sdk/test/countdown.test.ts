import { describe, expect, it } from "vitest";
import { breakdown, parseDate, parseTime, phaseOf, progress, resolveTarget } from "../../../widgets/countdown/count.js";

const none = { date: "", time: "00:00", startDate: "" };

describe("countdown target", () => {
  it("accepts only real dates and 24-hour times", () => {
    expect(parseDate("2027-03-09")).toEqual({ year: 2027, month: 3, day: 9 });
    for (const bad of ["2027-02-30", "2027-13-01", "27-1-1", "", "tomorrow"]) expect(parseDate(bad)).toBeNull();
    expect(parseTime("7:05")).toEqual({ hours: 7, minutes: 5 });
    for (const bad of ["24:00", "12:60", "noon", ""]) expect(parseTime(bad)).toBeNull();
  });

  it("an empty or invalid date means the next January 1st", () => {
    const { target } = resolveTarget(none, new Date(2026, 10, 4, 12, 0));
    expect(target).toEqual(new Date(2027, 0, 1));
    expect(resolveTarget({ ...none, date: "2026-02-30" }, new Date(2026, 10, 4)).target).toEqual(new Date(2027, 0, 1));
  });

  it("on January 1st itself the target is still today, so the day is not skipped", () => {
    expect(resolveTarget(none, new Date(2027, 0, 1, 15, 0)).target).toEqual(new Date(2027, 0, 1));
    expect(resolveTarget(none, new Date(2027, 0, 2)).target).toEqual(new Date(2028, 0, 1));
  });

  it("combines date and time, falling back to midnight for a bad time", () => {
    const now = new Date(2026, 0, 1);
    expect(resolveTarget({ ...none, date: "2026-06-30", time: "18:30" }, now).target).toEqual(new Date(2026, 5, 30, 18, 30));
    expect(resolveTarget({ ...none, date: "2026-06-30", time: "99:99" }, now).target).toEqual(new Date(2026, 5, 30));
  });

  it("the ring starts at startDate when it is before the target, else a year earlier", () => {
    const now = new Date(2026, 0, 1);
    const base = { ...none, date: "2026-12-25" };
    expect(resolveTarget({ ...base, startDate: "2026-09-01" }, now).start).toEqual(new Date(2026, 8, 1));
    const target = new Date(2026, 11, 25);
    for (const startDate of ["", "nonsense", "2027-01-01"]) {
      expect(resolveTarget({ ...base, startDate }, now).start.getTime()).toBe(target.getTime() - 365 * 86_400_000);
    }
  });
});

describe("countdown arithmetic", () => {
  it("splits a duration", () => {
    expect(breakdown(((2 * 24 + 3) * 60 + 4) * 60_000 + 5_000)).toEqual({ days: 2, hours: 3, minutes: 4, seconds: 5 });
    expect(breakdown(-50)).toEqual({ days: 0, hours: 0, minutes: 0, seconds: 0 });
  });

  it("rounds up to the unit that is displayed, so a redraw just after the minute is not off by one", () => {
    // 2 minutes to go, redrawn 5 ms late.
    expect(breakdown(2 * 60_000 - 5, 60_000).minutes).toBe(2);
    expect(breakdown(2 * 60_000 - 5).minutes).toBe(1);
  });

  it("progress is clamped to 0..1", () => {
    const start = new Date(2026, 0, 1);
    const target = new Date(2026, 0, 11);
    expect(progress(start, target, new Date(2026, 0, 6))).toBe(0.5);
    expect(progress(start, target, new Date(2025, 0, 1))).toBe(0);
    expect(progress(start, target, new Date(2027, 0, 1))).toBe(1);
  });

  it("phases: before, the target's own day, after", () => {
    const target = new Date(2026, 5, 30, 18, 0);
    expect(phaseOf(new Date(2026, 5, 30, 17, 59), target)).toBe("before");
    expect(phaseOf(new Date(2026, 5, 30, 18, 0), target)).toBe("today");
    expect(phaseOf(new Date(2026, 5, 30, 23, 59), target)).toBe("today");
    expect(phaseOf(new Date(2026, 6, 1, 0, 0), target)).toBe("after");
  });
});
