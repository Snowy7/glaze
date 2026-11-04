import { describe, expect, it } from "vitest";
import { fitGrid } from "../../../widgets/system-monitor/gauge.js";
import { batteryState, buildMeters, createHistory, describeConnection, pickDrives } from "../../../widgets/system-monitor/meters.js";

const GB = 1024 ** 3;
const stats = {
  cpu: 23,
  memory: { used: 8 * GB, total: 32 * GB },
  gpu: 40,
  net: { down: 2 * 1024 * 1024, up: 512 * 1024 },
  disks: [
    { name: "D:", used: 950 * GB, total: 1000 * GB },
    { name: "C:", used: 400 * GB, total: 1000 * GB },
  ],
  battery: { percent: 15, charging: false },
  uptime: 3600,
  network: { kind: "wifi" as const, name: "Home", signal: 78 },
};
const all = { cpu: true, ram: true, gpu: true, net: true, disk: true, battery: true };

describe("System meters", () => {
  it("shows what is switched on, in order, one meter per drive", () => {
    const meters = buildMeters(stats, all, createHistory(), { drives: "" });
    expect(meters.map((m) => m.id)).toEqual(["cpu", "ram", "gpu", "net", "disk-C:", "disk-D:", "battery"]);
    const [cpu, ram] = meters;
    expect([cpu?.value, cpu?.unit, cpu?.fraction]).toEqual(["23", "%", 0.23]);
    expect(ram?.detail).toBe("8.0 of 32 GB");
  });

  it("is what the replaced widgets were: one metric alone", () => {
    const only = (kind: keyof typeof all) => ({ cpu: false, ram: false, gpu: false, net: false, disk: false, battery: false, [kind]: true });
    const battery = buildMeters(stats, only("battery"), createHistory(), { drives: "" });
    expect(battery).toHaveLength(1);
    expect(battery[0]).toMatchObject({ label: "Battery", value: "15", state: "warn", detail: "Low" });
    const net = buildMeters(stats, only("net"), createHistory(), { drives: "" })[0];
    expect(net).toMatchObject({ label: "Home", value: "2.0", unit: "MB/s", detail: "↑ 512 KB/s", icon: "wifi" });
  });

  it("warns about a nearly full drive, filters drives and skips what the PC lacks", () => {
    const disks = buildMeters(stats, { ...all, cpu: false, ram: false, gpu: false, net: false, battery: false }, createHistory(), { drives: "d" });
    expect(disks.map((m) => [m.label, m.state])).toEqual([["D:", "warn"]]);
    const { gpu: _gpu, battery: _battery, ...desktop } = stats;
    expect(buildMeters(desktop, all, createHistory(), { drives: "" }).map((m) => m.kind)).not.toContain("battery");
    expect(buildMeters(desktop, all, createHistory(), { drives: "" }).map((m) => m.kind)).not.toContain("gpu");
  });

  it("keeps a history per metric, scaled to 0..1", () => {
    const history = createHistory();
    buildMeters(stats, all, history, { drives: "" });
    const cpu = buildMeters({ ...stats, cpu: 50 }, all, history, { drives: "" })[0];
    expect(cpu?.history).toEqual([0.23, 0.5]);
  });

  it("reads battery, drives and connections like the old widgets did", () => {
    expect(batteryState(undefined)).toEqual({ kind: "none" });
    expect(batteryState({ percent: 100, charging: true })).toEqual({ kind: "full", percent: 100 });
    expect(batteryState({ percent: 54.6, charging: true })).toEqual({ kind: "charging", percent: 55 });
    expect(pickDrives(stats.disks, "c, e").map((d) => d.name)).toEqual(["C:"]);
    expect(describeConnection({ kind: "ethernet" })).toEqual({ name: "Ethernet", sub: "Wired" });
    expect(describeConnection({ kind: "none" }).name).toBe("Offline");
  });
});

describe("fitGrid", () => {
  it("shows as many cells as fit, then the biggest, with a short last row allowed", () => {
    expect(fitGrid(5, 184, 184, { minW: 60, minH: 60 })).toEqual({ cols: 2, rows: 2, shown: 4 });
    expect(fitGrid(8, 272, 168, { minW: 60, minH: 60, ratio: 0.85 })).toEqual({ cols: 4, rows: 2, shown: 8 });
    expect(fitGrid(5, 272, 168, { minW: 60, minH: 60, ratio: 0.85 })).toMatchObject({ shown: 5 });
    expect(fitGrid(1, 80, 80, { minW: 60, minH: 60 })).toEqual({ cols: 1, rows: 1, shown: 1 });
    expect(fitGrid(3, 20, 20, { minW: 60, minH: 60 })).toEqual({ cols: 1, rows: 1, shown: 1 });
  });
});
