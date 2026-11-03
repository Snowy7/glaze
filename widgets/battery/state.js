// @ts-check
// Pure battery logic, kept out of main.js so it can be tested.

/** At or below this (and not charging) the widget shows its "Low" emphasis. */
export const LOW_PERCENT = 20;

export const STYLES = /** @type {const} */ (["ring", "bar", "number"]);

/** @typedef {typeof STYLES[number]} Style */

/**
 * @typedef {{ kind: "none" }
 *   | { kind: "charging" | "full" | "discharging" | "low"; percent: number }} BatteryState
 * `full` is plugged in at 100%; `low` is on battery at or below LOW_PERCENT.
 */

/**
 * @param {{ percent: number; charging: boolean } | undefined} battery from `SystemStats.battery`
 * @returns {BatteryState}
 */
export function batteryState(battery) {
  if (!battery) return { kind: "none" };
  const percent = Number.isFinite(battery.percent) ? Math.min(100, Math.max(0, Math.round(battery.percent))) : 0;
  if (battery.charging) return { kind: percent >= 100 ? "full" : "charging", percent };
  return { kind: percent <= LOW_PERCENT ? "low" : "discharging", percent };
}

/**
 * The status line. No time estimate: the host does not report one, so none is invented.
 * @param {Exclude<BatteryState, { kind: "none" }>["kind"]} kind
 */
export function statusText(kind) {
  switch (kind) {
    case "charging":
      return "Charging";
    case "full":
      return "Plugged in";
    default:
      return "On battery";
  }
}

const DASH = 3;
const GAP = 3;

/**
 * `stroke-dasharray` for an arc on a circle with `pathLength="100"`: the arc covers `percent`,
 * solid, or (when `dashed`) as short dashes, the calm "running low" look.
 * @param {number} percent
 * @param {boolean} dashed
 */
export function ringDash(percent, dashed) {
  const p = Math.min(100, Math.max(0, percent));
  if (!dashed || p === 0) return `${p} ${100 - p}`;
  /** @type {number[]} */
  const parts = [];
  let at = 0;
  while (at < p) {
    const dash = Math.min(DASH, p - at);
    at += dash;
    // The last gap runs to the end of the circle so the pattern never repeats.
    parts.push(dash, at >= p ? 100 - at : GAP);
    at += GAP;
  }
  return parts.map((n) => Number(n.toFixed(2))).join(" ");
}

/**
 * @param {unknown} value a `style` setting (untrusted)
 * @returns {Style}
 */
export function styleOf(value) {
  return STYLES.find((s) => s === value) ?? "ring";
}
