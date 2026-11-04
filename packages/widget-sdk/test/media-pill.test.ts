import { describe, expect, it } from "vitest";
import {
  appName,
  describe as describeTrack,
  formatClock,
  marqueeSeconds,
  needsMarquee,
  progressAt,
} from "../../../widgets/media-pill/pill.js";

describe("media pill text", () => {
  it("shows title and artist, falling back to the app and the album", () => {
    expect(describeTrack({ title: " Nightcall ", artist: " Kavinsky ", app: "Spotify.exe" })).toEqual({ title: "Nightcall", artist: "Kavinsky" });
    expect(describeTrack({ title: "", artist: "", album: "OutRun", app: "Spotify.exe" })).toEqual({ title: "Spotify", artist: "OutRun" });
    expect(describeTrack({ title: "  ", artist: "", app: "" })).toEqual({ title: "Unknown", artist: "" });
  });

  it("names an app from its executable", () => {
    expect(appName("chrome.EXE")).toBe("chrome");
    expect(appName("Windows Media Player")).toBe("Windows Media Player");
  });

  it("formats a clock", () => {
    expect([0, 5, 65, 3599, 3600, 3723, -4, Number.NaN].map(formatClock)).toEqual(["0:00", "0:05", "1:05", "59:59", "1:00:00", "1:02:03", "0:00", "0:00"]);
  });
});

describe("media pill marquee", () => {
  it("scrolls only text that really overflows", () => {
    expect(needsMarquee(180, 180)).toBe(false);
    expect(needsMarquee(180.6, 180)).toBe(false);
    expect(needsMarquee(220, 180)).toBe(true);
    expect(needsMarquee(220, 0)).toBe(false);
  });

  it("takes longer for a longer way, within bounds", () => {
    expect(marqueeSeconds(10)).toBe(4);
    expect(marqueeSeconds(120)).toBeGreaterThan(marqueeSeconds(60));
    expect(marqueeSeconds(120)).toBeCloseTo(5.6, 1);
    expect(marqueeSeconds(100_000)).toBe(60);
  });
});

describe("media pill progress", () => {
  const base = { position: 60, duration: 240, playing: true, at: 10_000 };

  it("moves on from the sample while playing", () => {
    expect(progressAt(base, 10_000)).toBeCloseTo(0.25);
    expect(progressAt(base, 40_000)).toBeCloseTo(0.375);
  });

  it("stands still while paused, and never passes the end", () => {
    expect(progressAt({ ...base, playing: false }, 99_000)).toBeCloseTo(0.25);
    expect(progressAt(base, 10_000_000)).toBe(1);
  });

  it("has no progress without a usable position and length", () => {
    expect(progressAt({ ...base, duration: undefined }, 10_000)).toBeNull();
    expect(progressAt({ ...base, position: undefined }, 10_000)).toBeNull();
    expect(progressAt({ ...base, duration: 0 }, 10_000)).toBeNull();
    expect(progressAt({ ...base, position: Number.NaN }, 10_000)).toBeNull();
  });
});
