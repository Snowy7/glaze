import { describe, expect, it } from "vitest";
import { bestGrid, listColumns, signalBars, stateOf, TILES } from "../../../widgets/quick-toggles/toggles.js";

describe("quick toggles state", () => {
  it("maps signal strength to four bars, assuming full when unknown", () => {
    expect([100, 75, 74, 50, 49, 25, 24, 0].map(signalBars)).toEqual([4, 4, 3, 3, 2, 2, 1, 1]);
    expect(signalBars(undefined)).toBe(4);
  });

  it("lights Wi-Fi when connected and shows the network name", () => {
    expect(stateOf("wifi", { network: { kind: "wifi", name: "Home-5G", signal: 80 } })).toMatchObject({
      on: true,
      status: "Home-5G",
      bars: 4,
    });
    expect(stateOf("wifi", { network: { kind: "wifi" } })).toMatchObject({ on: true, status: "Connected" });
  });

  it("turns the Wi-Fi tile into Ethernet on a cable, and off without a connection", () => {
    expect(stateOf("wifi", { network: { kind: "ethernet", name: "Ethernet 2" } })).toMatchObject({
      on: true,
      label: "Ethernet",
      link: "ethernet",
    });
    expect(stateOf("wifi", { network: { kind: "none" } })).toMatchObject({ on: false, status: "Not connected" });
    expect(stateOf("wifi", {})).toMatchObject({ on: false, status: "Not connected" });
    // Before the first sample there is nothing to say yet.
    expect(stateOf("wifi", undefined)).toMatchObject({ on: false, status: "" });
  });

  it("shows the charge on the battery tile, lit while charging", () => {
    expect(stateOf("battery", { battery: { percent: 73.4, charging: false } })).toEqual({
      on: false,
      status: "73%",
      level: 0.73,
      charging: false,
    });
    expect(stateOf("battery", { battery: { percent: 100, charging: true } })).toMatchObject({ on: true, status: "100% · Charging" });
    expect(stateOf("battery", {})).toMatchObject({ on: false, status: "", label: "Battery saver" });
  });

  it("leaves the other tiles quiet", () => {
    for (const tile of TILES.filter((t) => t.id !== "wifi" && t.id !== "battery")) {
      expect(stateOf(tile.id, { network: { kind: "wifi" }, battery: { percent: 5, charging: true } })).toEqual({ on: false, status: "" });
    }
  });

  it("opens an ms-settings page for every tile", () => {
    expect(TILES.every((t) => t.uri.startsWith("ms-settings:"))).toBe(true);
    expect(new Set(TILES.map((t) => t.id)).size).toBe(TILES.length);
  });
});

describe("quick toggles layout", () => {
  it("picks the arrangement with the biggest tiles for the box", () => {
    expect(bestGrid(8, 408, 200, 8)).toEqual({ cols: 4, rows: 2 });
    expect(bestGrid(8, 200, 96, 8)).toEqual({ cols: 4, rows: 2 });
    expect(bestGrid(8, 408, 408, 8)).toEqual({ cols: 3, rows: 3 });
    expect(bestGrid(8, 100, 408, 8)).toEqual({ cols: 2, rows: 4 });
    expect(bestGrid(8, 60, 408, 8)).toEqual({ cols: 1, rows: 8 });
  });

  it("handles a single tile and none", () => {
    expect(bestGrid(1, 300, 100, 8)).toEqual({ cols: 1, rows: 1 });
    expect(bestGrid(0, 300, 100, 8)).toEqual({ cols: 1, rows: 1 });
  });

  it("keeps a list in one column until the rows get too short, then uses two if there is room", () => {
    expect(listColumns(8, 300, 400, 4)).toBe(1);
    expect(listColumns(8, 408, 200, 4)).toBe(2);
    expect(listColumns(8, 200, 96, 4)).toBe(2);
    expect(listColumns(8, 160, 200, 4)).toBe(1);
  });
});
