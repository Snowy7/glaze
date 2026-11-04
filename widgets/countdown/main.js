// @ts-check
// Countdown to (or since) a date. Digits: the biggest unit that isn't zero as the hero ("57 days"),
// the rest as a quiet line ("11 h 11 min"), then the date and how far along. Ring: the same hero
// inside an arc of the progress. One timer, aligned to the minute (or second), only while visible.
import { $, onVisible, setText, trackSettings, trackSize } from "./kit.js";
import { breakdown, phaseOf, progress, resolveTarget } from "./count.js";

/** @typedef {{ n: number, text: string, label: string }} Part */
/** @typedef {"digits" | "digits-tiny" | "ring-row" | "ring-col" | "ring-tiny"} Layout */

const TINY_BELOW_PX = 120; // narrower than this (or shorter than 80): one number, no frills
const DIGIT_EM = 0.58; // width of a light tabular numeral, in em
const MESSAGE_EM = 0.56; // average width of a letter of "Today!", in em

const card = $("#card");
const arc = $("#arc");
const ringValue = $("#rv");
const ringLabel = $("#rl");
const titleEl = $("#title");
const unitsEl = $("#units");
const msgEl = $("#msg");
const footEl = $("#foot");
const restEl = $("#rest");
const barEl = $("#bar");
const barFill = $("#bar-fill");

const defaults = {
  title: "New Year",
  date: "",
  time: "00:00",
  style: "digits",
  showSeconds: false,
  afterwards: "countUp",
  startDate: "",
};
let settings = defaults;
let size = { w: 200, h: 96 };

const dateFormat = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric", month: "short", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

// Four unit nodes, reused: days, hours, minutes, seconds.
const unitNodes = Array.from({ length: 4 }, () => {
  const root = document.createElement("div");
  root.className = "unit";
  const value = document.createElement("span");
  value.className = "v hero";
  const label = document.createElement("span");
  label.className = "l";
  root.append(value, label);
  unitsEl.append(root);
  return { root, value, label };
});

/**
 * @param {import("./count.js").Parts} b
 * @param {boolean} seconds
 * @returns {Part[]}
 */
function partsOf(b, seconds) {
  /** @type {Part[]} */
  const parts = [
    { n: b.days, text: String(b.days), label: b.days === 1 ? "day" : "days" },
    { n: b.hours, text: String(b.hours), label: b.hours === 1 ? "hour" : "hours" },
    { n: b.minutes, text: String(b.minutes), label: "min" },
  ];
  if (seconds) parts.push({ n: b.seconds, text: String(b.seconds), label: "s" });
  return parts;
}

/** @returns {Layout} */
function pickLayout() {
  const tiny = size.w < TINY_BELOW_PX || size.h < 80;
  if (settings.style === "ring") return tiny ? "ring-tiny" : size.w / size.h >= 1.5 ? "ring-row" : "ring-col";
  return tiny ? "digits-tiny" : "digits";
}

/** How many em wide the hero is: its numerals, then its word at 0.34em. */
const heroEm = (/** @type {Part | undefined} */ p) => (p ? p.text.length * DIGIT_EM + 0.2 + p.label.length * 0.34 * 0.55 : 1);

/** The units after the hero, as one line: "11 h 11 min". @param {Part[]} parts */
const restOf = (parts) =>
  parts
    .filter((p) => p.n > 0)
    .map((p) => `${p.n} ${p.label === "hours" || p.label === "hour" ? "h" : p.label}`)
    .join(" ");

/**
 * Draws the state for `now`.
 * @param {Date} now
 * @returns {boolean} whether the countdown is over for good (nothing more will change)
 */
function render(now) {
  const { target, start } = resolveTarget(settings, now);
  const phase = phaseOf(now, target);
  const stopped = phase === "after" && settings.afterwards === "stop";
  const layout = pickLayout();
  const ring = layout.startsWith("ring");

  /** @type {Part[]} */
  let parts = [];
  let message = "";
  if (phase === "before") parts = partsOf(breakdown(target.getTime() - now.getTime(), settings.showSeconds ? 1000 : 60_000), settings.showSeconds);
  else if (stopped) message = "Done";
  else if (phase === "after") parts = partsOf(breakdown(now.getTime() - target.getTime()), settings.showSeconds);
  else message = target.getHours() === 0 && target.getMinutes() === 0 ? "Today!" : "It's time";

  // The biggest unit that is not zero leads (all of them in a digits row; inside the ring otherwise).
  const lead = Math.max(0, parts.findIndex((p) => p.n > 0));
  const shown = ring ? [] : parts.slice(lead, lead + 1);
  const ago = phase === "after" && !stopped ? " ago" : "";

  shown.forEach((p, i) => {
    const node = unitNodes[i];
    if (!node) return;
    setText(node.value, p.text);
    setText(node.label, p.label + ago);
  });
  unitNodes.forEach((node, i) => (node.root.hidden = i >= shown.length));
  unitsEl.hidden = shown.length === 0;

  const leader = parts[lead];
  const rest = restOf(parts.slice(lead + 1));
  setText(restEl, rest);
  restEl.hidden = rest === "" || layout === "digits-tiny";
  setText(ringValue, ring ? (leader ? leader.text : message) : "");
  setText(ringLabel, ring && leader ? leader.label + ago : "");
  msgEl.hidden = ring || message === "";
  setText(msgEl, ring ? "" : message);

  const title = settings.title.trim();
  setText(titleEl, title && ago ? `Since ${title}` : title);
  titleEl.hidden = title === "";
  const timePart = target.getHours() === 0 && target.getMinutes() === 0 ? "" : `, ${timeFormat.format(target)}`;
  setText(footEl, dateFormat.format(target) + timePart);

  card.dataset.layout = layout;
  card.style.setProperty("--k", String(message ? message.length * MESSAGE_EM : heroEm(leader)));
  card.style.setProperty("--kc", String(message ? message.length * MESSAGE_EM * 0.9 : Math.max(2, (leader?.text.length ?? 2) * DIGIT_EM)));
  const fraction = phase === "before" ? progress(start, target, now) : 1;
  arc.setAttribute("stroke-dasharray", `${fraction < 0.004 ? 0 : fraction * 100} 100`);
  barEl.hidden = phase !== "before";
  barFill.style.width = `${fraction * 100}%`;

  card.setAttribute(
    "aria-label",
    [title, message || parts.map((p) => `${p.n} ${p.label}`).join(" ") + (ago ? " ago" : " left")].filter(Boolean).join(": "),
  );
  return stopped;
}

// One timer, aligned to the next second or minute, only while visible and while something changes.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let running = false;

function tick() {
  const now = new Date();
  if (render(now)) return;
  const step = settings.showSeconds ? 1000 : 60_000;
  timer = setTimeout(tick, step - (now.getTime() % step) + 5);
}

function restart() {
  clearTimeout(timer);
  if (running) tick();
  else render(new Date());
}

onVisible(
  () => {
    running = true;
    restart();
  },
  () => {
    running = false;
    clearTimeout(timer);
  },
);

trackSettings(defaults, (next) => {
  settings = next;
  restart();
});

trackSize(({ px: [w, h] }) => {
  size = { w, h };
  render(new Date());
});
