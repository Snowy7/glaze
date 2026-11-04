import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { KEEP_DAYS, bumpCounts, dayKey, readCounts } from "../../../widgets/todo/activity.js";

const noon = new Date(2026, 10, 4, 12, 0);

describe("activity counts", () => {
  it("names the local date, not the UTC one", () => {
    expect(dayKey(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
    expect(dayKey(new Date(2026, 11, 31, 0, 0))).toBe("2026-12-31");
  });

  it("keeps only real dates with positive whole counts from untrusted JSON", () => {
    const raw = { "2026-11-03": 2, "2026-02-30": 1, "2026-1-1": 1, "2026-11-01": -1, "2026-11-02": 1.5, "2026-10-31": "3", "2026-10-30": 0 };
    expect(readCounts(raw)).toEqual({ "2026-11-03": 2 });
    for (const junk of [null, undefined, "x", 7, [1, 2]]) expect(readCounts(junk)).toEqual({});
  });

  it("adds to a day and never goes below zero (a zero day disappears)", () => {
    expect(bumpCounts({ "2026-11-04": 1 }, "2026-11-04", 1, noon)).toEqual({ "2026-11-04": 2 });
    expect(bumpCounts(undefined, "2026-11-04", 1, noon)).toEqual({ "2026-11-04": 1 });
    expect(bumpCounts({ "2026-11-04": 1 }, "2026-11-04", -1, noon)).toEqual({});
    expect(bumpCounts({}, "2026-11-04", -1, noon)).toEqual({});
  });

  it("ignores a bad day and drops what the other writer broke", () => {
    expect(bumpCounts({ "2026-11-03": 4, nope: 2 }, "tomorrow", 1, noon)).toEqual({ "2026-11-03": 4 });
  });

  it("prunes days older than the retention window", () => {
    const oldest = dayKey(new Date(2026, 10, 4 - KEEP_DAYS));
    const before = dayKey(new Date(2026, 10, 4 - KEEP_DAYS - 1));
    const out = bumpCounts({ [oldest]: 2, [before]: 5 }, "2026-11-04", 1, noon);
    expect(out).toEqual({ [oldest]: 2, "2026-11-04": 1 });
  });

  it("is the same file in todo and timer", () => {
    const read = (id: string) => readFileSync(new URL(`../../../widgets/${id}/activity.js`, import.meta.url), "utf8");
    expect(read("timer")).toBe(read("todo"));
  });
});
