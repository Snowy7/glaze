// @ts-check
// Activity heatmap: reads what the todo and timer widgets write into the shared "activity"
// namespace ({ "YYYY-MM-DD": count } under the keys "todo" and "timer") and draws it as a weeks
// heatmap, a month calendar or a streak card. Layout of each variant lives in style.css.
import { $, clamp, every, setText, trackSettings } from "./kit.js";
import {
  dateKey,
  levelOf,
  mergeSources,
  monthGrid,
  monthLabels,
  parseKey,
  readCounts,
  recentDays,
  scaleMax,
  streaks,
  weekGrid,
} from "./heat.js";

/** @typedef {import("./heat.js").Counts} Counts */
/** @typedef {import("./heat.js").Source} Source */

const root = $("#root");
const card = $("#card");
const tip = $("#tip");

const VARIANTS = ["weeks", "month", "streak"];
const SOURCES = ["all", "todo", "timer"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WORDS = {
  all: { one: "completed", many: "completed", total: "completed" },
  todo: { one: "task", many: "tasks", total: "tasks done" },
  timer: { one: "focus session", many: "focus sessions", total: "focus sessions" },
};

const defaults = { variant: "weeks", source: "all", weekStart: "monday", labels: true, legend: true };
let settings = defaults;
/** @type {Counts} */
let todoCounts = {};
/** @type {Counts} */
let timerCounts = {};
/** The counts on screen. */
/** @type {Counts} */
let counts = {};
/** Size of the weeks plot, from its ResizeObserver. */
let plot = { w: 0, h: 0 };
/** @type {ResizeObserver | undefined} */
let observer;
/** What the current DOM was built for; a change in any of it repaints. */
let paintedKey = "";

const activity = mypaper.storage.shared("activity");

// ---------------------------------------------------------------- small helpers

/**
 * @param {string} tag
 * @param {string} [cls]
 * @param {string} [text]
 */
function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** Today at local midnight. */
function today() {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate());
}

/** @param {number} n @param {Source} source */
function phrase(n, source) {
  const words = WORDS[source];
  return source === "all" ? `${n} completed` : `${n} ${n === 1 ? words.one : words.many}`;
}

/**
 * A heat square: `data-l` is its intensity, `data-key` (a real day, not in the future) makes the
 * tooltip work.
 * @param {string} key "" for a decoration (legend, ghost)
 * @param {number} level
 * @param {boolean} [future]
 */
function heatCell(key, level, future = false) {
  const cell = el("i", "hm");
  cell.dataset["l"] = String(level);
  if (future) cell.dataset["future"] = "";
  else if (key) cell.dataset["key"] = key;
  return cell;
}

/** A calm, repeatable pattern of ghost levels for the empty state. @param {number} i */
function ghostLevel(i) {
  const v = (i * 7 + Math.floor(i / 7) * 5) % 13;
  return v < 6 ? 0 : v < 9 ? 1 : v < 11 ? 2 : v < 12 ? 3 : 4;
}

// ---------------------------------------------------------------- tooltip

/** @param {HTMLElement} cell */
function showTip(cell) {
  const key = cell.dataset["key"];
  const date = key ? parseKey(key) : null;
  if (!key || !date) return hideTip();
  const n = counts[key] ?? 0;
  const day = `${WEEKDAYS[date.getDay()]} ${date.getDate()} ${MONTHS[date.getMonth()]}`;
  const source = /** @type {Source} */ (settings.source);
  tip.replaceChildren(el("b", "num", n > 0 ? phrase(n, source) : "No activity"), el("span", "dim", day));
  tip.hidden = false;

  const box = card.getBoundingClientRect();
  const r = cell.getBoundingClientRect();
  // The page may be scaled: work in layout px.
  const k = box.width / card.offsetWidth || 1;
  const tw = tip.offsetWidth;
  const th = tip.offsetHeight;
  const cx = (r.left + r.width / 2 - box.left) / k;
  const above = (r.top - box.top) / k - th - 6;
  const left = clamp(cx - tw / 2, 2, card.offsetWidth - tw - 2);
  const top = above >= 2 ? above : (r.bottom - box.top) / k + 6;
  tip.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
}

function hideTip() {
  tip.hidden = true;
}

root.addEventListener("pointerover", (e) => {
  const target = e.target instanceof Element ? e.target.closest("[data-key]") : null;
  if (target instanceof HTMLElement) showTip(target);
  else hideTip();
});
root.addEventListener("pointerleave", hideTip);

// ---------------------------------------------------------------- variants

