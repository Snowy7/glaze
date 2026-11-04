// @ts-check
// Quick toggles' pure logic: the tile catalogue, what live system data says about a tile, and how
// many columns a set of tiles should have in a box.

/** @typedef {Parameters<Parameters<typeof mypaper.system.subscribe>[1]>[0]} Stats */

/**
 * `uri` is the Settings page the tile opens. The tiles do not flip system state: Windows offers no
 * safe, permissionless way to do that from a widget, so a tile is a shortcut to its page.
 * @typedef {{ id: TileId, label: string, uri: string }} Tile
 * @typedef {"wifi" | "bluetooth" | "focus" | "airplane" | "nightlight" | "sound" | "display" | "battery"} TileId
 */

/** @type {readonly Tile[]} */
export const TILES = [
  { id: "wifi", label: "Wi-Fi", uri: "ms-settings:network-wifi" },
  { id: "bluetooth", label: "Bluetooth", uri: "ms-settings:bluetooth" },
  { id: "focus", label: "Focus", uri: "ms-settings:quiethours" },
  { id: "airplane", label: "Airplane", uri: "ms-settings:network-airplanemode" },
  { id: "nightlight", label: "Night light", uri: "ms-settings:nightlight" },
  { id: "sound", label: "Sound", uri: "ms-settings:sound" },
  { id: "display", label: "Display", uri: "ms-settings:display" },
  { id: "battery", label: "Battery", uri: "ms-settings:batterysaver" },
];

/**
 * @typedef {Object} TileState
 * @property {boolean} on lit: the system says this is active
 * @property {string} status a short live line ("Home-5G", "73% · Charging"), "" when there is none
 * @property {string} [label] replaces the tile's label (Wi-Fi becomes Ethernet on a cable)
 * @property {"wifi" | "ethernet"} [link] which network icon to draw
 * @property {number} [bars] Wi-Fi signal, 1 to 4
 * @property {number} [level] battery charge, 0 to 1
 * @property {boolean} [charging]
 */

/**
 * Wi-Fi bars for a signal strength in percent (4 when the host doesn't know).
 * @param {number | undefined} signal
 */
export function signalBars(signal) {
  if (signal === undefined || !Number.isFinite(signal)) return 4;
  return signal >= 75 ? 4 : signal >= 50 ? 3 : signal >= 25 ? 2 : 1;
}

/**
 * What the live stats say about a tile. Only Wi-Fi and the battery are live; the rest stay quiet.
 * @param {TileId} id
 * @param {Pick<Stats, "network" | "battery"> | undefined} stats `undefined` before the first sample
 * @returns {TileState}
 */
export function stateOf(id, stats) {
  if (id === "wifi") {
    const network = stats?.network;
    if (!network || network.kind === "none") return { on: false, status: stats ? "Not connected" : "", link: "wifi", bars: 4 };
    if (network.kind === "ethernet") return { on: true, status: network.name || "Connected", label: "Ethernet", link: "ethernet" };
    return { on: true, status: network.name || "Connected", link: "wifi", bars: signalBars(network.signal) };
  }
  if (id === "battery") {
    const battery = stats?.battery;
    if (!battery) return { on: false, status: "", label: stats ? "Battery saver" : "Battery" };
    const percent = Math.round(Math.min(100, Math.max(0, battery.percent)));
    return {
      on: battery.charging,
      status: `${percent}%${battery.charging ? " · Charging" : ""}`,
      level: percent / 100,
      charging: battery.charging,
    };
  }
  return { on: false, status: "" };
}

/**
 * Columns for `count` tiles in a `width` x `height` box: the count that makes the tiles biggest
 * (the smaller of their two sides), so a wide box gets a wide arrangement.
 * @param {number} count
 * @param {number} width
 * @param {number} height
 * @param {number} gap
 * @returns {{ cols: number, rows: number }}
 */
export function bestGrid(count, width, height, gap) {
  let best = { cols: 1, rows: Math.max(1, count) };
  let bestSide = Number.NEGATIVE_INFINITY;
  for (let cols = 1; cols <= Math.max(1, count); cols++) {
    const rows = Math.max(1, Math.ceil(count / cols));
    const side = Math.min((width - (cols - 1) * gap) / cols, (height - (rows - 1) * gap) / rows);
    if (side > bestSide + 0.5) {
      best = { cols, rows };
      bestSide = side;
    }
  }
  return best;
}

/**
 * Columns for a list of rows: one column unless the rows would get too short, and then two when the
 * box is wide enough for them.
 * @param {number} count
 * @param {number} width
 * @param {number} height
 * @param {number} gap
 * @param {number} [minRow] the shortest comfortable row
 */
export function listColumns(count, width, height, gap, minRow = 36) {
  const rowHeight = (/** @type {number} */ cols) => (height - (Math.ceil(count / cols) - 1) * gap) / Math.ceil(count / cols);
  if (rowHeight(1) >= minRow || width < 180) return 1;
  return 2;
}
