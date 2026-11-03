import { describe, expect, it } from "vitest";
import { mediaLabel, signalLevel, volumeDelta, volumeGlyph, workspaceSteps } from "./logic";

describe("top bar logic", () => {
  it("maps Wi-Fi signal to four levels", () => {
    expect([0, 25, 26, 50, 51, 75, 76, 100].map(signalLevel)).toEqual([1, 1, 2, 2, 3, 3, 4, 4]);
    expect(signalLevel(undefined)).toBe(4);
  });

  it("picks the speaker glyph", () => {
    expect(volumeGlyph({ level: 0.8, muted: true })).toBe("mute");
    expect(volumeGlyph({ level: 0, muted: false })).toBe("mute");
    expect(volumeGlyph({ level: 0.2, muted: false })).toBe("low");
    expect(volumeGlyph({ level: 0.5, muted: false })).toBe("mid");
    expect(volumeGlyph({ level: 0.9, muted: false })).toBe("high");
  });

  it("steps between virtual desktops", () => {
    expect(workspaceSteps(1, 1)).toBeNull();
    expect(workspaceSteps(3, 0)).toEqual({ direction: "left", count: 3 });
    expect(workspaceSteps(0, 2)).toEqual({ direction: "right", count: 2 });
  });

  it("labels media with or without an artist", () => {
    expect(mediaLabel({ title: "Weightless", artist: "Marconi Union" })).toBe("Weightless — Marconi Union");
    expect(mediaLabel({ title: "Podcast", artist: "" })).toBe("Podcast");
  });

  it("turns wheel notches into small volume steps (up is louder)", () => {
    expect(volumeDelta(-100)).toBeCloseTo(0.02);
    expect(volumeDelta(100)).toBeCloseTo(-0.02);
    expect(volumeDelta(-2000)).toBeCloseTo(0.06);
  });
});