/** Resets the DOM for a variant and wires what it needs. @param {string} variant */
function build(variant) {
  observer?.disconnect();
  observer = undefined;
  root.className = `root v-${variant}`;
  root.replaceChildren();
  hideTip();
  if (variant !== "weeks") return;

  const head = el("div", "head");
  const lead = el("div", "lead");
  const total = el("b", "hero num total");
  const sub = el("span", "sub dim");
  sub.append(el("span", "what"), el("span", "span"));
  lead.append(el("span", "label", "Activity"), total, sub);
  const legend = el("div", "legend label");
  legend.append(el("span", "", "Less"));
  for (let l = 0; l < 5; l++) legend.append(heatCell("", l));
  legend.append(el("span", "", "More"));
  head.append(lead, legend);

  const area = el("div", "plot");
  const inner = el("div", "plot-in");
  area.append(inner);
  root.append(head, area);

  observer = new ResizeObserver(([entry]) => {
    if (!entry) return;
    plot = { w: Math.floor(entry.contentRect.width), h: Math.floor(entry.contentRect.height) };
    paint();
  });
  observer.observe(area);
}

function paintWeeks() {
  const inner = $(".plot-in");
  if (plot.w < 40 || plot.h < 20) return;
  const now = today();
  // Labels need some room: a short widget keeps only the squares.
  const showLabels = settings.labels && plot.h >= 70;
  const monthRow = showLabels ? 14 : 0;
  let gap = 3;
  let cell = clamp(Math.floor((plot.h - monthRow - 6 * gap) / 7), 4, 20);
  if (cell < 9) {
    gap = 2;
    cell = clamp(Math.floor((plot.h - monthRow - 6 * gap) / 7), 4, 20);
  }
  const labelW = showLabels && cell >= 9 ? 26 : 0;
  const weeks = clamp(Math.floor((plot.w - labelW + gap) / (cell + gap)), 4, 53);
  const weekStart = settings.weekStart === "sunday" ? 0 : 1;
  const grid = weekGrid(now, weeks, weekStart);

  const shown = grid.flat().filter((c) => !c.future);
  const source = /** @type {Source} */ (settings.source);
  const total = shown.reduce((sum, c) => sum + (counts[c.key] ?? 0), 0);
  // Nothing yet: the header says how to start, over ghost squares.
  const none = total === 0;
  $(".total").hidden = none;
  setText($(".total"), String(total));
  setText($(".what"), none ? "Finish a task or a focus session to start" : WORDS[source].total);
  setText($(".span"), none ? "" : ` · ${weeks} weeks`);
  $(".legend").hidden = !settings.legend;

  // Too short even for the smallest squares: the total in the header says it all.
  if (plot.h - monthRow - 6 * gap < 7 * 3) return inner.replaceChildren();

  const empty = !shown.some((c) => (counts[c.key] ?? 0) > 0);
  const max = scaleMax(shown.map((c) => counts[c.key] ?? 0));

  const cells = el("div", "cells");
  grid.forEach((week, w) => {
    for (const [d, day] of week.entries()) {
      const level = empty ? (day.future ? 0 : ghostLevel(w * 7 + d)) : levelOf(counts[day.key] ?? 0, max);
      cells.append(heatCell(empty ? "" : day.key, level, day.future));
    }
  });

  const body = el("div", "body");
  body.style.setProperty("--cell", `${cell}px`);
  body.style.setProperty("--gap", `${gap}px`);
  body.style.setProperty("--weeks", String(weeks));
  body.style.setProperty("--lw", `${labelW}px`);
  body.toggleAttribute("data-empty", empty);

  if (showLabels) {
    const months = el("div", "months");
    // A name is about 26px wide: it takes as many columns as that, and so does the gap between names.
    const span = Math.max(3, Math.ceil(28 / (cell + gap)));
    for (const { col, month } of monthLabels(grid, span)) {
      const label = el("span", "", MONTHS[month]);
      label.style.gridColumn = `${col + 1} / span ${span}`;
      months.append(label);
    }
    const days = el("div", "days");
    if (labelW > 0) {
      for (let i = 0; i < 7; i += 2) {
        const label = el("span", "", WEEKDAYS[(i + (weekStart === 1 ? 1 : 2)) % 7]);
        label.style.gridRow = String(i + (weekStart === 1 ? 1 : 2));
        days.append(label);
      }
    }
    body.append(el("span"), months, days, cells);
  } else {
    body.append(cells);
  }
  inner.replaceChildren(body);
}

