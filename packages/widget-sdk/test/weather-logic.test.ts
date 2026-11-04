import { describe, expect, it } from "vitest";
import {
  dayLabel,
  degrees,
  errorInfo,
  placeRequest,
  readLocated,
  readPlace,
  hourLabel,
  pickLayout,
  placeNow,
  rangeBar,
  sceneFor,
  slotCount,
  upcomingHours,
  type Hour,
} from "../../../widgets/weather/weather-logic.js";

describe("weather layout", () => {
  it("one short row is mini, or a strip with hours when it is wide", () => {
    expect(pickLayout("card", 200, 100)).toBe("mini");
    expect(pickLayout("card", 308, 100)).toBe("strip");
    // Two rows need about 160px: a 3x2 card at scale 1.5 is 205 x 139.
    expect(pickLayout("card", 205, 139)).toBe("mini");
    expect(pickLayout("card", 308, 159)).toBe("strip");
    expect(pickLayout("card", 308, 160)).toBe("stack");
    expect(pickLayout("compact", 410, 100)).toBe("strip");
  });

  it("the card grows from stack to tall, and gets the days beside it when wide", () => {
    expect(pickLayout("card", 308, 208)).toBe("stack");
    expect(pickLayout("card", 308, 312)).toBe("tall");
    expect(pickLayout("card", 410, 208)).toBe("side");
    expect(pickLayout("card", 512, 312)).toBe("side");
  });

  it("compact never shows days, so it never goes tall or side", () => {
    expect(pickLayout("compact", 512, 312)).toBe("stack");
  });

  it("forecast is columns when short and rows when tall", () => {
    expect(pickLayout("forecast", 200, 100)).toBe("mini");
    expect(pickLayout("forecast", 308, 150)).toBe("strip");
    expect(pickLayout("forecast", 308, 208)).toBe("tall");
    expect(pickLayout("forecast", 200, 150)).toBe("tall");
  });

  it("counts the slots that fit, with gaps", () => {
    expect(slotCount(306, 46, 6)).toBe(6);
    expect(slotCount(305, 46, 6)).toBe(5);
    expect(slotCount(45, 46, 6)).toBe(0);
    expect(slotCount(0, 46)).toBe(0);
    expect(slotCount(96, 32)).toBe(3);
  });
});

describe("weather scenes", () => {
  it("maps plugin icons to illustration layers", () => {
    expect(sceneFor("clear-day").layers).toEqual(["sun"]);
    expect(sceneFor("clear-night").layers).toEqual(["moon", "stars"]);
    expect(sceneFor("partly-cloudy-night").scene).toBe("partly-night");
    expect(sceneFor("thunderstorm")).toMatchObject({ scene: "storm", tone: "dark" });
    expect(sceneFor("rain").layers).toContain("rain");
    expect(sceneFor("snow").layers).toContain("snow");
  });

  it("an unknown icon is a cloudy sky", () => {
    expect(sceneFor("hail").scene).toBe("cloudy");
  });

  it("takes how much falls from the WMO code", () => {
    expect(sceneFor("rain", 61).intensity).toBe("light");
    expect(sceneFor("rain", 63).intensity).toBe("normal");
    expect(sceneFor("rain", 65).intensity).toBe("heavy");
    expect(sceneFor("snow", 75).intensity).toBe("heavy");
    expect(sceneFor("drizzle", 55).intensity).toBe("light");
    expect(sceneFor("drizzle").intensity).toBe("light");
  });
});

