// @ts-check
// Pure date and agenda logic for the calendar widget: no DOM, so it is unit-tested
// (packages/widget-sdk/test/calendar-logic.test.ts).
//
// Dates travel as "YYYY-MM-DD" strings (a calendar day, no time zone), and all arithmetic on them
// goes through UTC so a daylight-saving change can't move a day.

/**
 * An event, as stored under the `events` key.
 * @typedef {{ id: string, date: string, time?: string, title: string, color?: number }} CalEvent
 * @typedef {"month" | "agenda" | "strip"} Variant
 * @typedef {"month" | "agenda" | "strip" | "tile"} Mode
 */

export const MAX_EVENTS = 500;
export const MAX_TITLE = 80;
export const COLOR_COUNT = 5;

const DAY_MS = 86_400_000;
const DATE_RE = /^(\d{4})-(\d{2})-(\d{2})$/;
const TIME_RE = /^([01]\d|2[0-3]):([0-5]\d)$/;

/** @param {number} n */
const pad2 = (n) => String(n).padStart(2, "0");

/**
 * @param {number} year
 * @param {number} month 1 to 12
 * @param {number} day
 */
export const toDateString = (year, month, day) => `${String(year).padStart(4, "0")}-${pad2(month)}-${pad2(day)}`;

/** A local `Date` as "YYYY-MM-DD". @param {Date} date */
export const dateOf = (date) => toDateString(date.getFullYear(), date.getMonth() + 1, date.getDate());

/**
 * The parts of a real calendar date, or null ("2026-02-30" is not one).
 * @param {unknown} value
 */
