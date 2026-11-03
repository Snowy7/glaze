// @ts-check
import { $, onVisible, trackSettings, trackSize } from "./kit.js";

const defaults = { weekStart: "auto", weekNumbers: false };
let settings = defaults;

/** @typedef {"month" | "strip" | "tile"} Mode */
/** @type {Mode} */
let mode = "month";
/** How far the user has navigated: months in `month` mode, weeks in `strip` mode. */
let shift = 0;

const card = $("#card");
const view = $("#view");
const titleBtn = $("#title");
const badge = $("#badge");

const DAY_MS = 86_400_000;
/** @type {Record<string, number | undefined>} */
const FIRST_DAY = { sunday: 0, monday: 1, saturday: 6 };

const fmt = {
  title: new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }),
  short: new Intl.DateTimeFormat(undefined, { weekday: "short" }),
  narrow: new Intl.DateTimeFormat(undefined, { weekday: "narrow" }),
  long: new Intl.DateTimeFormat(undefined, { weekday: "long" }),
  month: new Intl.DateTimeFormat(undefined, { month: "short" }),
};

/**
 * @param {string} tag
 * @param {string} [className]
 * @param {string} [text]
 */
function h(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** The locale's first weekday (0 = Sunday), from Intl week info; Monday if the engine has none. */
const localeFirstDay = (() => {
  /** @type {number | undefined} */
  let cached;
  return () => {
    if (cached !== undefined) return cached;
    cached = 1;
    try {
      const locale = new Intl.Locale(navigator.language);
      const info = "getWeekInfo" in locale && typeof locale.getWeekInfo === "function" ? locale.getWeekInfo() : "weekInfo" in locale ? locale.weekInfo : null;
      if (info && typeof info === "object" && "firstDay" in info && typeof info.firstDay === "number") {
        cached = info.firstDay % 7; // Intl counts Monday = 1 .. Sunday = 7
      }
    } catch {
      // Unknown locale tag: keep Monday.
    }
    return cached;
  };
})();

const firstDayOfWeek = () => FIRST_DAY[settings.weekStart] ?? localeFirstDay();

/** @param {Date} date @param {number} days */
const addDays = (date, days) => new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);

/** @param {Date} a @param {Date} b */
const sameDay = (a, b) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** ISO 8601 week number of the week containing `date`. */
function isoWeek(/** @type {Date} */ date) {
  const thursday = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) + (4 - (date.getDay() || 7)) * DAY_MS;
  const year = new Date(thursday).getUTCFullYear();
  return Math.floor((thursday - Date.UTC(year, 0, 1)) / DAY_MS / 7) + 1;
}

/** Week number shown for a row starting on `rowStart`: the ISO number of the row's Thursday. */
function rowWeek(/** @type {Date} */ rowStart) {
  return isoWeek(addDays(rowStart, (4 - rowStart.getDay() + 7) % 7));
}

/**
 * One day: a number, in a circle when it is today.
 * @param {Date} date
 * @param {Date} today
 * @param {boolean} out outside the shown month
 */
function dayCell(date, today, out) {
  const cell = h("span", "d");
  cell.append(h("i", "", String(date.getDate())));
  if (out) cell.classList.add("out");
  if (sameDay(date, today)) {
    cell.classList.add("today");
    cell.setAttribute("aria-current", "date");
  }
  return cell;
}

/** Weekday label with a short and a narrow form; CSS picks by width. */
function weekdayLabel(/** @type {number} */ weekday) {
  const date = new Date(2024, 0, 7 + weekday); // 2024-01-07 is a Sunday
  const label = h("span", "wd caps");
  label.append(h("span", "l", fmt.short.format(date)), h("span", "n", fmt.narrow.format(date)));
  return label;
}

/** The title as "November 2026" in the locale's order, with the year quieter. */
function setTitle(/** @type {Date} */ date) {
  titleBtn.replaceChildren(
    ...fmt.title.formatToParts(date).map((part) => (part.type === "year" ? h("span", "yr", part.value) : document.createTextNode(part.value))),
  );
}

/** @param {Date} today */
function renderMonth(today) {
  const first = firstDayOfWeek();
  const month = new Date(today.getFullYear(), today.getMonth() + shift, 1);
  const start = addDays(month, -((month.getDay() - first + 7) % 7));
  const weekNumbers = settings.weekNumbers;
  setTitle(month);

  const cal = h("div", "cal");
  cal.style.setProperty("--cols", String(weekNumbers ? 8 : 7));
  if (weekNumbers) cal.append(h("span", "wd caps"));
  for (let c = 0; c < 7; c++) cal.append(weekdayLabel((first + c) % 7));
  for (let r = 0; r < 6; r++) {
    const rowStart = addDays(start, r * 7);
    if (weekNumbers) cal.append(h("span", "d wk num", String(rowWeek(rowStart))));
    for (let c = 0; c < 7; c++) {
      const date = addDays(rowStart, c);
      cal.append(dayCell(date, today, date.getMonth() !== month.getMonth()));
    }
  }
  view.replaceChildren(cal);
  badge.hidden = true;
}

/** @param {Date} today */
function renderStrip(today) {
  const first = firstDayOfWeek();
  const anchor = addDays(today, shift * 7);
  const start = addDays(anchor, -((anchor.getDay() - first + 7) % 7));
  setTitle(addDays(start, 3));

  const week = h("div", "week");
  for (let c = 0; c < 7; c++) {
    const date = addDays(start, c);
    const col = h("div", "sd");
    col.append(weekdayLabel(date.getDay()), dayCell(date, today, false));
    week.append(col);
  }
  view.replaceChildren(week);
  badge.hidden = !settings.weekNumbers;
  badge.textContent = `W${rowWeek(start)}`;
}

/** @param {Date} today */
function renderTile(today) {
  const tile = h("div", "tile");
  const wd = h("span", "t-wd caps ellipsis");
  wd.append(h("span", "l",fmt.long.format(today)), h("span", "s", fmt.short.format(today)));
  tile.append(
    wd,
    h("span", "t-num num", String(today.getDate())),
    h("span", "t-mo ellipsis", fmt.month.format(today)),
  );
  view.replaceChildren(tile);
}

function render() {
  const today = new Date();
  card.dataset.mode = mode;
  if (mode === "month") renderMonth(today);
  else if (mode === "strip") renderStrip(today);
  else renderTile(today);
}

// Sizes: a date tile when small, a week strip when short, the month otherwise.
trackSize(({ px: [width, height] }) => {
  /** @type {Mode} */
  const next = height < 130 ? (width < 150 ? "tile" : "strip") : width < 130 ? "tile" : "month";
  if (next === mode) return;
  mode = next;
  shift = 0;
  render();
});

/** @param {number} by */
function go(by) {
  shift += by;
  render();
}
$("#prev").addEventListener("click", () => go(-1));
$("#next").addEventListener("click", () => go(1));
titleBtn.addEventListener("click", () => {
  shift = 0;
  render();
});

// Re-render when the date rolls over, only while visible.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
function scheduleMidnight() {
  const now = new Date();
  const wait = addDays(now, 1).getTime() - now.getTime() + 50;
  timer = setTimeout(() => {
    render();
    scheduleMidnight();
  }, Math.min(wait, 2 ** 31 - 1));
}
onVisible(
  () => {
    render(); // the date may have changed while held
    scheduleMidnight();
  },
  () => clearTimeout(timer),
);

trackSettings(defaults, (next) => {
  settings = next;
  render();
});
