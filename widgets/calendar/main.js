// @ts-check
import { $, onVisible, trackSettings, trackSize } from "./kit.js";
import {
  COLOR_COUNT,
  MAX_TITLE,
  addDays,
  agenda,
  checkDraft,
  dateOf,
  dayHeading,
  eventsByDate,
  firstWeekday,
  isoWeek,
  monthGrid,
  parseEvents,
  pickMode,
  shiftMonth,
  weekStart,
  weekdayOf,
} from "./calendar-logic.js";

/** @typedef {import("./calendar-logic.js").CalEvent} CalEvent */
/** @typedef {import("./calendar-logic.js").Variant} Variant */
/** @typedef {import("./calendar-logic.js").Mode} Mode */

const VARIANTS = ["month", "agenda", "strip"];
const defaults = { variant: "month", weekStart: "auto", weekNumbers: false };
let settings = defaults;
/** @type {Variant} */
let variant = "month";
/** @type {Mode} */
let mode = "month";
let box = { width: 0, height: 0 };
/** How far the user has navigated: months in `month` mode, weeks in `strip` mode. */
let shift = 0;
/** The day picked in the month or strip ("" = today); the add form starts with it. */
let selected = "";
/** The add form is open (agenda mode). */
let adding = false;
/** @type {CalEvent[]} */
let events = [];
let byDate = eventsByDate(events);

const card = $("#card");
const view = $("#view");
const titleBtn = $("#title");
const badge = $("#badge");

const fmt = {
  title: new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }),
  month: new Intl.DateTimeFormat(undefined, { month: "short" }),
  short: new Intl.DateTimeFormat(undefined, { weekday: "short" }),
  narrow: new Intl.DateTimeFormat(undefined, { weekday: "narrow" }),
  long: new Intl.DateTimeFormat(undefined, { weekday: "long" }),
  dayMonth: new Intl.DateTimeFormat(undefined, { day: "numeric", month: "long" }),
};

/**
 * @template {keyof HTMLElementTagNameMap} K
 * @param {K} tag
 * @param {string} [className]
 * @param {string} [text]
 * @returns {HTMLElementTagNameMap[K]}
 */
