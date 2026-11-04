// @ts-check
// Daily activity counts, shared with the activity heatmap widget through the `activity` storage
// namespace: each writer owns one key (`todo`, `timer`) holding `{ "YYYY-MM-DD": count }` in local
// dates. Identical copy in todo/ and timer/ (a widget can't import from outside its folder).

const NAMESPACE = "activity";
/** Older days are dropped whenever the object is written. */
export const KEEP_DAYS = 400;
const MAX_COUNT = 100_000;

const pad2 = (/** @type {number} */ n) => String(n).padStart(2, "0");

/**
 * The local calendar date as `YYYY-MM-DD`.
 * @param {Date} date
 */
export function dayKey(date) {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/**
 * A real calendar date in `YYYY-MM-DD` form (not `2026-02-30`, not `2026-2-3`).
 * @param {string} key
 */
function isDayKey(key) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return false;
  const [year, month, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const probe = new Date(Date.UTC(year, month - 1, day));
  return probe.getUTCFullYear() === year && probe.getUTCMonth() === month - 1 && probe.getUTCDate() === day;
}

/**
 * The stored value is untrusted JSON: keep real dates with positive whole-number counts.
 * @param {unknown} raw
 * @returns {Record<string, number>}
 */
export function readCounts(raw) {
  /** @type {Record<string, number>} */
  const counts = {};
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return counts;
  for (const [key, value] of Object.entries(raw)) {
    if (isDayKey(key) && typeof value === "number" && Number.isInteger(value) && value > 0) {
      counts[key] = Math.min(value, MAX_COUNT);
    }
  }
  return counts;
}

/**
 * The stored counts with `delta` added to `day` (never below 0; a zero count is removed) and
 * everything older than KEEP_DAYS before `now` dropped.
 * @param {unknown} raw
 * @param {string} day
 * @param {number} delta
 * @param {Date} [now]
 * @returns {Record<string, number>}
 */
export function bumpCounts(raw, day, delta, now = new Date()) {
  const counts = readCounts(raw);
  if (isDayKey(day)) {
    const next = Math.min(MAX_COUNT, Math.max(0, (counts[day] ?? 0) + delta));
    if (next > 0) counts[day] = next;
    else delete counts[day];
  }
  const oldest = dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - KEEP_DAYS));
  // ISO dates sort like the days they name.
  return Object.fromEntries(Object.entries(counts).filter(([key]) => key >= oldest));
}

/** Writes are queued so two quick changes can't read the same old value. */
let queue = Promise.resolve();

/**
 * Adds `delta` to this widget's count for `day` in the shared namespace. Never rejects: a host
 * that denies the permission (or has no shared storage) just means no history.
 * @param {"todo" | "timer"} key this widget's own key
 * @param {string} day
 * @param {number} delta
 */
export function recordActivity(key, day, delta) {
  queue = queue
    .then(async () => {
      const store = mypaper.storage.shared(NAMESPACE);
      await store.set(key, bumpCounts(await store.get(key), day, delta));
    })
    .catch((err) => console.warn(`${key}: could not record activity`, err));
}
