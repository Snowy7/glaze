// @ts-check

const DAY = 86_400;

/**
 * A headline's age as `now`, `5m`, `2h` or `3d`; a week or more shows its date (`12 Mar`, with the
 * year when it is not this one).
 * @param {number} published Unix seconds.
 * @param {number} now Unix seconds.
 * @param {Intl.LocalesArgument} [locale]
 */
export function relativeTime(published, now, locale) {
  const age = Math.floor(now - published);
  if (age < 60) return "now"; // includes clock skew (a date slightly in the future)
  if (age < 3600) return `${Math.floor(age / 60)}m`;
  if (age < DAY) return `${Math.floor(age / 3600)}h`;
  if (age < 7 * DAY) return `${Math.floor(age / DAY)}d`;
  const date = new Date(published * 1000);
  const sameYear = date.getFullYear() === new Date(now * 1000).getFullYear();
  return new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", ...(sameYear ? {} : { year: "numeric" }) }).format(date);
}
