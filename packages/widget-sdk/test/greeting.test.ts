import { describe, expect, it } from "vitest";
import { dayProgress, firstName, greet, periodOf, secondaryLine, splitLead } from "../../../widgets/greeting/greeting.js";

describe("greeting time of day", () => {
  it("changes at 5, 12, 17 and 21", () => {
    const at = (h: number) => periodOf(h);
    expect([0, 4, 5, 11, 12, 16, 17, 20, 21, 23].map(at)).toEqual([
      "night",
      "night",
      "morning",
      "morning",
      "afternoon",
      "afternoon",
      "evening",
      "evening",
      "night",
      "night",
    ]);
  });

  it("measures the day in whole percent", () => {
    expect(dayProgress(new Date(2026, 10, 4, 0, 0))).toBe(0);
    expect(dayProgress(new Date(2026, 10, 4, 12, 0))).toBe(50);
    expect(dayProgress(new Date(2026, 10, 4, 17, 36))).toBe(73);
    expect(dayProgress(new Date(2026, 10, 4, 23, 59))).toBe(99);
  });
});

describe("greeting name", () => {
  it("takes the first word of a display name", () => {
    expect(firstName("Ada Lovelace")).toBe("Ada");
    expect(firstName("  Alex K.  ")).toBe("Alex");
    expect(firstName("Lovelace, Ada")).toBe("Ada");
    expect(firstName("Zoë")).toBe("Zoë");
  });

  it("capitalises an account name and gives up on nothing usable", () => {
    expect(firstName("you")).toBe("You");
    expect(firstName("McKay")).toBe("McKay");
    for (const empty of ["", "   ", "...", "-"]) expect(firstName(empty)).toBe("");
  });
});

describe("greeting words", () => {
  it("has a friendly, a formal and a short wording", () => {
    expect(greet("morning", "friendly", "Ada Lovelace")).toEqual({ lead: "Good morning", name: "Ada" });
    expect(greet("evening", "formal", "Ada Lovelace")).toEqual({ lead: "Good evening", name: "Ada Lovelace" });
    expect(greet("morning", "short", "Ada Lovelace")).toEqual({ lead: "Morning", name: "Ada" });
    expect(greet("night", "friendly", "Ada")).toEqual({ lead: "Still up", name: "Ada" });
  });

  it("greets without a name", () => {
    expect(greet("afternoon", "friendly", "")).toEqual({ lead: "Good afternoon", name: "" });
  });

  it("splits the greeting when there is no name to accent", () => {
    expect(splitLead("Good morning")).toEqual(["Good", "morning"]);
    expect(splitLead("Morning")).toEqual(["", "Morning"]);
  });
});

describe("greeting secondary line", () => {
  const date = new Date(2026, 10, 4, 17, 36);

  it("shows the date, the progress, a custom text, or nothing", () => {
    expect(secondaryLine("date", date, "", "en-US")).toBe("Wednesday, November 4");
    expect(secondaryLine("progress", date, "")).toBe("73% of the day");
    expect(secondaryLine("custom", date, "  Ship it  ")).toBe("Ship it");
    expect(secondaryLine("none", date, "ignored")).toBe("");
  });
});
