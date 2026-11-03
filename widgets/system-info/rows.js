// @ts-check
// Pure helpers for the system-info widget: which rows exist, and which of them fit.

/** @typedef {"uptime" | "cpu" | "ram" | "gpu" | "disk" | "network" | "battery"} RowId */

/** Top to bottom, as shown. */
export const ORDER = /** @type {const} */ (["uptime", "cpu", "ram", "gpu", "disk", "network", "battery"]);

/** What survives first when there is room for only a few rows. */
const PRIORITY = /** @type {const} */ (["cpu", "ram", "uptime", "disk", "gpu", "network", "battery"]);

/**
 * The rows that have data (and that the settings allow), in display order.
 * @param {{ gpu?: number; disks?: readonly unknown[]; network?: unknown; battery?: unknown }} stats
 * @param {{ showGpu: boolean; showDisk: boolean; showNetwork: boolean; showBattery: boolean }} settings
 * @returns {RowId[]}
 */
export function availableRows(stats, settings) {
  /** @type {Record<RowId, boolean>} */
  const has = {
    uptime: true,
    cpu: true,
    ram: true,
    gpu: settings.showGpu && stats.gpu !== undefined,
    disk: settings.showDisk && (stats.disks?.length ?? 0) > 0,
    network: settings.showNetwork && stats.network !== undefined,
    battery: settings.showBattery && stats.battery !== undefined,
  };
  return ORDER.filter((id) => has[id]);
}

/**
 * At most `capacity` of the rows: the most important ones, still in display order.
 * @param {readonly RowId[]} rows
 * @param {number} capacity
 * @returns {RowId[]}
 */
export function fitRows(rows, capacity) {
  const keep = new Set(PRIORITY.filter((id) => rows.includes(id)).slice(0, Math.max(0, capacity)));
  return rows.filter((id) => keep.has(id));
}

/**
 * The drive the "Disk" row talks about: C: when there is one, else the first by name.
 * @template {{ name: string }} D
 * @param {readonly D[]} disks
 * @returns {D | undefined}
 */
export function primaryDisk(disks) {
  return disks.find((d) => d.name.toUpperCase() === "C:") ?? [...disks].sort((a, b) => a.name.localeCompare(b.name))[0];
}
