import { describe, expect, it } from "vitest";
import { availableRows, fitRows, primaryDisk } from "../../../widgets/system-info/rows.js";

const all = { showGpu: true, showDisk: true, showNetwork: true, showBattery: true };
const full = { gpu: 9, disks: [{ name: "C:" }], network: { kind: "wifi" }, battery: { percent: 50, charging: false } };

describe("system-info rows", () => {
  it("shows only rows that have data and are allowed", () => {
    expect(availableRows(full, all)).toEqual(["uptime", "cpu", "ram", "gpu", "disk", "network", "battery"]);
    expect(availableRows({}, all)).toEqual(["uptime", "cpu", "ram"]); // a desktop without gpu, disks or network info
    expect(availableRows(full, { ...all, showGpu: false, showBattery: false })).toEqual([
      "uptime", "cpu", "ram", "disk", "network",
    ]);
  });

  it("keeps the most important rows when space is short, in display order", () => {
    const rows = availableRows(full, all);
    expect(fitRows(rows, 2)).toEqual(["cpu", "ram"]);
    expect(fitRows(rows, 4)).toEqual(["uptime", "cpu", "ram", "disk"]);
    expect(fitRows(rows, 99)).toEqual(rows);
    expect(fitRows(rows, 0)).toEqual([]);
  });

  it("picks C: as the primary drive, else the first by name", () => {
    expect(primaryDisk([{ name: "D:" }, { name: "C:" }])?.name).toBe("C:");
    expect(primaryDisk([{ name: "E:" }, { name: "D:" }])?.name).toBe("D:");
    expect(primaryDisk([])).toBeUndefined();
  });
});
