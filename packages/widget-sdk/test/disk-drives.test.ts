import { describe, expect, it } from "vitest";
import { driveUsage, parseDrives, pickDrives } from "../../../widgets/disk/drives.js";

const GB = 1024 ** 3;
const disks = [
  { name: "E:", used: 41 * GB, total: 238 * GB },
  { name: "C:", used: 612 * GB, total: 1000 * GB },
  { name: "D:", used: 1710 * GB, total: 1863 * GB },
];

describe("disk drives", () => {
  it("parses a letters setting loosely", () => {
    expect([...parseDrives("c, D: e;f  ")]).toEqual(["C", "D", "E", "F"]);
    expect(parseDrives("").size).toBe(0);
    expect(parseDrives("1, ?, ,").size).toBe(0);
  });

  it("sorts by name and filters by letter; empty keeps all", () => {
    expect(pickDrives(disks, "").map((d) => d.name)).toEqual(["C:", "D:", "E:"]);
    expect(pickDrives(disks, "e, c").map((d) => d.name)).toEqual(["C:", "E:"]);
    expect(pickDrives(disks, "z")).toEqual([]);
  });

  it("reports used or free, and survives odd numbers", () => {
    expect(driveUsage(disks[1]!, false)).toMatchObject({ bytes: 612 * GB, fraction: 0.612 });
    expect(driveUsage(disks[1]!, true)).toMatchObject({ bytes: 388 * GB });
    expect(driveUsage(disks[1]!, true).fraction).toBeCloseTo(0.388);
    expect(driveUsage({ name: "X:", used: 5, total: 0 }, false).fraction).toBe(0);
    expect(driveUsage({ name: "X:", used: 50, total: 10 }, false).fraction).toBe(1);
  });
});
