// @ts-check
// Pure helpers for the disk widget: which drives to show, and how full each one is.

/** @typedef {{ name: string; used: number; total: number }} Disk */

/**
 * The drive letters of a `drives` setting ("C, d: E") as a set of upper-case letters.
 * @param {string} setting
 * @returns {Set<string>}
 */
export function parseDrives(setting) {
  const letters = setting
    .split(/[\s,;]+/)
    .map((token) => token.trim().charAt(0).toUpperCase())
    .filter((c) => /^[A-Z]$/.test(c));
  return new Set(letters);
}

/**
 * The drives to show, sorted by name. An empty filter keeps all of them.
 * @param {readonly Disk[]} disks
 * @param {string} setting the `drives` setting
 * @returns {Disk[]}
 */
export function pickDrives(disks, setting) {
  const wanted = parseDrives(setting);
  return disks
    .filter((d) => wanted.size === 0 || wanted.has(d.name.charAt(0).toUpperCase()))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * What to draw for a drive: the bytes to print (used, or free), and the matching share 0..1.
 * @param {Disk} disk
 * @param {boolean} showFree
 * @returns {{ name: string; bytes: number; total: number; fraction: number }}
 */
export function driveUsage(disk, showFree) {
  const total = Math.max(0, disk.total);
  const used = Math.min(total, Math.max(0, disk.used));
  const usedFraction = total > 0 ? used / total : 0;
  return showFree
    ? { name: disk.name, bytes: total - used, total, fraction: total > 0 ? 1 - usedFraction : 0 }
    : { name: disk.name, bytes: used, total, fraction: usedFraction };
}
