// @ts-check
// The countdown's arithmetic: which moment is the target, what is left, how far along are we.
// All dates are the user's local time.

export const DAY_MS = 86_400_000;

/**
 * A real calendar date from `YYYY-MM-DD` (so `2026-02-30` is rejected), or null.
 * @param {string} value
 * @returns {{ year: number, month: number, day: number } | null} month is 1 to 12
 */
export function parseDate(value) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value.trim());
  if (!m) return null;
  const [year, month, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const probe = new Date(year, month - 1, day);
  return probe.getFullYear() === year && probe.getMonth() === month - 1 && probe.getDate() === day ? { year, month, day } : null;
}

/**
 * `HH:MM` (24 hour), or null.
 * @param {string} value
 * @returns {{ hours: number, minutes: number } | null}
 */
export function parseTime(value) {
  const m = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  if (!m) return null;
  const [hours, minutes] = [Number(m[1]), Number(m[2])];
  return hours < 24 && minutes < 60 ? { hours, minutes } : null;
}

/** Whether both dates fall on the same local calendar day. */
export function sameDay(/** @type {Date} */ a, /** @type {Date} */ b) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/**
 * The target moment and the start of the progress ring.
 * An empty or invalid `date` means the next January 1st (this year's during January 1st itself,
 * so the day is celebrated rather than skipped). An invalid `time` means midnight. The ring starts
 * at `startDate` when that is a valid date before the target, else a year before the target.
 * @param {{ date: string, time: string, startDate: string }} settings
 * @param {Date} now
 * @returns {{ target: Date, start: Date }}
 */
export function resolveTarget({ date, time, startDate }, now) {
  const { hours, minutes } = parseTime(time) ?? { hours: 0, minutes: 0 };
  const d = parseDate(date);
  let target;
  if (d) {
    target = new Date(d.year, d.month - 1, d.day, hours, minutes);
  } else {
    target = new Date(now.getFullYear(), 0, 1, hours, minutes);
    if (!sameDay(now, target) && target.getTime() <= now.getTime()) target = new Date(now.getFullYear() + 1, 0, 1, hours, minutes);
  }
  const s = parseDate(startDate);
  const from = s ? new Date(s.year, s.month - 1, s.day) : null;
  const start = from && from.getTime() < target.getTime() ? from : new Date(target.getTime() - 365 * DAY_MS);
  return { target, start };
}

/** @typedef {{ days: number, hours: number, minutes: number, seconds: number }} Parts */

/**
 * Splits a duration into days, hours, minutes and seconds. With `roundUpTo`, the duration is first
 * rounded up to that unit (60000 for a display without seconds), so a countdown that is redrawn
 * just after each minute shows the minutes still to go, not one fewer.
 * @param {number} ms
 * @param {number} [roundUpTo]
 * @returns {Parts}
 */
export function breakdown(ms, roundUpTo = 1) {
  const total = Math.floor((Math.ceil(Math.max(0, ms) / roundUpTo) * roundUpTo) / 1000);
  return {
    days: Math.floor(total / 86_400),
    hours: Math.floor((total % 86_400) / 3600),
    minutes: Math.floor((total % 3600) / 60),
    seconds: total % 60,
  };
}

/** How far along from `start` to `target`, 0 to 1. */
export function progress(/** @type {Date} */ start, /** @type {Date} */ target, /** @type {Date} */ now) {
  const span = target.getTime() - start.getTime();
  return span <= 0 ? 1 : Math.min(1, Math.max(0, (now.getTime() - start.getTime()) / span));
}

/**
 * Where we are relative to the target: `before` it, on its day (`today`, from the target moment
 * to midnight), or `after` its day.
 * @param {Date} now
 * @param {Date} target
 * @returns {"before" | "today" | "after"}
 */
export function phaseOf(now, target) {
  if (now.getTime() < target.getTime()) return "before";
  return sameDay(now, target) ? "today" : "after";
}
