// @ts-check
// The clock: one widget, five faces (the `variant` setting). Digital and Flip are the time as big
// as the box allows; Analog a dial; Greeting the time of day in words; World the time in other
// cities. Each face is an object with `render(now, animate)` and `seconds()`; one timer, aligned
// to the next second or minute and stopped while hidden, drives whichever face is showing.
import { $, onVisible, setText, trackSettings } from "./kit.js";
import { clockParts, dateLabels, fitScale, fontFamily, msToNextTick, timeParts } from "./clock-format.js";
import { createDial } from "./dial.js";
import { glyphSvg } from "./glyph.js";
import { greet, periodOf, splitLead } from "./greeting.js";
import { cityName, isDaytime, offsetLabel, parseZones, worldLayout, zoneClock } from "./zones.js";

/** @typedef {{ render(now: Date, animate: boolean): void, seconds(): boolean, reset?(): void }} Face */

const FACES = ["desk", "flip", "analog", "words", "world"];
const defaults = {
  variant: "desk",
  format24: true,
  showSeconds: false,
  showDate: true,
  zones: [
    { label: "New York", tz: "America/New_York" },
    { label: "London", tz: "Europe/London" },
    { label: "Tokyo", tz: "Asia/Tokyo" },
    { label: "Sydney", tz: "Australia/Sydney" },
  ],
  name: "",
  font: "",
};
let settings = defaults;

const card = $("#card");
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

/**
 * @param {string} className
 * @param {string} [text]
 */
function div(className, text) {
  const el = document.createElement("div");
  el.className = className;
  if (text !== undefined) el.textContent = text;
  return el;
}

// ================================================================ Digital and Flip

const FLIP_MS = 500; // keep in sync with the animation in style.css

/**
 * A split-flap card holding two digits. The static halves show the new value on top and the old
 * one below; the two leaves hinge over them only while the value changes (style.css animates them
 * for FLIP_MS, and a timer settles the card, so nothing depends on animation events).
 * @param {string} className
 */
function createFlipCard(className) {
  const el = div(`fc ${className}`);
  const halves = [div("half top"), div("half bottom"), div("leaf top"), div("leaf bottom")];
  const [topText, bottomText, leafTopText, leafBottomText] = halves.map((half) => {
    const span = document.createElement("span");
    half.append(span);
    return span;
  });
  if (!topText || !bottomText || !leafTopText || !leafBottomText) throw new Error("flip card parts");
  el.append(...halves);

  let shown = "";
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let settleTimer;
  // The flip is over: the lower half catches up and the leaves hide.
  const settle = () => {
    clearTimeout(settleTimer);
    el.classList.remove("flipping");
    setText(bottomText, shown);
  };
  return {
    el,
    /** @param {string} value @param {boolean} animate */
    set(value, animate) {
      if (value === shown) return;
      settle();
      if (animate && !reducedMotion.matches) {
        setText(leafTopText, shown);
        setText(leafBottomText, value);
        void el.offsetWidth; // restart the animation if the class was just removed
        el.classList.add("flipping");
        settleTimer = setTimeout(settle, FLIP_MS + 30);
      }
      shown = value;
      setText(topText, value);
      if (!el.classList.contains("flipping")) setText(bottomText, value);
    },
  };
}

