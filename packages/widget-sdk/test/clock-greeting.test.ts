import { describe, expect, it } from "vitest";
import { firstName, greet, periodOf, splitLead } from "../../../widgets/clock/greeting.js";

describe("the Greeting face's time of day", () => {
  it("changes at 5, 12, 17 and 21", () => {
    expect([0, 4, 5, 11, 12, 16, 17, 20, 21, 23].map(periodOf)).toEqual([
      "night", "night", "morning", "morning", "afternoon", "afternoon", "evening", "evening", "night", "night",
    ]);
  });
});

describe("the Greeting face's name", () => {
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

describe("the Greeting face's words", () => {
  it("greets by the part of the day, with the first name", () => {
    expect(greet("morning", "Ada Lovelace")).toEqual({ lead: "Good morning", name: "Ada" });
    expect(greet("night", "Ada")).toEqual({ lead: "Still up", name: "Ada" });
    expect(greet("afternoon", "")).toEqual({ lead: "Good afternoon", name: "" });
  });

  it("splits the greeting when there is no name to accent", () => {
    expect(splitLead("Good morning")).toEqual(["Good", "morning"]);
    expect(splitLead("Morning")).toEqual(["", "Morning"]);
  });
});
