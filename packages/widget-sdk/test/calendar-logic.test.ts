import { describe, expect, it } from "vitest";
import {
  MAX_EVENTS,
  addDays,
  agenda,
  checkDraft,
  dayHeading,
  eventsByDate,
  firstWeekday,
  isoWeek,
  monthGrid,
  parseDate,
  parseEvents,
  pickMode,
  shiftMonth,
  sortEvents,
  weekStart,
  type CalEvent,
} from "../../../widgets/calendar/calendar-logic.js";

describe("calendar dates", () => {
  it("accepts only real dates", () => {
    expect(parseDate("2026-11-04")).toEqual({ year: 2026, month: 11, day: 4 });
    expect(parseDate("2028-02-29")).not.toBeNull();
    for (const bad of ["2026-02-29", "2026-13-01", "2026-11-31", "26-1-1", "", null, 20261104]) expect(parseDate(bad)).toBeNull();
  });

  it("adds days across months, years and leap days", () => {
    expect(addDays("2026-11-04", 30)).toBe("2026-12-04");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2028-02-28", 1)).toBe("2028-02-29");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("starts the week on the chosen day", () => {
    // 2026-11-04 is a Wednesday.
    expect(weekStart("2026-11-04", 1)).toBe("2026-11-02");
    expect(weekStart("2026-11-04", 0)).toBe("2026-11-01");
    expect(weekStart("2026-11-04", 6)).toBe("2026-10-31");
    expect(weekStart("2026-11-02", 1)).toBe("2026-11-02");
  });

  it("resolves the week-start setting, auto being the locale's", () => {
    expect(firstWeekday("monday", 0)).toBe(1);
    expect(firstWeekday("sunday", 1)).toBe(0);
    expect(firstWeekday("saturday", 1)).toBe(6);
    expect(firstWeekday("auto", 0)).toBe(0);
    expect(firstWeekday("whatever", 1)).toBe(1);
  });

  it("numbers weeks like ISO 8601, including the year boundaries", () => {
    expect(isoWeek("2026-11-04")).toBe(45);
    expect(isoWeek("2026-01-01")).toBe(1); // a Thursday
    expect(isoWeek("2027-01-01")).toBe(53); // a Friday: still 2026's last week
    expect(isoWeek("2024-12-30")).toBe(1); // a Monday: already 2025's first week
  });

  it("shifts by months across years, forwards and back", () => {
    expect(shiftMonth("2026-11-04", 0)).toEqual({ year: 2026, month: 11 });
    expect(shiftMonth("2026-11-04", 2)).toEqual({ year: 2027, month: 1 });
    expect(shiftMonth("2026-01-15", -1)).toEqual({ year: 2025, month: 12 });
    expect(shiftMonth("2026-01-15", -13)).toEqual({ year: 2024, month: 12 });
  });
});

describe("month grid", () => {
  it("is rows of 7 starting on the first weekday, with the days around the month", () => {
    const rows = monthGrid(2026, 11, 1); // November 2026 begins on a Sunday
    expect(rows).toHaveLength(6);
    expect(rows.every((r) => r.days.length === 7)).toBe(true);
    expect(rows[0]?.days[0]).toMatchObject({ date: "2026-10-26", inMonth: false });
    expect(rows[0]?.days[6]).toMatchObject({ date: "2026-11-01", inMonth: true, weekend: true });
    expect(rows[5]?.days[6]?.date).toBe("2026-12-06");
  });

  it("drops a last row that is all next month", () => {
    const rows = monthGrid(2026, 11, 0); // Sunday-first: Nov 29 to Dec 5 is the last row that has a day of November
    expect(rows).toHaveLength(5);
    expect(rows[4]?.days[0]?.date).toBe("2026-11-29");
  });

  it("has as many rows as the month needs: 4 for a Monday-first February starting on a Monday, 6 at most", () => {
    expect(monthGrid(2027, 2, 1)).toHaveLength(4); // 2027-02-01 is a Monday
    expect(monthGrid(2027, 2, 0)).toHaveLength(5);
    expect(monthGrid(2026, 8, 1)).toHaveLength(6); // 31 days from a Saturday
  });

  it("starts on Sunday when asked, and marks weekends by weekday", () => {
    const rows = monthGrid(2026, 11, 0);
    expect(rows[0]?.days[0]).toMatchObject({ date: "2026-11-01", weekend: true });
    expect(rows[0]?.days[1]).toMatchObject({ date: "2026-11-02", weekend: false });
  });

  it("labels a row with the week most of it belongs to", () => {
    const rows = monthGrid(2026, 11, 1);
    expect(rows.map((r) => r.week)).toEqual([44, 45, 46, 47, 48, 49]);
    // Sunday-first rows are numbered by their Thursday too.
    expect(monthGrid(2026, 11, 0).slice(0, 2).map((r) => r.week)).toEqual([45, 46]);
  });
});

