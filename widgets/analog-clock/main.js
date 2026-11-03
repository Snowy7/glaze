// @ts-check
import { $, onVisible, setText, trackSettings } from "./kit.js";

const NS = "http://www.w3.org/2000/svg";
const defaults = { showSeconds: true, smooth: false, numerals: "quarters", showDate: true };
let settings = defaults;

const card = $("#card");
const els = {
  clock: $("#clock", SVGSVGElement),
  ticks: $("#ticks", SVGElement),
  numerals: $("#numerals", SVGElement),
  date: $("#date", SVGElement),
  hour: $("#hour", SVGLineElement),
  minute: $("#minute", SVGLineElement),
  sec: $("#sec", SVGLineElement),
  hubSec: $("#hub-sec", SVGElement),
};
const dateFormat = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric" });

/**
 * @param {string} name
 * @param {Record<string, string | number>} attrs
 */
function svgEl(name, attrs) {
  const node = document.createElementNS(NS, name);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
  return node;
}

// The 60 ticks never change: hour ticks are longer and brighter (styled in CSS).
for (let i = 0; i < 60; i++) {
  const major = i % 5 === 0;
  els.ticks.append(
    svgEl("line", {
      class: major ? "tick major" : "tick minor",
      x1: 50,
      x2: 50,
      y1: 2.8,
      y2: major ? 8.6 : 5,
      transform: `rotate(${i * 6} 50 50)`,
    }),
  );
}

/** Which hours get a numeral for each `numerals` setting. */
const NUMERAL_HOURS = { none: [], quarters: [12, 3, 6, 9], all: [12, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] };

/** @param {string} setting */
function numeralHours(setting) {
  return setting === "none" || setting === "all" ? NUMERAL_HOURS[setting] : NUMERAL_HOURS.quarters;
}

function buildNumerals() {
  const hours = numeralHours(settings.numerals);
  els.numerals.replaceChildren(
    ...hours.map((hour) => {
      const angle = ((hour % 12) * 30 * Math.PI) / 180;
      const text = svgEl("text", { x: 50 + 34 * Math.sin(angle), y: 50 - 34 * Math.cos(angle) });
      text.textContent = String(hour);
      return text;
    }),
  );
  // Hands stop short of the numerals instead of running through them.
  const withNumerals = hours.length > 0;
  els.hour.setAttribute("y2", withNumerals ? "32" : "27");
  els.minute.setAttribute("y2", withNumerals ? "20" : "11");
}

// --- drawing ---------------------------------------------------------------------------------

/** The seconds hand's unwrapped angle, so the CSS transition never swings back at 59 -> 0. */
let secAngle = 0;
/** After a pause the seconds hand jumps to the right place instead of sweeping there. */
let snapNext = true;

/** @param {number} deg */
const rotate = (deg) => `rotate(${deg.toFixed(2)}deg)`;

/** @param {Date} now */
function draw(now) {
  const smooth = settings.showSeconds && settings.smooth;
  const seconds = now.getSeconds() + (smooth ? now.getMilliseconds() / 1000 : 0);
  const minutes = now.getMinutes() + now.getSeconds() / 60;
  const hours = (now.getHours() % 12) + minutes / 60;
  els.hour.style.transform = rotate(hours * 30);
  els.minute.style.transform = rotate(minutes * 6);

  if (settings.showSeconds) {
    const target = seconds * 6;
    let delta = target - (secAngle % 360);
    if (delta < -180) delta += 360;
    secAngle = snapNext ? target : secAngle + delta;
    els.sec.classList.toggle("snap", snapNext);
    els.sec.style.transform = rotate(secAngle);
    if (snapNext) {
      void els.sec.getBoundingClientRect(); // commit the jump before the transition comes back
      els.sec.classList.remove("snap");
      snapNext = false;
    }
  }
  setText(els.date, dateFormat.format(now));
}

// One loop: a frame loop only for the smooth sweep, otherwise a timeout aligned to the next
// second (ticking hand) or minute. Nothing runs while the widget is hidden.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let frame = 0;
let running = false;

function stop() {
  clearTimeout(timer);
  cancelAnimationFrame(frame);
}

function tick() {
  const now = new Date();
  draw(now);
  const step = settings.showSeconds ? 1000 : 60_000;
  timer = setTimeout(tick, step - (now.getTime() % step) + 5);
}

function loop() {
  draw(new Date());
  frame = requestAnimationFrame(loop);
}

function restart() {
  stop();
  if (!running) return;
  snapNext = true;
  if (settings.showSeconds && settings.smooth) loop();
  else tick();
}

onVisible(
  () => {
    running = true;
    restart();
  },
  () => {
    running = false;
    stop();
  },
);

trackSettings(defaults, (next) => {
  settings = next;
  buildNumerals();
  els.sec.toggleAttribute("hidden", !settings.showSeconds);
  els.hubSec.toggleAttribute("hidden", !settings.showSeconds);
  els.date.toggleAttribute("hidden", !settings.showDate);
  els.clock.classList.toggle("smooth", settings.smooth);
  card.dataset.numerals = settings.numerals;
  draw(new Date()); // paint right away, even before the visibility is known
  restart();
});
