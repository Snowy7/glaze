import { describe, expect, it } from "vitest";
import { topLayout } from "./layout";

describe("topLayout", () => {
  it("keeps the configured order", () => {
    expect(topLayout({ left: ["start", "app"], center: ["media"], right: ["clock", "power"] })).toEqual({
      left: ["start", "app"],
      center: ["media"],
      right: ["clock", "power"],
    });
  });

  it("drops unknown names and duplicates across zones", () => {
    expect(
      topLayout({
        left: ["start", "bogus", "clock"],
        center: ["clock", "media", "start"],
        right: ["volume", "volume", "Battery"],
      }),
    ).toEqual({ left: ["start", "clock"], center: ["media"], right: ["volume"] });
  });
});