describe("stored events are untrusted", () => {
  const good: CalEvent = { id: "a", date: "2026-11-04", time: "09:30", title: "Standup", color: 2 };

  it("keeps well-formed events and drops the rest", () => {
    const raw = [
      good,
      { id: "b", date: "2026-02-30", title: "Impossible day" },
      { id: "c", date: "2026-11-05", title: "   " },
      { id: "d", date: "2026-11-05" },
      { id: "", date: "2026-11-05", title: "No id" },
      { date: "2026-11-05", title: "Missing id" },
      "junk",
      null,
      42,
    ];
    expect(parseEvents(raw)).toEqual([good]);
  });

  it("drops a bad time or color but keeps the event", () => {
    const [event] = parseEvents([{ id: "x", date: "2026-11-04", time: "25:99", title: "T", color: 9 }]);
    expect(event).toEqual({ id: "x", date: "2026-11-04", title: "T" });
    expect(parseEvents([{ id: "x", date: "2026-11-04", time: "7:05", title: "T", color: 1.5 }])[0]).toEqual({ id: "x", date: "2026-11-04", title: "T" });
  });

  it("trims and caps titles, drops duplicate ids and caps the count", () => {
    expect(parseEvents([{ id: "x", date: "2026-11-04", title: `  ${"y".repeat(200)} ` }])[0]?.title).toHaveLength(80);
    expect(parseEvents([good, { ...good, title: "Again" }])).toHaveLength(1);
    const many = Array.from({ length: MAX_EVENTS + 20 }, (_, i) => ({ id: String(i), date: "2026-11-04", title: "e" }));
    expect(parseEvents(many)).toHaveLength(MAX_EVENTS);
  });

  it("anything that isn't a list is no events", () => {
    for (const raw of [undefined, null, {}, "x", 3]) expect(parseEvents(raw)).toEqual([]);
  });
});

describe("the add form", () => {
  const draft = { title: "  Dentist ", date: "2026-11-06", time: "14:00", color: 3 };

  it("makes an event from a good draft", () => {
    expect(checkDraft(draft, "id1")).toEqual({ ok: true, event: { id: "id1", date: "2026-11-06", time: "14:00", title: "Dentist", color: 3 } });
  });

  it("the time and color are optional", () => {
    expect(checkDraft({ ...draft, time: "", color: undefined }, "id1")).toEqual({ ok: true, event: { id: "id1", date: "2026-11-06", title: "Dentist" } });
  });

  it("says which field is wrong", () => {
    expect(checkDraft({ ...draft, title: "  " }, "i")).toEqual({ ok: false, error: "title" });
    expect(checkDraft({ ...draft, date: "2026-02-30" }, "i")).toEqual({ ok: false, error: "date" });
    expect(checkDraft({ ...draft, date: "" }, "i")).toEqual({ ok: false, error: "date" });
    expect(checkDraft({ ...draft, time: "24:00" }, "i")).toEqual({ ok: false, error: "time" });
  });
});

describe("the agenda", () => {
  const e = (id: string, date: string, title: string, time?: string): CalEvent => (time ? { id, date, title, time } : { id, date, title });
  const events = [
    e("1", "2026-11-05", "Lunch", "12:30"),
    e("2", "2026-11-04", "Standup", "09:30"),
    e("3", "2026-11-05", "Rent due"),
    e("4", "2026-11-03", "Yesterday"),
    e("5", "2026-11-04", "Dentist", "14:00"),
    e("6", "2026-11-09", "Later"),
  ];

  it("sorts by day, then time with all-day first, then title", () => {
    expect(sortEvents(events).map((x) => x.id)).toEqual(["4", "2", "5", "3", "1", "6"]);
  });

  it("groups events by day for the month's dots", () => {
    const byDate = eventsByDate(events);
    expect([...byDate.keys()]).toEqual(["2026-11-03", "2026-11-04", "2026-11-05", "2026-11-09"]);
    expect(byDate.get("2026-11-05")?.map((x) => x.title)).toEqual(["Rent due", "Lunch"]);
  });

  it("starts with today, drops the past, and keeps today even when empty", () => {
    const days = agenda(events, "2026-11-04", 10);
    expect(days.map((d) => d.date)).toEqual(["2026-11-04", "2026-11-05", "2026-11-09"]);
    expect(days[0]?.events.map((x) => x.title)).toEqual(["Standup", "Dentist"]);
    expect(agenda(events, "2026-11-06", 10).map((d) => d.date)).toEqual(["2026-11-06", "2026-11-09"]);
    expect(agenda(events, "2026-11-06", 10)[0]?.events).toEqual([]);
    expect(agenda([], "2026-11-04", 10)).toEqual([{ date: "2026-11-04", events: [] }]);
  });

  it("stops at the limit, in the middle of a day if need be", () => {
    const days = agenda(events, "2026-11-04", 3);
    expect(days.map((d) => d.events.length)).toEqual([2, 1]);
    expect(agenda(events, "2026-11-04", 0)).toEqual([{ date: "2026-11-04", events: [] }]);
  });

  it("names the days", () => {
    expect(dayHeading("2026-11-04", "2026-11-04", "en-GB")).toBe("Today");
    expect(dayHeading("2026-11-05", "2026-11-04", "en-GB")).toBe("Tomorrow");
    expect(dayHeading("2026-11-09", "2026-11-04", "en-GB")).toBe("Mon 9 Nov");
  });
});

describe("what to draw at a size", () => {
  it("month needs height: a strip when short, a tile when small", () => {
    expect(pickMode("month", 308, 312)).toBe("month");
    expect(pickMode("month", 200, 208)).toBe("month");
    expect(pickMode("month", 308, 100)).toBe("strip");
    expect(pickMode("month", 200, 100)).toBe("tile");
    expect(pickMode("month", 100, 100)).toBe("tile");
  });

  it("agenda needs height, strip is a strip", () => {
    expect(pickMode("agenda", 200, 208)).toBe("agenda");
    expect(pickMode("agenda", 308, 100)).toBe("tile");
    expect(pickMode("strip", 308, 100)).toBe("strip");
    expect(pickMode("strip", 100, 100)).toBe("tile");
  });
});