function h(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** A local-time Date for a day string (for Intl). @param {string} date */
function localDate(date) {
  const [y = 1970, m = 1, d = 1] = date.split("-").map(Number);
  return new Date(y, m - 1, d);
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
const firstDay = () => firstWeekday(settings.weekStart, localeFirstDay());

/** The event's color as a CSS value: the style's highlight unless the user picked one. @param {number | undefined} color */
const colorVar = (color) => (color === undefined ? "var(--hi)" : `var(--ev-${color})`);

// ---------------------------------------------------------------- pieces

/**
 * One day: its number (a disc when it is today, a ring when picked) and a dot per event.
 * @param {string} date
 * @param {string} today
 * @param {{ out?: boolean, weekend?: boolean }} [flags]
 */
function dayCell(date, today, flags = {}) {
  const cell = h("button", "d");
  cell.type = "button";
  cell.dataset.date = date;
  cell.append(h("i", "", String(Number(date.slice(8)))));
  if (flags.out) cell.classList.add("out");
  if (flags.weekend) cell.classList.add("we");
  if (date === today) {
    cell.classList.add("today");
    cell.setAttribute("aria-current", "date");
  }
  if (date === selected) cell.classList.add("sel");
  const list = byDate.get(date);
  if (list) {
    const dots = h("span", "dots");
    for (const ev of list.slice(0, 3)) {
      const dot = h("b", "glow");
      dot.style.setProperty("--c", colorVar(ev.color));
      dots.append(dot);
    }
    cell.append(dots);
    cell.title = list.map((ev) => `${ev.time ?? ""} ${ev.title}`.trim()).join("\n");
  }
  return cell;
}

/** Weekday label with a short and a narrow form; CSS picks by width. @param {number} weekday */
function weekdayLabel(weekday) {
  const date = new Date(2024, 0, 7 + weekday); // 2024-01-07 is a Sunday
  const label = h("span", "wd caps");
  label.append(h("span", "l", fmt.short.format(date)), h("span", "n", fmt.narrow.format(date)));
  return label;
}

/** The title as "November 2026" in the locale's order, with the year quieter. @param {Date} date */
function setTitle(date) {
  titleBtn.replaceChildren(
    ...fmt.title.formatToParts(date).map((part) => (part.type === "year" ? h("span", "yr", part.value) : document.createTextNode(part.value))),
  );
}

/**
 * One event line: a dot in its color, the time, the title and, in the agenda, a delete button.
 * @param {CalEvent} ev
 * @param {boolean} deletable
 */
function eventRow(ev, deletable) {
  const row = h("li", "ev");
  row.style.setProperty("--c", colorVar(ev.color));
  row.append(h("i", "dot glow"), h("span", ev.time ? "t num" : "t num all", ev.time ?? "All day"), h("span", "x ellipsis", ev.title));
  if (deletable) {
    const del = h("button", "del", "×");
    del.type = "button";
    del.title = "Delete";
    del.setAttribute("aria-label", `Delete ${ev.title}`);
    del.dataset.id = ev.id;
    row.append(del);
  }
  return row;
}

/**
 * A day's events under a small heading, or a quiet line when there are none.
 * @param {string} date
 * @param {string} today
 */
function dayList(date, today) {
  const section = h("section", "day-events");
  section.append(h("div", "gh caps", dayHeading(date, today)));
  const list = byDate.get(date) ?? [];
  if (list.length === 0) {
    section.append(h("div", "none", "Nothing planned"));
  } else {
    const ul = h("ul", "evs");
    ul.append(...list.map((ev) => eventRow(ev, false)));
    section.append(ul);
  }
  return section;
}

// ---------------------------------------------------------------- the modes

/** @param {string} today */
function renderMonth(today) {
  const first = firstDay();
  const { year, month } = shiftMonth(today, shift);
  setTitle(new Date(year, month - 1, 1));
  const cal = h("div", "cal");
  cal.style.setProperty("--cols", String(settings.weekNumbers ? 8 : 7));
  if (settings.weekNumbers) cal.append(h("span", "wd caps"));
  for (let c = 0; c < 7; c++) cal.append(weekdayLabel((first + c) % 7));
  const rows = monthGrid(year, month, first);
  cal.style.setProperty("--rows", String(rows.length));
  for (const row of rows) {
    if (settings.weekNumbers) cal.append(h("span", "wk caps num", String(row.week)));
    for (const day of row.days) cal.append(dayCell(day.date, today, { out: !day.inMonth, weekend: day.weekend }));
  }
  view.replaceChildren(cal);
  // With room under the grid, the picked day (or today) lists its events.
  if (box.height >= 340) view.append(dayList(selected || today, today));
  badge.hidden = true;
}

/** @param {string} today */
function renderStrip(today) {
  const start = weekStart(addDays(today, shift * 7), firstDay());
  setTitle(localDate(addDays(start, 3)));
  const week = h("div", "week");
  for (let c = 0; c < 7; c++) {
    const date = addDays(start, c);
    const col = h("div", "sd");
    col.append(weekdayLabel(weekdayOf(date)), dayCell(date, today, { weekend: [0, 6].includes(weekdayOf(date)) }));
    week.append(col);
  }
  view.replaceChildren(week);
  // Below the week: the picked day's events, when there is height for them.
  if (box.height >= 160) view.append(dayList(selected || today, today));
  badge.hidden = !settings.weekNumbers;
  badge.textContent = `W${isoWeek(addDays(start, 3))}`;
}

/** Today's date as a tile; the agenda variant adds what is next. @param {string} today */
function renderTile(today) {
  const date = localDate(today);
  const tile = h("div", "tile-box");
  const wd = h("span", "t-wd caps ellipsis");
  wd.append(h("span", "l", fmt.long.format(date)), h("span", "s", fmt.short.format(date)));
  tile.append(wd, h("span", "t-num hero num", String(date.getDate())), h("span", "t-mo ellipsis", fmt.month.format(date)));
  if (variant === "agenda") {
    const next = agenda(events, today, 1).flatMap((day) => day.events)[0];
    const line = h("div", "t-next ellipsis");
    if (next) {
      line.style.setProperty("--c", colorVar(next.color));
      line.append(h("i", "dot glow"), h("span", "x", `${next.time ? `${next.time} ` : ""}${next.title}`));
    } else line.textContent = "Nothing planned";
    tile.append(line);
  }
  view.replaceChildren(tile);
}

/** @param {string} today */
function renderAgenda(today) {
  const date = localDate(today);
  titleBtn.replaceChildren(h("span", "wd-big", fmt.long.format(date)), h("span", "dm", fmt.dayMonth.format(date)));
  badge.hidden = true;
  if (adding) return renderForm(today);

  const days = agenda(events, today, 60);
  const planned = days.some((day) => day.events.length > 0);
  if (!planned) {
    const empty = h("div", "empty");
    empty.append(h("div", "e-title", "Nothing planned"), h("div", "e-hint", "Add an event with the plus."));
    view.replaceChildren(empty);
    return;
  }
  const list = h("div", "agenda");
  for (const day of days) {
    if (day.events.length === 0) continue;
    const ul = h("ul", "evs");
    ul.append(...day.events.map((ev) => eventRow(ev, true)));
    list.append(h("div", "gh caps", dayHeading(day.date, today)), ul);
  }
  view.replaceChildren(list);
}

/** The inline add form (title, date, optional time and color). @param {string} today */
function renderForm(today) {
  const form = h("div", "form");
  const title = h("input", "field");
  title.name = "title";
  title.placeholder = "Event title";
  title.maxLength = MAX_TITLE;
  title.autocomplete = "off";
  const date = h("input", "field");
  date.name = "date";
  date.type = "date";
  date.value = selected || today;
  const time = h("input", "field");
  time.name = "time";
  time.type = "time";
  const when = h("div", "when");
  when.append(date, time);

  const swatches = h("div", "swatches");
  swatches.setAttribute("role", "radiogroup");
  swatches.setAttribute("aria-label", "Color");
  for (let i = -1; i < COLOR_COUNT; i++) {
    const sw = h("button", "sw");
    sw.type = "button";
    sw.dataset.color = String(i);
    sw.setAttribute("aria-label", i < 0 ? "Default color" : `Color ${i + 1}`);
    sw.style.setProperty("--c", colorVar(i < 0 ? undefined : i));
    sw.classList.toggle("on", i < 0);
    swatches.append(sw);
  }
  swatches.addEventListener("click", (e) => {
    const sw = e.target instanceof Element ? e.target.closest(".sw") : null;
    if (!sw) return;
    for (const other of swatches.children) other.classList.toggle("on", other === sw);
  });

  // Not a <form>: the shell sandboxes widgets without `allow-forms`, so a submit would never fire.
  const submit = () => {
    const chosen = swatches.querySelector(".sw.on");
    const color = chosen instanceof HTMLElement ? Number(chosen.dataset.color) : -1;
    const result = checkDraft({ title: title.value, date: date.value, time: time.value, color: color < 0 ? undefined : color }, newId());
    if (!result.ok) {
      const field = { title, date, time }[result.error];
      field.classList.remove("bad");
      void field.offsetWidth; // restart the shake
      field.classList.add("bad");
      field.focus();
      return;
    }
    events = [...events, result.event];
    commit();
    adding = false;
    render();
  };
  const cancel = h("button", "btn cancel", "Cancel");
  cancel.type = "button";
  cancel.addEventListener("click", closeForm);
  const add = h("button", "btn primary", "Add");
  add.type = "button";
  add.addEventListener("click", submit);
  const actions = h("div", "actions");
  actions.append(cancel, add);

  form.append(title, when, swatches, actions);
  form.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeForm();
    else if (e.key === "Enter" && e.target instanceof HTMLInputElement) submit();
  });
  view.replaceChildren(form);
  requestAnimationFrame(() => title.focus());
}

