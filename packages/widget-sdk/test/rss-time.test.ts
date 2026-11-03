import { describe, expect, it } from "vitest";
import { relativeTime } from "../../../widgets/rss/time.js";

const now = Date.UTC(2026, 10, 4, 12, 0) / 1000;

describe("rss relativeTime", () => {
  it("counts minutes, hours and days", () => {
    expect(relativeTime(now - 20, now)).toBe("now");
    expect(relativeTime(now - 5 * 60, now)).toBe("5m");
    expect(relativeTime(now - 3 * 3600 - 59, now)).toBe("3h");
    expect(relativeTime(now - 2 * 86_400, now)).toBe("2d");
  });

  it("treats a date in the future (clock skew) as now", () => {
    expect(relativeTime(now + 3600, now)).toBe("now");
  });

  it("switches to a short date after a week, with the year only when it differs", () => {
    expect(relativeTime(now - 8 * 86_400, now, "en-US")).toBe("Oct 27");
    expect(relativeTime(now - 400 * 86_400, now, "en-US")).toMatch(/2025/);
  });
});
