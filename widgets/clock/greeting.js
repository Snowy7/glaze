// @ts-check
// The Greeting face's logic: the part of the day, the words for it and the name. No DOM, so it is
// unit-tested (packages/widget-sdk/test/clock-greeting.test.ts).

/** @typedef {"morning" | "afternoon" | "evening" | "night"} Period */

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
 * The first word of a display name ("Alex K." gives "Alex", "Lovelace, Ada" gives "Ada"). An
 * account name that is all lower case ("you") is capitalised. "" when there is nothing usable.
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

/** @type {Record<Period, string>} */
const WORDS = { morning: "Good morning", afternoon: "Good afternoon", evening: "Good evening", night: "Still up" };

/**
 * The greeting for a part of the day: the words, and the first name to accent ("" for none).
 * @param {Period} period
 * @param {string} displayName the full name, or whatever the user typed
 * @returns {{ lead: string, name: string }}
 */
export function greet(period, displayName) {
  return { lead: WORDS[period], name: firstName(displayName) };
}

/**
 * Splits a greeting into the part before the accent word and the accent word itself, for when
 * there is no name to accent ("Good morning" gives "Good" and "morning").
 * @param {string} lead
 * @returns {[string, string]}
 */
export function splitLead(lead) {
  const at = lead.lastIndexOf(" ");
  return at < 0 ? ["", lead] : [lead.slice(0, at), lead.slice(at + 1)];
}