function newId() {
  return typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

function render() {
  const today = dateOf(new Date());
  card.dataset.mode = mode;
  card.dataset.variant = variant;
  card.classList.toggle("adding", adding && mode === "agenda");
  if (mode === "month") renderMonth(today);
  else if (mode === "strip") renderStrip(today);
  else if (mode === "agenda") renderAgenda(today);
  else renderTile(today);
}

// ---------------------------------------------------------------- state

/** Rebuilds the day lookup and keeps the events (the file is the user's). */
function commit() {
  byDate = eventsByDate(events);
  void mypaper.storage.set("events", events);
}

function closeForm() {
  adding = false;
  render();
}

/** @param {number} by */
function go(by) {
  shift += by;
  render();
}

function applyMode() {
  const next = pickMode(variant, box.width, box.height);
  if (next !== mode) {
    mode = next;
    shift = 0;
    adding = false;
  }
  render();
}

trackSize(({ px: [width, height] }) => {
  box = { width, height };
  applyMode();
});

$("#prev").addEventListener("click", () => go(-1));
$("#next").addEventListener("click", () => go(1));
$("#add").addEventListener("click", () => {
  adding = !adding;
  render();
});
titleBtn.addEventListener("click", () => {
  shift = 0;
  selected = "";
  render();
});

// One listener for the cells and the delete buttons of whatever is drawn.
view.addEventListener("click", (e) => {
  const target = e.target instanceof Element ? e.target : null;
  const del = target?.closest(".del");
  if (del instanceof HTMLElement) {
    events = events.filter((ev) => ev.id !== del.dataset.id);
    commit();
    render();
    return;
  }
  const cell = target?.closest(".d");
  if (cell instanceof HTMLElement && (mode === "month" || mode === "strip")) {
    const date = cell.dataset.date ?? "";
    selected = selected === date ? "" : date;
    render();
  }
});

// Re-render when the date rolls over, only while visible.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
function scheduleMidnight() {
  const now = new Date();
  const wait = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime() - now.getTime() + 50;
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
  variant = /** @type {Variant} */ (VARIANTS.includes(next.variant) ? next.variant : "month");
  applyMode();
});

void mypaper.storage.get("events").then((stored) => {
  events = parseEvents(stored);
  byDate = eventsByDate(events);
  render();
});
