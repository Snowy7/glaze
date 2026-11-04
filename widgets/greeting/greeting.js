// @ts-check
// The greeting's pure logic: the time of day, the words for it, the name, and the secondary line.

/** @typedef {"morning" | "afternoon" | "evening" | "night"} Period */
/** @typedef {"friendly" | "formal" | "short"} Wording */
/** @typedef {"none" | "date" | "progress" | "custom"} Line */

/**
 * The part of the day: morning 5-11, afternoon 12-16, evening 17-20, night 21-4.
 * @param {number} hour 0 to 23
 * @returns {Period}
 */
export function periodOf(hour) {
  if (hour >= 5 && hour < 12) return "morning";
  if (hour >= 12 && hour < 17) return "afternoon";
  if (hour >= 17 && hour < 21) return "evening";
  return "night";
}

/**
 * The first word of a display name ("Alex K." gives "Alex", "Lovelace, Ada" gives "Ada"). An account
 * name that is all lower case ("you") is capitalised. "" when there is nothing usable.
 * @param {string} name
 */
export function firstName(name) {
  const trimmed = name.trim();
  // "Surname, Given" is the way some directories write a display name.
  const given = trimmed.includes(",") ? (trimmed.split(",")[1] ?? "").trim() : trimmed;
  const word = given.split(/\s+/)[0]?.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "") ?? "";
  if (word === "") return "";
  return word === word.toLowerCase() ? word.charAt(0).toUpperCase() + word.slice(1) : word;
}

/** @type {Record<Wording, Record<Period, string>>} */
const WORDS = {
  friendly: { morning: "Good morning", afternoon: "Good afternoon", evening: "Good evening", night: "Still up" },
  formal: { morning: "Good morning", afternoon: "Good afternoon", evening: "Good evening", night: "Good evening" },
  short: { morning: "Morning", afternoon: "Afternoon", evening: "Evening", night: "Night" },
};

/**
 * The greeting for a part of the day. `name` is the first name (friendly, short) or the whole display
 * name (formal); "" gives the greeting alone.
 * @param {Period} period
 * @param {Wording} wording
 * @param {string} displayName the full name, or whatever the user typed
 * @returns {{ lead: string, name: string }}
 */
export function greet(period, wording, displayName) {
  const name = wording === "formal" ? displayName.trim() : firstName(displayName);
  return { lead: WORDS[wording][period], name };
}

/**
 * Splits a greeting into the part before the accent word and the accent word itself, for when there
 * is no name to accent ("Good morning" gives "Good" and "morning").
 * @param {string} lead
 * @returns {[string, string]}
 */
export function splitLead(lead) {
  const at = lead.lastIndexOf(" ");
  return at < 0 ? ["", lead] : [lead.slice(0, at), lead.slice(at + 1)];
}

/**
 * How much of the day has gone, in whole percent (0 at midnight, 50 at noon).
 * @param {Date} date
 */
export function dayProgress(date) {
  return Math.floor(((date.getHours() * 60 + date.getMinutes()) / 1440) * 100);
}

/**
 * The line under the greeting.
 * @param {Line} kind
 * @param {Date} date
 * @param {string} custom
 * @param {string} [locale]
 */
export function secondaryLine(kind, date, custom, locale) {
  if (kind === "date") return date.toLocaleDateString(locale, { weekday: "long", day: "numeric", month: "long" });
  if (kind === "progress") return `${dayProgress(date)}% of the day`;
  if (kind === "custom") return custom.trim();
  return "";
}