function paintMonth() {
  const now = today();
  const { year, month, weeks } = monthGrid(now, settings.weekStart === "sunday" ? 0 : 1);
  const source = /** @type {Source} */ (settings.source);
  const days = weeks.flat().filter((c) => c !== null);
  const max = scaleMax(days.map((c) => counts[c.key] ?? 0));
  const total = days.reduce((sum, c) => sum + (c.future ? 0 : (counts[c.key] ?? 0)), 0);

  const head = el("div", "head");
  const title = el("div", "lead");
  title.append(el("span", "caps", String(year)), el("b", "hero month", MONTHS_LONG[month]));
  const stat = el("div", "stat");
  stat.append(el("b", "num", String(total)), el("span", "dim", WORDS[source].total));
  head.append(title, stat);

  const box = el("div", "box");
  const grid = el("div", "mgrid");
  // The weekday letters are a row 0.6 of a cell high; --rows is the grid's height in cells.
  grid.style.setProperty("--rows", String(weeks.length + (settings.labels ? 0.6 : 0)));
  grid.style.gridTemplateRows = `${settings.labels ? "0.6fr " : ""}repeat(${weeks.length}, 1fr)`;
  if (settings.labels) {
    const first = settings.weekStart === "sunday" ? 0 : 1;
    for (let i = 0; i < 7; i++) grid.append(el("span", "wd caps", (WEEKDAYS[(first + i) % 7] ?? "").slice(0, 1)));
  }
  for (const week of weeks) {
    for (const c of week) {
      if (!c) {
        grid.append(el("span", "gap"));
        continue;
      }
      const cell = heatCell(c.key, levelOf(counts[c.key] ?? 0, max), c.future);
      cell.append(el("span", "num", String(c.day)));
      if (c.today) cell.dataset["today"] = "";
      grid.append(cell);
    }
  }
  box.append(grid);
  root.replaceChildren(head, box);
}

function paintStreak() {
  const now = today();
  const { current, best, total } = streaks(counts, now);
  const source = /** @type {Source} */ (settings.source);

  const main = el("div", "main");
  const flame = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  flame.setAttribute("viewBox", "0 0 24 24");
  flame.classList.add("flame");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.07-2.14-.22-4.05 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.15.43-2.29 1-3a2.5 2.5 0 0 0 2.5 2.5z");
  flame.append(path);
  main.dataset["on"] = String(current > 0);
  const label = el("span", "caps", "Current streak");
  const row = el("div", "row");
  row.append(flame, el("b", "hero num count", String(current)), el("span", "unit dim", current === 1 ? "day" : "days"));
  main.append(label, row);

  const side = el("div", "side");
  const stat = (/** @type {string} */ name, /** @type {string} */ value) => {
    const s = el("div", "stat");
    s.append(el("b", "num", value), el("span", "caps", name));
    return s;
  };
  side.append(stat("Best", String(best)), stat(WORDS[source].total === "completed" ? "Total" : WORDS[source].total, String(total)));

  const strip = el("div", "strip");
  const recent = recentDays(now, 28);
  const max = scaleMax(recent.map((d) => counts[d.key] ?? 0));
  for (const { key } of recent) strip.append(heatCell(key, levelOf(counts[key] ?? 0, max), false));

  root.replaceChildren(main, side, strip);
}

/** Draws the current variant from the current counts, unless nothing it shows has changed. */
function paint() {
  const variant = VARIANTS.includes(settings.variant) ? settings.variant : "weeks";
  const key = JSON.stringify([variant, dateKey(today()), settings, counts, plot]);
  if (key === paintedKey) return;
  if (root.className !== `root v-${variant}`) build(variant);
  paintedKey = key;
  document.documentElement.dataset["variant"] = variant;
  if (variant === "weeks") paintWeeks();
  else if (variant === "month") paintMonth();
  else paintStreak();
}

/** Rebuilds `counts` from the two sources and the source setting. */
function merge() {
  const source = SOURCES.includes(settings.source) ? /** @type {Source} */ (settings.source) : "all";
  counts = mergeSources(todoCounts, timerCounts, source);
}

async function load() {
  const read = (/** @type {string} */ key) => activity.get(key).catch(() => undefined);
  const [todo, timer] = await Promise.all([read("todo"), read("timer")]);
  todoCounts = readCounts(todo);
  timerCounts = readCounts(timer);
  merge();
  paint();
}

trackSettings(defaults, (next) => {
  settings = next;
  merge();
  paint();
});

// Re-read when the widget becomes visible and then every minute (which also rolls over midnight).
every(60_000, () => void load());