/** @returns {Face} */
function digitalFace() {
  const stage = $("#stage");
  const timeEl = $("#time");
  const flipEl = $("#flip");
  const els = { h: $("#h"), m: $("#m"), s: $("#s"), ampm: $("#ampm"), wdL: $("#wd-l"), wdS: $("#wd-s"), dL: $("#d-l"), dS: $("#d-s") };
  const flips = { h: createFlipCard("h"), m: createFlipCard("m"), s: createFlipCard("s") };
  const period = div("period");
  flips.h.el.append(period);
  flipEl.append(flips.h.el, flips.m.el, flips.s.el);
  let dateKey = "";
  let hourLength = 0;

  // Shrinks the time (`--fit`) when a wide custom font would overflow the room it has.
  const fit = () => {
    card.style.setProperty("--fit", "1");
    const hero = settings.variant === "flip" ? flipEl : timeEl;
    card.style.setProperty("--fit", String(fitScale(stage.clientWidth, hero.offsetWidth)));
  };
  new ResizeObserver(fit).observe(card);
  document.fonts.addEventListener("loadingdone", fit);

  return {
    seconds: () => settings.showSeconds,
    reset() {
      dateKey = "";
      hourLength = 0;
    },
    render(now, animate) {
      const { variant, format24, showSeconds } = settings;
      const parts = timeParts(now, { format24, padHours: variant === "flip" });
      if (variant === "flip") {
        flips.h.set(parts.h, animate);
        flips.m.set(parts.m, animate);
        if (showSeconds) flips.s.set(parts.s, animate);
        setText(period, parts.period);
      } else {
        setText(els.h, parts.h);
        setText(els.m, parts.m);
        setText(els.s, parts.s);
      }
      setText(els.ampm, parts.period);
      els.s.hidden = !showSeconds;
      els.ampm.hidden = format24;
      const key = `${now.toDateString()}`;
      if (key !== dateKey) {
        dateKey = key;
        const labels = dateLabels(now, "full");
        setText(els.wdL, labels.weekday.long);
        setText(els.wdS, labels.weekday.short);
        setText(els.dL, labels.date.long);
        setText(els.dS, labels.date.short);
      }
      // "9" to "10" changes the text clock's width.
      if (parts.h.length !== hourLength) {
        hourLength = parts.h.length;
        fit();
      }
    },
  };
}

// ================================================================ Analog

/** @returns {Face} */
function analogFace() {
  const dial = createDial({ numerals: true, date: true });
  $("#dial-box").append(dial.el);
  const wd = $("#a-wd");
  const dt = $("#a-dt");
  const windowFormat = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric" });
  return {
    seconds: () => settings.showSeconds,
    reset() {
      dial.configure({ numerals: true, date: settings.showDate, seconds: settings.showSeconds });
    },
    render(now) {
      dial.set(now.getHours(), now.getMinutes(), now.getSeconds());
      dial.setDate(windowFormat.format(now));
      const labels = dateLabels(now, "full");
      setText(wd, labels.weekday.long);
      setText(dt, new Intl.DateTimeFormat(undefined, { day: "numeric", month: "long" }).format(now));
    },
  };
}

// ================================================================ Greeting

/** @returns {Face} */
function wordsFace() {
  const box = $("#words");
  const glyph = $("#w-glyph");
  const lead = $("#w-lead");
  const name = $("#w-name");
  const line = $("#w-line");
  let account = "";
  let asked = false;
  let shownPeriod = "";
  let key = "";

  // The greeting is as big as the box allows: measured at 100px, then scaled to fit.
  const fit = () => {
    box.style.setProperty("--fs", "100px");
    const text = /** @type {HTMLElement} */ (box.querySelector(".w-text"));
    const scale = Math.min(box.clientWidth / Math.max(1, text.scrollWidth), box.clientHeight / Math.max(1, text.scrollHeight));
    box.style.setProperty("--fs", `${Math.max(8, Math.floor(100 * scale * 0.98))}px`);
  };
  new ResizeObserver(fit).observe(box);
  document.fonts.addEventListener("loadingdone", fit);

  /** @type {Face} */
  const face = {
    seconds: () => false,
    reset() {
      key = "";
      if (!settings.name && !asked) {
        asked = true;
        mypaper.user.info().then(
          (info) => {
            account = info.name;
            key = "";
            face.render(new Date(), false);
          },
          () => {},
        );
      }
    },
    render(now) {
      const period = periodOf(now.getHours());
      const greeting = greet(period, settings.name || account);
      const [before, word] = greeting.name ? [greeting.lead, greeting.name] : splitLead(greeting.lead);
      const parts = timeParts(now, { format24: settings.format24, padHours: false });
      const time = `${parts.h}:${parts.m}${parts.period ? ` ${parts.period}` : ""}`;
      const date = now.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
      const next = `${before}|${word}|${time}|${date}|${settings.showDate}`;
      if (period !== shownPeriod) {
        shownPeriod = period;
        glyph.innerHTML = glyphSvg(period); // a constant from glyph.js
      }
      if (next === key) return;
      key = next;
      setText(lead, before);
      setText(name, word);
      box.dataset.named = String(Boolean(greeting.name));
      setText(line, settings.showDate ? `${time}  ·  ${date}` : time);
      fit();
    },
  };
  return face;
}

