// Formats the clock event's timestamp with `config.top.clock.format` (docs/bars.md, "Modules").

import type { ClockConfig, SystemClock } from "./protocol";

/** Explicit token formats win; the default taskbar clock uses Windows' regional settings. */
export function clockText(config: ClockConfig, date: Date, system?: SystemClock | null, locale?: string): string {
  if (config.format !== null) return formatClock(config.format, date, locale);
  if (system) return `${config.seconds ? system.timeWithSeconds : system.time}\n${system.date}`;
  // Browser preview (or unavailable NLS): Intl uses the browser's system locale.
  return `${date.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit", ...(config.seconds ? { second: "2-digit" } : {}) })}\n${date.toLocaleDateString(locale)}`;
}

// Longest first, so "dddd" wins over "ddd" and "d".
const TOKENS = [
  "dddd", "MMMM", "yyyy", "ddd", "MMM", "dd", "MM", "yy", "HH", "hh", "mm", "ss", "tt", "d", "M", "H", "h",
] as const;
type Token = (typeof TOKENS)[number];

const pad = (n: number): string => String(n).padStart(2, "0");

function render(token: Token, date: Date, locale: string | undefined): string {
  switch (token) {
    case "dddd": return date.toLocaleDateString(locale, { weekday: "long" });
    case "ddd": return date.toLocaleDateString(locale, { weekday: "short" });
    case "dd": return pad(date.getDate());
    case "d": return String(date.getDate());
    case "MMMM": return date.toLocaleDateString(locale, { month: "long" });
    case "MMM": return date.toLocaleDateString(locale, { month: "short" });
    case "MM": return pad(date.getMonth() + 1);
    case "M": return String(date.getMonth() + 1);
    case "yyyy": return String(date.getFullYear());
    case "yy": return pad(date.getFullYear() % 100);
    case "HH": return pad(date.getHours());
    case "H": return String(date.getHours());
    case "hh": return pad(date.getHours() % 12 || 12);
    case "h": return String(date.getHours() % 12 || 12);
    case "mm": return pad(date.getMinutes());
    case "ss": return pad(date.getSeconds());
    case "tt": return date.getHours() < 12 ? "AM" : "PM";
  }
}

/**
 * Tokens: `dddd ddd d dd MMMM MMM MM M yyyy yy HH H hh h mm ss tt` (local time). `'text'` is
 * literal, `''` is a quote, and everything else is copied as is. `locale` only picks day and month names.
 */
export function formatClock(format: string, date: Date, locale?: string): string {
  let out = "";
  let i = 0;
  while (i < format.length) {
    const ch = format.charAt(i);
    if (ch === "'") {
      i++;
      if (format.charAt(i) === "'") {
        out += "'";
        i++;
        continue;
      }
      while (i < format.length) {
        if (format.charAt(i) === "'") {
          if (format.charAt(i + 1) === "'") {
            out += "'";
            i += 2;
            continue;
          }
          i++;
          break;
        }
        out += format.charAt(i++);
      }
      continue;
    }
    const token = TOKENS.find((t) => format.startsWith(t, i));
    if (token) {
      out += render(token, date, locale);
      i += token.length;
    } else {
      out += ch;
      i++;
    }
  }
  return out;
}