describe("weather numbers and labels", () => {
  it("rounds temperatures and never shows -0", () => {
    expect(degrees(11.4)).toBe("11°");
    expect(degrees(-0.4)).toBe("0°");
    expect(degrees(-2.6)).toBe("-3°");
  });

  it("places a day's range on the week's scale", () => {
    expect(rangeBar(4, 10, 0, 20)).toEqual({ start: 20, size: 30 });
    expect(rangeBar(0, 20, 0, 20)).toEqual({ start: 0, size: 100 });
    // A day with no spread is still visible, and a bar never runs past the end.
    expect(rangeBar(20, 20, 0, 20)).toEqual({ start: 92, size: 8 });
    expect(rangeBar(5, 5, 5, 5)).toEqual({ start: 0, size: 100 });
  });

  it("names days and hours in the locale", () => {
    expect(dayLabel("2026-11-04", 0, "en-GB")).toBe("Today");
    expect(dayLabel("2026-11-05", 1, "en-GB")).toBe("Thu");
    expect(hourLabel("2026-11-04T14:00", "en-GB")).toBe("14");
    expect(hourLabel("2026-11-04T14:00", "en-US")).toBe("2 PM");
    expect(hourLabel("garbage")).toBe("");
  });

  it("explains an error by its code", () => {
    expect(errorInfo("NotFound", "Atlantis").title).toBe("Unknown town");
    expect(errorInfo("NotFound", "Atlantis").hint).toContain("Atlantis");
    expect(errorInfo("PluginUnavailable", "x").title).toBe("Weather unavailable");
    expect(errorInfo(undefined, "x").title).toBe("Couldn't load the weather");
  });
});

describe("the hourly strip", () => {
  const hour = (time: string): Hour => ({ time, temperature: 10, code: 0, condition: "Clear sky", icon: "clear-day", is_day: true });
  const hourly = ["2026-11-04T13:00", "2026-11-04T14:00", "2026-11-04T15:00", "2026-11-04T16:00"].map(hour);

  it("moves the place's clock forward from the fetch, whatever the user's time zone", () => {
    const fetchedAt = Date.UTC(2026, 10, 4, 9, 0);
    expect(placeNow("2026-11-04T14:00", fetchedAt, fetchedAt)).toBe("2026-11-04T14:00");
    expect(placeNow("2026-11-04T14:00", fetchedAt, fetchedAt + 75 * 60_000)).toBe("2026-11-04T15:15");
    // Across midnight.
    expect(placeNow("2026-11-04T23:00", fetchedAt, fetchedAt + 90 * 60_000)).toBe("2026-11-05T00:30");
    expect(placeNow("nonsense", fetchedAt, fetchedAt)).toBe("nonsense");
  });

  it("starts at the current hour and drops the ones already past", () => {
    expect(upcomingHours(hourly, "2026-11-04T14:20", 9).map((h) => h.time)).toEqual(["2026-11-04T14:00", "2026-11-04T15:00", "2026-11-04T16:00"]);
    expect(upcomingHours(hourly, "2026-11-04T14:20", 2)).toHaveLength(2);
    expect(upcomingHours(hourly, "2026-11-04T18:00", 5)).toEqual([]);
  });
});

describe("where the weather is for", () => {
  it("reads the location setting: automatic, a picked town, or an older city name", () => {
    expect(readPlace(null)).toBeNull();
    expect(readPlace({ name: " " })).toBeNull();
    expect(readPlace({ name: "Bergen", lat: 60.39, lon: 5.32, country: "NO" })).toEqual({ name: "Bergen", lat: 60.39, lon: 5.32 });
    expect(readPlace("Oslo")).toEqual({ name: "Oslo" });
    expect(readPlace({ name: "Oslo", lat: 59.9 })).toEqual({ name: "Oslo" });
  });

  it("asks by coordinates when it has them, else by name", () => {
    expect(placeRequest({ name: "Bergen", lat: 60.39, lon: 5.32 })).toEqual({ lat: 60.39, lon: 5.32 });
    expect(placeRequest({ name: "Oslo" })).toEqual({ city: "Oslo" });
  });

  it("takes a located place only with its coordinates", () => {
    expect(readLocated({ name: "Kensington", admin1: "England", lat: 51.5, lon: -0.19 })).toEqual({ name: "Kensington", lat: 51.5, lon: -0.19 });
    expect(readLocated({ name: "Somewhere" })).toBeNull();
    expect(readLocated(undefined)).toBeNull();
  });
});
