// @ts-check
// What the System widget shows, as data: one meter per switched-on metric (one per drive for
// disks), built from a stats sample and the history. No DOM, so it is unit-tested
// (packages/widget-sdk/test/system-meters.test.ts). main.js draws the meters as rings, bars or graphs.

import { HISTORY, axisMax, percent, percentOf, pushSample } from "./gauge.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */
/** @typedef {{ name: string; used: number; total: number }} Disk */
/** @typedef {NonNullable<Stats["network"]>} Network */
/** @typedef {"cpu" | "ram" | "gpu" | "net" | "disk" | "battery"} Kind */

/**
 * One thing to draw. `fraction` (0..1) is what a ring or bar fills; `history` what a graph draws
 * (already scaled to 0..1). `state` colors it: `warn` (nearly full, battery low), `charging`.
 * @typedef {{ id: string, kind: Kind, label: string, value: string, unit: string, detail: string,
 *   fraction: number, history: number[], state: "" | "warn" | "charging", icon: string }} Meter
 */

/** The order meters are shown in, and the setting that switches each on. */
export const KINDS = /** @type {const} */ (["cpu", "ram", "gpu", "net", "disk", "battery"]);

const GB = 1024 ** 3;
/** Bytes per second: idle noise stays flat on the network axis. */
export const NET_FLOOR = 256 * 1024;
/** From this share of used space a drive is "nearly full". */
export const NEARLY_FULL = 0.9;
/** At or below this (and not charging) the battery is low. */
export const LOW_BATTERY = 20;

/**
 * The drive letters of a `drives` setting ("C, d: E") as a set of upper-case letters.
 * @param {string} setting
 */
export function parseDrives(setting) {
  return new Set(
    setting
      .split(/[\s,;]+/)
      .map((token) => token.trim().charAt(0).toUpperCase())
      .filter((c) => /^[A-Z]$/.test(c)),
  );
}

/**
 * The drives to show, sorted by name. An empty filter keeps all of them.
 * @param {readonly Disk[]} disks
 * @param {string} setting
 */
