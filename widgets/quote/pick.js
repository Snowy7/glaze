// @ts-check
// Which quote is "current": the rotation periods and the deterministic pick for a period.

/** @typedef {"daily" | "hourly" | "click"} Mode */

const HOUR_MS = 3_600_000;

/** Whole local days since 1970-01-01, by the local calendar date (so it flips at local midnight). */
function localDay(/** @type {Date} */ date) {
  return Math.round(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000);
}

/**
 * The rotation period `now` falls in: a number that changes when a new quote is due.
 * `click` never rotates by itself.
 * @param {Mode} mode
 * @param {Date} now
 */
export function periodOf(mode, now) {
  if (mode === "daily") return localDay(now);
  if (mode === "hourly") return localDay(now) * 24 + now.getHours();
  return 0;
}

/**
 * When the period after `now`'s begins, or null if it never does.
 * @param {Mode} mode
 * @param {Date} now
 * @returns {Date | null}
 */
export function nextPeriodStart(mode, now) {
  if (mode === "daily") return new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (mode === "hourly") {
    // Rebuilt from fields rather than `+ HOUR_MS`, which drifts across a DST change.
    const next = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours() + 1);
    return next.getTime() > now.getTime() ? next : new Date(now.getTime() + HOUR_MS);
  }
  return null;
}

/**
 * The quote index for a period: consecutive periods walk the list, so nothing repeats before the
 * list is used up.
 * @param {number} period
 * @param {number} count
 */
export function indexForPeriod(period, count) {
  if (count <= 0) return 0;
  return ((period % count) + count) % count;
}

/**
 * A random index that differs from `current` whenever there is a choice.
 * @param {number} current
 * @param {number} count
 * @param {() => number} [random]
 */
export function otherIndex(current, count, random = Math.random) {
  if (count <= 1) return 0;
  const step = 1 + Math.floor(random() * (count - 1));
  return (current + step) % count;
}
