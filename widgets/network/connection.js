// @ts-check
// Pure helpers for the network widget: how a connection is named and how strong its signal looks.

/** @typedef {NonNullable<Awaited<ReturnType<typeof mypaper.system.stats>>["network"]>} Network */

/**
 * Signal (0-100) as 1 to 4 lit marks of the wifi glyph (the dot, then three arcs). An unknown
 * signal shows the full glyph rather than a pessimistic one.
 * @param {number | undefined} signal
 * @returns {1 | 2 | 3 | 4}
 */
export function signalBars(signal) {
  if (signal === undefined || !Number.isFinite(signal)) return 4;
  if (signal >= 70) return 4;
  if (signal >= 45) return 3;
  if (signal >= 20) return 2;
  return 1;
}

/**
 * What to print for a connection: `name` is the headline, `sub` the small caps line under it.
 * @param {Network} network
 * @returns {{ name: string; sub: string }}
 */
export function describeConnection(network) {
  switch (network.kind) {
    case "wifi": {
      const pct = network.signal !== undefined && Number.isFinite(network.signal) ? Math.round(network.signal) : undefined;
      return { name: network.name || "Wi-Fi", sub: pct === undefined ? "Wi-Fi" : `Wi-Fi · ${Math.min(100, Math.max(0, pct))}%` };
    }
    case "ethernet":
      return { name: network.name || "Ethernet", sub: "Wired" };
    default:
      return { name: "Offline", sub: "" };
  }
}