export function pickDrives(disks, setting) {
  const wanted = parseDrives(setting);
  return disks.filter((d) => wanted.size === 0 || wanted.has(d.name.charAt(0).toUpperCase())).sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Battery as a state: `none` on desktops, else its percent and whether it charges, is full or low.
 * @param {Stats["battery"]} battery
 * @returns {{ kind: "none" } | { kind: "charging" | "full" | "discharging" | "low", percent: number }}
 */
export function batteryState(battery) {
  if (!battery) return { kind: "none" };
  const pct = Number.isFinite(battery.percent) ? Math.min(100, Math.max(0, Math.round(battery.percent))) : 0;
  if (battery.charging) return { kind: pct >= 100 ? "full" : "charging", percent: pct };
  return { kind: pct <= LOW_BATTERY ? "low" : "discharging", percent: pct };
}

/**
 * A connection's name and the line under it ("Home-5G", "Wi-Fi · 78%").
 * @param {Network | undefined} network
 */
export function describeConnection(network) {
  if (!network) return { name: "Network", sub: "" };
  if (network.kind === "wifi") {
    const signal = network.signal !== undefined && Number.isFinite(network.signal) ? Math.round(Math.min(100, Math.max(0, network.signal))) : undefined;
    return { name: network.name || "Wi-Fi", sub: signal === undefined ? "Wi-Fi" : `Wi-Fi · ${signal}%` };
  }
  if (network.kind === "ethernet") return { name: network.name || "Ethernet", sub: "Wired" };
  return { name: "Offline", sub: "No connection" };
}

/**
 * A byte count as a short number and its unit: `{ n: "612", unit: "GB" }` (as the kit's).
 * @param {number} bytes
 */
export function bytes(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = Math.max(0, bytes);
  let i = 0;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return { n: n < 10 && i > 0 ? n.toFixed(1) : String(Math.round(n)), unit: units[i] ?? "B" };
}

/** @param {number} perSecond */
const rate = (perSecond) => {
  const b = bytes(perSecond);
  return { n: b.n, unit: `${b.unit}/s` };
};

/** Samples kept per metric (and per drive). */
export function createHistory() {
  /** @type {Map<string, number[]>} */
  const series = new Map();
  return {
    /** @param {string} id @param {number} value */
    push(id, value) {
      let values = series.get(id);
      if (!values) series.set(id, (values = []));
      pushSample(values, value, HISTORY);
      return values;
    },
    /** @param {string} id */
    get: (id) => series.get(id) ?? [],
  };
}

/**
 * The meters for a sample, in display order, recording it in `history`. A metric the machine
 * doesn't have (no GPU counter, no battery) is left out.
 * @param {Stats} stats
 * @param {Record<Kind, boolean>} on
 * @param {ReturnType<typeof createHistory>} history
 * @param {{ drives: string }} opts
 * @returns {Meter[]}
 */
export function buildMeters(stats, on, history, { drives }) {
  /** @type {Meter[]} */
  const meters = [];
  /** @param {number} pct 0..100 */
  const pctOf = (pct) => ({ value: String(percent(pct)), unit: "%", fraction: percent(pct) / 100 });
  /** @param {string} id @param {number} pct */
  const scaled = (id, pct) => history.push(id, pct).map((v) => v / 100);

  if (on.cpu) {
    meters.push({ id: "cpu", kind: "cpu", label: "CPU", detail: "", state: "", icon: "cpu", history: scaled("cpu", stats.cpu), ...pctOf(stats.cpu) });
  }
  if (on.ram) {
    const pct = percentOf(stats.memory.used, stats.memory.total);
    const detail = `${(stats.memory.used / GB).toFixed(1)} of ${Math.round(stats.memory.total / GB)} GB`;
    meters.push({ id: "ram", kind: "ram", label: "Memory", detail, state: "", icon: "ram", history: scaled("ram", pct), ...pctOf(pct) });
  }
  if (on.gpu && stats.gpu !== undefined) {
    meters.push({ id: "gpu", kind: "gpu", label: "GPU", detail: "", state: "", icon: "gpu", history: scaled("gpu", stats.gpu), ...pctOf(stats.gpu) });
  }
  if (on.net) {
    const down = stats.net?.down ?? 0;
    const up = stats.net?.up ?? 0;
    const downs = history.push("net", down);
    const ups = history.push("net-up", up);
    const max = axisMax([...downs, ...ups], NET_FLOOR);
    const d = rate(down);
    const u = rate(up);
    const connection = describeConnection(stats.network);
    meters.push({
      id: "net",
      kind: "net",
      label: connection.name,
      value: d.n,
      unit: d.unit,
      detail: `↑ ${u.n} ${u.unit}`,
      fraction: Math.min(1, down / max),
      history: downs.map((v) => v / max),
      state: "",
      icon: stats.network?.kind === "ethernet" ? "ethernet" : stats.network?.kind === "none" ? "offline" : "wifi",
    });
  }
  if (on.disk) {
    for (const disk of pickDrives(stats.disks ?? [], drives)) {
      const pct = percentOf(disk.used, disk.total);
      const used = bytes(disk.used);
      const total = bytes(disk.total);
      meters.push({
        id: `disk-${disk.name}`,
        kind: "disk",
        label: disk.name,
        detail: `${used.n} ${used.unit} of ${total.n} ${total.unit}`,
        state: pct / 100 >= NEARLY_FULL ? "warn" : "",
        icon: "disk",
        history: [],
        ...pctOf(pct),
      });
    }
  }
  if (on.battery) {
    const battery = batteryState(stats.battery);
    if (battery.kind !== "none") {
      const charging = battery.kind === "charging" || battery.kind === "full";
      meters.push({
        id: "battery",
        kind: "battery",
        label: "Battery",
        detail: battery.kind === "full" ? "Plugged in" : charging ? "Charging" : battery.kind === "low" ? "Low" : "On battery",
        state: charging ? "charging" : battery.kind === "low" ? "warn" : "",
        icon: "battery",
        history: [],
        ...pctOf(battery.percent),
      });
    }
  }
  return meters;
}