export function parseDate(value) {
  const m = typeof value === "string" ? DATE_RE.exec(value) : null;
  if (!m) return null;
  const [year, month, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const check = new Date(Date.UTC(year, month - 1, day));
  if (check.getUTCFullYear() !== year || check.getUTCMonth() !== month - 1 || check.getUTCDate() !== day) return null;
  return { year, month, day };
}

/** UTC midnight of a valid date string, in ms. @param {string} date */
function utc(date) {
  const p = parseDate(date);
  return p ? Date.UTC(p.year, p.month - 1, p.day) : Number.NaN;
}

/**
 * @param {string} date
 * @param {number} days may be negative
 */
export function addDays(date, days) {
  const d = new Date(utc(date) + days * DAY_MS);
  return toDateString(d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate());
}

/** 0 = Sunday .. 6 = Saturday. @param {string} date */
export const weekdayOf = (date) => new Date(utc(date)).getUTCDay();

/** Whole days from `a` to `b`. @param {string} a @param {string} b */
export const daysBetween = (a, b) => Math.round((utc(b) - utc(a)) / DAY_MS);

/**
 * The first day of the week for the setting: monday, sunday and saturday are fixed, anything else
 * (auto) is the locale's.
 * @param {string} setting
 * @param {number} localeFirstDay 0 = Sunday
 */
export function firstWeekday(setting, localeFirstDay) {
  return setting === "monday" ? 1 : setting === "sunday" ? 0 : setting === "saturday" ? 6 : localeFirstDay;
}

/**
 * The start of the week containing `date`.
 * @param {string} date
 * @param {number} firstDay 0 = Sunday
 */
export function weekStart(date, firstDay) {
  return addDays(date, -((weekdayOf(date) - firstDay + 7) % 7));
}

/** ISO 8601 week number of the week containing `date`. @param {string} date */
export function isoWeek(date) {
  const thursday = addDays(date, 4 - (weekdayOf(date) || 7));
  const year = Number(thursday.slice(0, 4));
  return Math.floor(daysBetween(toDateString(year, 1, 1), thursday) / 7) + 1;
}

/**
 * The month as rows of 7 days (4 to 6 of them), starting on `firstDay`, with the days around it
 * that complete the rows (`inMonth` false). `week` is the number shown for each row: the ISO week
 * of its Thursday, so it is the week most of the row belongs to.
 * @param {number} year
 * @param {number} month 1 to 12
 * @param {number} firstDay 0 = Sunday
 */
export function monthGrid(year, month, firstDay) {
  const first = toDateString(year, month, 1);
  const start = weekStart(first, firstDay);
  const rows = Array.from({ length: 6 }, (_, r) => {
    const rowStart = addDays(start, r * 7);
    return {
      week: isoWeek(addDays(rowStart, (4 - weekdayOf(rowStart) + 7) % 7)),
      days: Array.from({ length: 7 }, (_, c) => {
        const date = addDays(rowStart, c);
        return { date, day: Number(date.slice(8)), inMonth: Number(date.slice(5, 7)) === month, weekend: [0, 6].includes(weekdayOf(date)) };
      }),
    };
  });
  // A last row that is all next month's days is dropped.
  return rows.filter((row) => row.days.some((d) => d.inMonth));
}

/**
 * The year and month `shift` months away from `date`'s.
 * @param {string} date
 * @param {number} shift
 */
export function shiftMonth(date, shift) {
  const p = parseDate(date);
  if (!p) return { year: 1970, month: 1 };
  const index = p.year * 12 + (p.month - 1) + shift;
  return { year: Math.floor(index / 12), month: (index % 12) + 1 };
}

// ---------------------------------------------------------------- events

/**
 * The events from storage, which is a JSON file the user can edit: keep the ones that are well
 * formed (a real date, a 24-hour time or none, a title, a color index or none, a unique id), at most
 * {@link MAX_EVENTS}, and drop the rest.
 * @param {unknown} raw
 * @returns {CalEvent[]}
 */
export function parseEvents(raw) {
  if (!Array.isArray(raw)) return [];
  /** @type {CalEvent[]} */
  const events = [];
  const seen = new Set();
  for (const item of raw) {
    if (events.length >= MAX_EVENTS) break;
    if (typeof item !== "object" || item === null) continue;
    const { id, date, time, title, color } = /** @type {Record<string, unknown>} */ (item);
    if (typeof id !== "string" || !id || seen.has(id)) continue;
    if (!parseDate(date) || typeof date !== "string") continue;
    const text = typeof title === "string" ? title.trim().slice(0, MAX_TITLE) : "";
    if (!text) continue;
    seen.add(id);
    /** @type {CalEvent} */
    const event = { id, date, title: text };
    if (typeof time === "string" && TIME_RE.test(time)) event.time = time;
    if (typeof color === "number" && Number.isInteger(color) && color >= 0 && color < COLOR_COUNT) event.color = color;
    events.push(event);
  }
  return events;
}

/**
 * Validates what the add form holds: the event to add, or why not.
 * @param {{ title: string, date: string, time: string, color: number | undefined }} draft
 * @param {string} id
 * @returns {{ ok: true, event: CalEvent } | { ok: false, error: "title" | "date" | "time" }}
 */
export function checkDraft(draft, id) {
  const title = draft.title.trim().slice(0, MAX_TITLE);
  if (!title) return { ok: false, error: "title" };
  if (!parseDate(draft.date)) return { ok: false, error: "date" };
  const time = draft.time.trim();
  if (time && !TIME_RE.test(time)) return { ok: false, error: "time" };
  /** @type {CalEvent} */
  const event = { id, date: draft.date, title };
  if (time) event.time = time;
  if (draft.color !== undefined && draft.color >= 0 && draft.color < COLOR_COUNT) event.color = draft.color;
  return { ok: true, event };
}

/**
 * By day, then by time (an event without a time is all-day and comes first), then by title.
 * @param {CalEvent[]} events
 */
export function sortEvents(events) {
  return [...events].sort((a, b) => a.date.localeCompare(b.date) || (a.time ?? "").localeCompare(b.time ?? "") || a.title.localeCompare(b.title));
}

/**
 * The events of each day, in time order, for the dots and the day lists.
 * @param {CalEvent[]} events
 * @returns {Map<string, CalEvent[]>}
 */
export function eventsByDate(events) {
  /** @type {Map<string, CalEvent[]>} */
  const byDate = new Map();
  for (const event of sortEvents(events)) {
    const list = byDate.get(event.date);
    if (list) list.push(event);
    else byDate.set(event.date, [event]);
  }
  return byDate;
}

/**
 * The agenda: today (always, even with nothing on) and then the days that have events, up to
 * `limit` events in total (a day is cut where the limit falls).
 * @param {CalEvent[]} events
 * @param {string} today
 * @param {number} limit
 * @returns {{ date: string, events: CalEvent[] }[]}
 */
export function agenda(events, today, limit) {
  const byDate = eventsByDate(events);
  let left = Math.max(0, limit);
  const days = [{ date: today, events: /** @type {CalEvent[]} */ ([]) }];
  const upcoming = [...byDate.keys()].filter((d) => d >= today).sort();
  for (const date of upcoming) {
    const list = byDate.get(date) ?? [];
    const take = list.slice(0, left);
    if (take.length === 0) break;
    left -= take.length;
    if (date === today) days[0] = { date, events: take };
    else days.push({ date, events: take });
  }
  return days;
}

/**
 * A day's heading: "Today", "Tomorrow", or the short weekday and date.
 * @param {string} date
 * @param {string} today
 * @param {string} [locale]
 */
export function dayHeading(date, today, locale) {
  const diff = daysBetween(today, date);
  if (diff === 0) return "Today";
  if (diff === 1) return "Tomorrow";
  return new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric", month: "short", timeZone: "UTC" }).format(utc(date));
}

// ---------------------------------------------------------------- layout

/**
 * What to draw at a size (CSS px): the variant when it fits, a week strip or a date tile when it
 * does not (a month needs height, an agenda too).
 * @param {Variant} variant
 * @param {number} width
 * @param {number} height
 * @returns {Mode}
 */
export function pickMode(variant, width, height) {
  if (width < 130 || height < 70) return "tile";
  if (variant === "month") return height < 170 ? (width < 240 ? "tile" : "strip") : "month";
  if (variant === "agenda") return height < 130 ? "tile" : "agenda";
  return "strip";
}
