// Pure helpers behind the smaller flyouts: the calendar's month grid and extra clocks, the tray's
// split between the bar and the overflow, relative times, and Wi-Fi signal bars.

import type { TrayItem } from "../protocol";
import type { CalendarConfig } from "./protocol";

// --- Calendar ---------------------------------------------------------------------------------

export interface Day {
  /** Local midnight of the day. */
  date: Date;
  inMonth: boolean;
  today: boolean;
}

const sameDay = (a: Date, b: Date): boolean =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** Six weeks of days around `month` (0–11) of `year`, weeks starting on `firstDay` (0 Sunday … 6 Saturday). */
export function monthGrid(year: number, month: number, firstDay: number, today: Date): Day[][] {
  const first = new Date(year, month, 1);
  const lead = (first.getDay() - firstDay + 7) % 7;
  const weeks: Day[][] = [];
  for (let w = 0; w < 6; w++) {
    const week: Day[] = [];
    for (let d = 0; d < 7; d++) {
      const date = new Date(year, month, 1 - lead + w * 7 + d);
      week.push({ date, inMonth: date.getMonth() === month, today: sameDay(date, today) });
    }
    weeks.push(week);
  }
  return weeks;
}

/** The week's first day: the config's, else the locale's (Intl week info), else Monday. */
export function firstDayOf(config: CalendarConfig["firstDay"], locale: string): number {
  if (config) return { sunday: 0, monday: 1, saturday: 6 }[config];
  try {
    const info = new Intl.Locale(locale) as Intl.Locale & {
      weekInfo?: { firstDay: number };
      getWeekInfo?(): { firstDay: number };
    };
    const first = info.getWeekInfo?.().firstDay ?? info.weekInfo?.firstDay;
    if (first !== undefined) return first % 7; // Intl counts Monday 1 … Sunday 7
  } catch {
    // An unknown locale: the default below.
  }
  return 1;
}

/** Minutes east of UTC that `zone` is at `at`. */
export function zoneOffset(at: Date, zone: string): number {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: zone,
    hourCycle: "h23",
    year: "numeric",
    month: "numeric",
    day: "numeric",
    hour: "numeric",
    minute: "numeric",
  }).formatToParts(at);
  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
  const asUtc = Date.UTC(get("year"), get("month") - 1, get("day"), get("hour"), get("minute"));
  return Math.round((asUtc - Math.floor(at.getTime() / 60_000) * 60_000) / 60_000);
}

/** "+5h", "−3:30", "same time": how far `zone` is from here (`localOffset` minutes east of UTC). */
export function offsetLabel(zoneMinutes: number, localMinutes: number): string {
  const diff = zoneMinutes - localMinutes;
  if (diff === 0) return "Same time";
  const sign = diff > 0 ? "+" : "−";
  const abs = Math.abs(diff);
  const hours = Math.floor(abs / 60);
  const minutes = abs % 60;
  return `${sign}${hours}${minutes ? `:${String(minutes).padStart(2, "0")}` : "h"}`;
}

/** A zone's label: the config's, else its city ("America/New_York" → "New York"). */
export function zoneLabel(zone: { zone: string; label?: string }): string {
  return zone.label ?? (zone.zone.split("/").pop() ?? zone.zone).replace(/_/g, " ");
}

// --- Tray ---------------------------------------------------------------------------------------

/**
 * What the bar shows and what's in the overflow. The host's items carry `pinned`; the UI
 * Automation fallback's `overflow` item is the native chevron, kept as the bar's last item.
 */
export function splitTray(items: readonly TrayItem[]): { bar: TrayItem[]; overflow: TrayItem[]; native: TrayItem | undefined } {
  const native = items.find((i) => i.id === "overflow");
  const rest = items.filter((i) => i.id !== "overflow");
  return { bar: rest.filter((i) => i.pinned), overflow: rest.filter((i) => !i.pinned), native };
}

// --- Time and signal -------------------------------------------------------------------------------

/** "Now", "5m", "3h", "Yesterday" or a short date, for a notification's age. */
export function ago(at: number, now: number, locale?: string): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return "Now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const then = new Date(at);
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (sameDay(then, yesterday)) return "Yesterday";
  return then.toLocaleDateString(locale, { day: "numeric", month: "short" });
}

/** Wi-Fi bars lit (1–4) for a signal quality 0–100. */
export function bars(signal: number): 1 | 2 | 3 | 4 {
  return signal <= 25 ? 1 : signal <= 50 ? 2 : signal <= 75 ? 3 : 4;
}
