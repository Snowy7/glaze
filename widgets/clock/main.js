// @ts-check
import { $, onVisible, trackSettings } from "./kit.js";

const defaults = { format24: true, showSeconds: false, showDate: true };
let settings = defaults;

const card = $("#card");
const els = {
  h: $("#h"),
  m: $("#m"),
  s: $("#s"),
  ampm: $("#ampm"),
  wdLong: $("#wd-l"),
  wdShort: $("#wd-s"),
  dLong: $("#d-l"),
  dShort: $("#d-s"),
};

const fmt = {
  wdLong: new Intl.DateTimeFormat(undefined, { weekday: "long" }),
  wdShort: new Intl.DateTimeFormat(undefined, { weekday: "short" }),
  dLong: new Intl.DateTimeFormat(undefined, { day: "numeric", month: "long" }),
  dShort: new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" }),
};

/** @param {number} n */
const pad = (n) => String(n).padStart(2, "0");

/** @param {Date} now */
function render(now) {
  const { format24, showSeconds, showDate } = settings;
  els.h.textContent = format24 ? pad(now.getHours()) : String(now.getHours() % 12 || 12);
  els.m.textContent = pad(now.getMinutes());
  els.s.hidden = !showSeconds;
  els.s.textContent = pad(now.getSeconds());
  els.ampm.hidden = format24;
  els.ampm.textContent = now.getHours() < 12 ? "am" : "pm";
  card.classList.toggle("has-sec", showSeconds);
  card.classList.toggle("has-ampm", !format24);
  card.classList.toggle("no-date", !showDate);
  if (showDate) {
    els.wdLong.textContent = fmt.wdLong.format(now);
    els.wdShort.textContent = fmt.wdShort.format(now);
    els.dLong.textContent = fmt.dLong.format(now);
    els.dShort.textContent = fmt.dShort.format(now);
  }
}

// One timer, aligned to the next second or minute boundary, and only while visible.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let running = false;

function tick() {
  const now = new Date();
  render(now);
  const step = settings.showSeconds ? 1000 : 60_000;
  timer = setTimeout(tick, step - (now.getTime() % step) + 5);
}

function restart() {
  clearTimeout(timer);
  if (running) tick();
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
  // Paint right away so the first frame isn't "--:--", even before the visibility is known.
  render(new Date());
  restart();
});
