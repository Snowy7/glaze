// @ts-check
// The activity heatmap's pure logic: reading untrusted counts, merging sources, bucketing into
// intensity levels, laying days out as week columns or a month calendar, and streaks.
// Dates are local calendar days, keyed "YYYY-MM-DD" (what the todo and timer widgets write).

/** @typedef {Record<string, number>} Counts */
/** @typedef {"all" | "todo" | "timer"} Source */
/** @typedef {0 | 1 | 2 | 3 | 4} Level */
/** @typedef {{ key: string, day: number, month: number, future: boolean }} DayCell */
/** @typedef {{ key: string, day: number, future: boolean, today: boolean }} MonthCell */

const KEY = /^(\d{4})-(\d{2})-(\d{2})$/;
const MAX_COUNT = 9999;
const DAY_MS = 86_400_000;

/**
 * Local calendar day of a date, "YYYY-MM-DD".
 * @param {Date} date
 */
export function dateKey(date) {
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  return `${String(date.getFullYear()).padStart(4, "0")}-${mm}-${dd}`;
}

/**
 * The local date at midnight for a key, or null when it isn't a real calendar day.
 * @param {string} key
 * @returns {Date | null}
 */
export function parseKey(key) {
  const m = KEY.exec(key);
  if (!m) return null;
  const date = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return dateKey(date) === key ? date : null;
}

/** The date `days` after `date`, at midnight (DST-safe: it goes through the calendar). @param {Date} date @param {number} days */
export function addDays(date, days) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/**
 * Counts from the shared storage arrive as untrusted JSON: keep real days with a positive whole
 * count (capped), drop everything else.
 * @param {unknown} raw
 * @returns {Counts}
 */
export function readCounts(raw) {
  /** @type {Counts} */
  const out = {};
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return out;
  for (const [key, value] of Object.entries(raw)) {
    if (typeof value !== "number" || !Number.isFinite(value) || parseKey(key) === null) continue;
    const n = Math.min(MAX_COUNT, Math.floor(value));
    if (n > 0) out[key] = n;
  }
  return out;
}

/**
 * The counts to show: one source, or both added up.
 * @param {Counts} todo
 * @param {Counts} timer
 * @param {Source} source
 * @returns {Counts}
 */
export function mergeSources(todo, timer, source) {
  if (source === "todo") return todo;
  if (source === "timer") return timer;
  const out = { ...todo };
  for (const [key, n] of Object.entries(timer)) out[key] = (out[key] ?? 0) + n;
  return out;
}

/**
 * The count that means "a very busy day" for this data: the 85th percentile of the busy days, so one
 * outlier doesn't wash everything else out. At least 1.
 * @param {readonly number[]} values counts of the days on screen (zeros are ignored)
 */
export function scaleMax(values) {
  const busy = values.filter((n) => n > 0).sort((a, b) => a - b);
  if (busy.length === 0) return 1;
  return Math.max(1, busy[Math.min(busy.length - 1, Math.floor(busy.length * 0.85))] ?? 1);
}

/**
 * Intensity of a day, 0 (nothing) to 4. Small scales use the count itself (1 task is level 1, 2 is
 * level 2...); larger ones spread evenly up to `max`, and anything above it is level 4.
 * @param {number} count
 * @param {number} max from `scaleMax`
 * @returns {Level}
 */
export function levelOf(count, max) {
  if (!(count > 0)) return 0;
  const level = max <= 4 ? Math.ceil(count) : Math.ceil((4 * count) / max);
  return /** @type {Level} */ (Math.min(4, Math.max(1, level)));
}

/**
 * `weeks` columns of 7 days, the last one holding `today`. `weekStart` is the first row: 1 Monday,
 * 0 Sunday.
 * @param {Date} today
 * @param {number} weeks
 * @param {0 | 1} weekStart
 * @returns {DayCell[][]}
 */
