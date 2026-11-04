// @ts-check
// Pure helpers for the clock widget: no DOM, so they are unit-tested
// (packages/widget-sdk/test/clock-format.test.ts).

/** @param {number} n */
const pad2 = (n) => String(n).padStart(2, "0");

/**
 * The digits to show. `padHours` keeps the leading zero in the 12-hour clock too (flip cards are
 * always two digits wide; a text clock reads better as "9:41").
 * @param {Date} date
 * @param {{ format24: boolean, padHours: boolean }} opts
 * @returns {{ h: string, m: string, s: string, period: "" | "AM" | "PM" }}
 */
export function timeParts(date, { format24, padHours }) {
  const hours = date.getHours();
  const h = format24 ? hours : hours % 12 || 12;
  return {
    h: format24 || padHours ? pad2(h) : String(h),
    m: pad2(date.getMinutes()),
    s: pad2(date.getSeconds()),
    period: format24 ? "" : hours < 12 ? "AM" : "PM",
  };
}

/**
 * The date as text, each in a long and a short form (CSS picks by width; they are equal when a
 * format has only one). `format` is the setting's value, so anything unknown is `full`:
 * "4 November 2026" / "4 Nov"; `short` is "4 Nov"; `numeric` the locale's "04/11/2026" / "04/11".
 * @param {Date} date
 * @param {string} format "full" | "short" | "numeric"
 * @param {string} [locale] browser default when omitted
 */
export function dateLabels(date, format, locale) {
  /** @param {Intl.DateTimeFormatOptions} opts */
  const f = (opts) => new Intl.DateTimeFormat(locale, opts).format(date);
  const abbreviated = f({ day: "numeric", month: "short" });
  const day =
    format === "short"
      ? { long: abbreviated, short: abbreviated }
      : format === "numeric"
        ? { long: f({ day: "2-digit", month: "2-digit", year: "numeric" }), short: f({ day: "2-digit", month: "2-digit" }) }
        : { long: f({ day: "numeric", month: "long", year: "numeric" }), short: abbreviated };
  return { weekday: { long: f({ weekday: "long" }), short: f({ weekday: "short" }) }, date: day };
}

/**
 * Milliseconds until the next second (or minute) boundary, plus a few to land just after it, so a
 * clock wakes exactly when what it shows changes.
 * @param {number} nowMs
 * @param {boolean} withSeconds
 */
export function msToNextTick(nowMs, withSeconds) {
  const step = withSeconds ? 1000 : 60_000;
  return step - (nowMs % step) + 5;
}

/**
 * How much to shrink a block that is `natural` px wide so it fits `available` px (never above 1).
 * Guards against a wide custom font overflowing the box.
 * @param {number} available
 * @param {number} natural
 */
export function fitScale(available, natural) {
  return natural > 0 && available > 0 ? Math.min(1, available / natural) : 1;
}

/**
 * A `font` setting as a CSS family list that falls back to the user's font; "" when unset. The
 * name is quoted, and quote characters are dropped so it cannot end the string.
 * @param {string} name
 */
export function fontFamily(name) {
  const clean = name.replace(/["']/g, "").trim();
  return clean ? `"${clean}", var(--font)` : "";
}