// ================================================================ World

const dayWord = (() => {
  const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  /** @param {number} diff */
  return (diff) => {
    const word = relative.format(diff, "day");
    return word.charAt(0).toUpperCase() + word.slice(1);
  };
})();

/** @returns {Face} */
function worldFace() {
  const box = $("#world");
  /** @type {{ el: HTMLElement, name: HTMLElement, sub: HTMLElement, time: HTMLElement, ap: HTMLElement, dial: ReturnType<typeof createDial> }[]} */
  let rows = [];
  let shape = "";

  /** @param {ReturnType<typeof worldLayout>} layout */
  const build = (layout) => {
    const zones = parseZones(settings.zones).slice(0, layout.shown);
    box.dataset.mode = layout.mode;
    box.style.setProperty("--cols", String(layout.cols));
    if (zones.length === 0) {
      box.replaceChildren(empty());
      rows = [];
      return;
    }
    rows = zones.map((zone) => {
      const el = div("city");
      const dial = createDial();
      const text = div("city-text");
      const name = div("city-name ellipsis", cityName(zone));
      const sub = div("city-sub ellipsis");
      const time = div("city-time num");
      const ap = document.createElement("span");
      ap.className = "city-ap";
      text.append(name, sub);
      el.append(dial.el, text, time);
      el.dataset.tz = zone.tz;
      return { el, name, sub, time, ap, dial };
    });
    box.replaceChildren(...rows.map((r) => r.el));
  };

  const empty = () => {
    const el = div("state");
    el.append(div("state-title", "No cities"), div("state-hint", "Add some in Settings"));
    return el;
  };

  const layout = () => {
    const next = worldLayout(box.clientWidth, box.clientHeight, parseZones(settings.zones).length);
    const nextShape = `${next.mode}|${next.cols}|${next.shown}|${JSON.stringify(settings.zones)}`;
    if (nextShape === shape) return;
    shape = nextShape;
    build(next);
    face.render(new Date(), false);
  };
  new ResizeObserver(layout).observe(box);

  /** @type {Face} */
  const face = {
    seconds: () => false,
    reset() {
      shape = "";
      layout();
    },
    render(now) {
      for (const row of rows) {
        const clock = zoneClock(row.el.dataset.tz ?? "", now);
        if (!clock) {
          setText(row.sub, "Unknown time zone");
          setText(row.time, "--:--");
          row.el.classList.add("unknown");
          continue;
        }
        const parts = clockParts(clock.hours, clock.minutes, clock.seconds, { format24: settings.format24, padHours: false });
        setText(row.sub, offsetLabel(clock, dayWord));
        setText(row.time, `${parts.h}:${parts.m}`);
        setText(row.ap, parts.period);
        if (parts.period && row.ap.parentElement !== row.time) row.time.append(row.ap);
        row.el.classList.toggle("night", !isDaytime(clock.hours));
        row.dial.set(clock.hours, clock.minutes, 0);
      }
    },
  };
  return face;
}

// ================================================================ the active face, and one timer

/** @type {Record<string, Face>} */
const faces = {};
const digital = digitalFace();
faces.desk = digital;
faces.flip = digital;
faces.analog = analogFace();
faces.words = wordsFace();
faces.world = worldFace();

const face = () => faces[settings.variant] ?? digital;

/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let running = false;

/** @param {boolean} animate */
function tick(animate) {
  const now = new Date();
  face().render(now, animate);
  timer = setTimeout(() => tick(true), msToNextTick(now.getTime(), face().seconds()));
}

function restart() {
  clearTimeout(timer);
  if (running) tick(false);
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
  settings = FACES.includes(next.variant) ? next : { ...next, variant: defaults.variant };
  card.dataset.face = settings.variant;
  card.classList.toggle("has-sec", settings.showSeconds);
  card.classList.toggle("has-ampm", !settings.format24);
  card.classList.toggle("no-date", !settings.showDate);
  const family = fontFamily(settings.font);
  if (family) card.style.setProperty("--w-font", family);
  else card.style.removeProperty("--w-font");
  face().reset?.();
  // Paint right away so the first frame isn't "--:--", even before the visibility is known.
  face().render(new Date(), false);
  restart();
});
