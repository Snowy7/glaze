import { describe, expect, it } from "vitest";
import { boundAction, wheelTrigger } from "../actions";
import { anchorFor } from "../anchor";
import { groupsOf } from "./build";

describe("groupsOf", () => {
  it("keeps the configured order and splits groups at |", () => {
    expect(groupsOf(["start", "|", "workspaces", "app", "|", "|", "custom:temp"])).toEqual([
      ["start"],
      ["workspaces", "app"],
      ["custom:temp"],
    ]);
  });

  it("drops unknown names and duplicates across zones", () => {
    const seen = new Set<string>();
    expect(groupsOf(["start", "bogus" as never, "clock"], seen)).toEqual([["start", "clock"]]);
    expect(groupsOf(["clock", "media", "start", "custom:Bad Id" as never], seen)).toEqual([["media"]]);
  });
});

describe("actions", () => {
  it("binds only configured inputs, and `default` gives the module its own back", () => {
    const options = { click: { left: "notifications" as const, middle: "default" as const, wheelUp: { exec: "x.exe", args: [] } }, style: {} };
    expect(boundAction(options, "left")).toBe("notifications");
    expect(boundAction(options, "middle")).toBeUndefined();
    expect(boundAction(options, "right")).toBeUndefined();
    expect(boundAction(options, "wheelUp")).toEqual({ exec: "x.exe", args: [] });
    expect(boundAction(undefined, "left")).toBeUndefined();
  });

  it("reads the wheel's direction", () => {
    expect(wheelTrigger(-100)).toBe("wheelUp");
    expect(wheelTrigger(53)).toBe("wheelDown");
    expect(wheelTrigger(0)).toBeUndefined();
  });
});

describe("anchorFor", () => {
  const item: [number, number, number, number] = [300, 4, 40, 24];
  it("opens toward the middle of the screen, across the whole band", () => {
    expect(anchorFor(item, [0, 0, 1920, 32], "top")).toEqual({ rect: [300, 0, 40, 32], side: "below", origin: "50% 0" });
    expect(anchorFor(item, [0, 300, 1920, 48], "bottom")).toMatchObject({ rect: [300, 300, 40, 48], side: "above" });
    expect(anchorFor([4, 200, 40, 40], [0, 0, 48, 1080], "left")).toMatchObject({ rect: [0, 200, 48, 40], side: "right" });
  });
});
