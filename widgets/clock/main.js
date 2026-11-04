// @ts-check
import { $, onVisible, setText, trackSettings } from "./kit.js";
import { dateLabels, fitScale, fontFamily, msToNextTick, timeParts } from "./clock-format.js";

const VARIANTS = ["desk", "flip", "minimal"];
const defaults = { variant: "desk", format24: true, showSeconds: false, showDate: true, dateFormat: "full", font: "", size: 100 };
let settings = defaults;

const card = $("#card");
const stage = $("#stage");
const timeEl = $("#time");
const flipEl = $("#flip");
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

const FLIP_MS = 500; // keep in sync with the animation in style.css
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

/**
 * A split-flap card holding two digits. The static halves show the new value on top and the old one
 * below; the two leaves hinge over them only while the value changes (style.css animates them for
 * FLIP_MS, and a timer settles the card, so nothing depends on animation events).
 * @param {string} className
 */
function createFlipCard(className) {
  const el = div(`fc ${className}`);
  const top = div("half top");
  const bottom = div("half bottom");
  const leafTop = div("leaf top");
  const leafBottom = div("leaf bottom");
  const spans = [top, bottom, leafTop, leafBottom].map((half) => {
    const span = document.createElement("span");
    half.append(span);
    return span;
  });
  const [topText, bottomText, leafTopText, leafBottomText] = spans;
  if (!topText || !bottomText || !leafTopText || !leafBottomText) throw new Error("flip card parts");
  el.append(top, bottom, leafTop, leafBottom);

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
    /**
     * @param {string} value
     * @param {boolean} animate
     */
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

const flips = {
  h: createFlipCard("h"),
  m: createFlipCard("m"),
  s: createFlipCard("s"),
};
const period = div("period");
flips.h.el.append(period);
flipEl.append(flips.h.el, flips.m.el, flips.s.el);

let dateKey = "";
let hourLength = 0;

/**
 * @param {Date} now
 * @param {boolean} animate flip the cards (false for the first paint and after a change of settings)
 */
function render(now, animate) {
  const { variant, format24, showSeconds, showDate } = settings;
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

  if (showDate) {
    const key = `${now.getFullYear()}-${now.getMonth()}-${now.getDate()}|${settings.dateFormat}`;
    if (key !== dateKey) {
      dateKey = key;
      const labels = dateLabels(now, settings.dateFormat);
      setText(els.wdLong, labels.weekday.long);
      setText(els.wdShort, labels.weekday.short);
      setText(els.dLong, labels.date.long);
      setText(els.dShort, labels.date.short);
    }
  }
  // "9" to "10" changes the text clock's width.
  if (parts.h.length !== hourLength) {
    hourLength = parts.h.length;
    fit();
  }
}

/** Shrinks the time (`--fit`) when a wide custom font would overflow the room it has. */
function fit() {
  card.style.setProperty("--fit", "1");
  const hero = settings.variant === "flip" ? flipEl : timeEl;
  card.style.setProperty("--fit", String(fitScale(stage.clientWidth, hero.offsetWidth)));
}

// One timer, aligned to the next second or minute boundary, and only while visible.
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let running = false;

/** @param {boolean} animate */
function tick(animate) {
  const now = new Date();
  render(now, animate);
  timer = setTimeout(() => tick(true), msToNextTick(now.getTime(), settings.showSeconds));
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
  settings = VARIANTS.includes(next.variant) ? next : { ...next, variant: defaults.variant };
  card.dataset.variant = settings.variant;
  card.classList.toggle("has-sec", settings.showSeconds);
  card.classList.toggle("has-ampm", !settings.format24);
  card.classList.toggle("no-date", !settings.showDate);
  card.style.setProperty("--zoom", String(Math.min(100, Math.max(50, settings.size)) / 100));
  const family = fontFamily(settings.font);
  if (family) card.style.setProperty("--w-font", family);
  else card.style.removeProperty("--w-font");
  dateKey = "";
  // Paint right away so the first frame isn't "--:--", even before the visibility is known.
  render(new Date(), false);
  fit();
  restart();
});

// A new size or a late-loading font changes what fits.
new ResizeObserver(fit).observe(card);
document.fonts.addEventListener("loadingdone", fit);