export function weekGrid(today, weeks, weekStart) {
  const todayKey = dateKey(today);
  const back = (today.getDay() - weekStart + 7) % 7;
  const first = addDays(today, -back - (weeks - 1) * 7);
  return Array.from({ length: weeks }, (_, w) =>
    Array.from({ length: 7 }, (_, d) => {
      const date = addDays(first, w * 7 + d);
      const key = dateKey(date);
      return { key, day: date.getDate(), month: date.getMonth(), future: key > todayKey };
    }),
  );
}

/**
 * Where the month names go above week columns: the column where a month starts, at least 3 columns
 * apart (the first column only when its month still has room to show its name).
 * @param {readonly (readonly DayCell[])[]} grid
 * @param {number} [minGap] columns a name needs, more for narrow columns
 * @returns {{ col: number, month: number }[]}
 */
export function monthLabels(grid, minGap = 3) {
  /** @type {{ col: number, month: number }[]} */
  const starts = [];
  grid.forEach((week, col) => {
    const month = week[0]?.month;
    if (month !== undefined && (col === 0 || month !== grid[col - 1]?.[0]?.month)) starts.push({ col, month });
  });
  /** @type {{ col: number, month: number }[]} */
  const out = [];
  for (const [i, s] of starts.entries()) {
    const next = starts[i + 1];
    if (i === 0 && next && next.col - s.col < minGap) continue;
    const prev = out[out.length - 1];
    if (!prev || s.col - prev.col >= minGap) out.push(s);
  }
  return out;
}

/**
 * The month of `today` as calendar rows (weeks), `null` for the cells of the neighbouring months.
 * @param {Date} today
 * @param {0 | 1} weekStart
 * @returns {{ year: number, month: number, weeks: (MonthCell | null)[][] }}
 */
export function monthGrid(today, weekStart) {
  const year = today.getFullYear();
  const month = today.getMonth();
  const todayKey = dateKey(today);
  const lead = (new Date(year, month, 1).getDay() - weekStart + 7) % 7;
  const days = new Date(year, month + 1, 0).getDate();
  /** @type {(MonthCell | null)[]} */
  const cells = Array.from({ length: lead }, () => null);
  for (let day = 1; day <= days; day++) {
    const key = dateKey(new Date(year, month, day));
    cells.push({ key, day, future: key > todayKey, today: key === todayKey });
  }
  while (cells.length % 7 !== 0) cells.push(null);
  return { year, month, weeks: Array.from({ length: cells.length / 7 }, (_, w) => cells.slice(w * 7, w * 7 + 7)) };
}

/**
 * The last `n` days ending today, oldest first.
 * @param {Date} today
 * @param {number} n
 * @returns {{ key: string, date: Date }[]}
 */
export function recentDays(today, n) {
  return Array.from({ length: n }, (_, i) => {
    const date = addDays(today, i - (n - 1));
    return { key: dateKey(date), date };
  });
}

/** Whole days since the epoch for a local calendar day (immune to DST). @param {Date} date */
const dayNumber = (date) => Math.round(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS);

/**
 * Streaks of consecutive active days. The current streak is still alive when today has nothing yet
 * (it ends yesterday); days after today are ignored.
 * @param {Counts} counts
 * @param {Date} today
 * @returns {{ current: number, best: number, total: number }}
 */
export function streaks(counts, today) {
  const todayKey = dateKey(today);
  /** Day numbers (days since epoch, calendar-based) of active days up to today, ascending. */
  const days = Object.keys(counts)
    .filter((key) => key <= todayKey && (counts[key] ?? 0) > 0)
    .map((key) => dayNumber(parseKey(key) ?? today))
    .sort((a, b) => a - b);

  let best = 0;
  let run = 0;
  let last = Number.NEGATIVE_INFINITY;
  for (const day of days) {
    run = day === last + 1 ? run + 1 : 1;
    best = Math.max(best, run);
    last = day;
  }
  // `run` is now the streak ending at the latest active day; it is current if that day is today or yesterday.
  const current = days.length > 0 && dayNumber(today) - last <= 1 ? run : 0;
  let total = 0;
  for (const [key, n] of Object.entries(counts)) if (key <= todayKey) total += n;
  return { current, best, total };
}
