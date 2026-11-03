// @ts-check
import { $, clamp, onVisible, pad2, setText, trackSettings, trackSize } from "./kit.js";

/** @typedef {"focus" | "short" | "long" | "custom"} Mode */
/** @type {readonly Mode[]} */
const MODES = ["focus", "short", "long", "custom"];
const MODE_NAMES = { focus: "Focus", short: "Short", long: "Long", custom: "Custom" };

const defaults = {
  focusMinutes: 25,
  shortMinutes: 5,
  longMinutes: 15,
  customMinutes: 10,
  sessionsBeforeLong: 4,
  autoAdvance: false,
};
let settings = defaults;

const KEY = "state";
/** How long the "done" glow plays. */
const PULSE_MS = 10_000;

const card = $("#card");
const els = {
  dial: $("#dial"),
  prog: $("#prog", SVGElement),
  time: $("#time"),
  label: $("#label"),
  dots: $("#dots"),
  side: $("#side"),
  controls: $("#controls"),
  play: $("#play"),
  iconPlay: $(".i-play", SVGElement),
  iconPause: $(".i-pause", SVGElement),
};
const segs = [...document.querySelectorAll(".seg")].filter((el) => el instanceof HTMLElement);

// --- state -----------------------------------------------------------------------------------
// Everything is wall-clock based, so it survives reloads and a held widget:
//   running:  endsAt is the epoch ms it finishes at; remaining is the value when it started
//   paused or idle: endsAt is null and remaining is what is left
//   finished: finishedAt is when it ended (remaining 0)
// `total` is the length of the current run (the progress ring's denominator).

/** @typedef {{ mode: Mode, total: number, remaining: number, endsAt: number | null, finishedAt: number | null, sessions: number }} State */

/** Minutes for a mode, from settings the user (or a hand-edited file) may have got wrong. */
function duration(/** @type {Mode} */ mode) {
  const minutes = { focus: settings.focusMinutes, short: settings.shortMinutes, long: settings.longMinutes, custom: settings.customMinutes }[mode];
  return Math.round(clamp(Number.isFinite(minutes) ? minutes : 1, 1, 600)) * 60_000;
}

const cycleLength = () => Math.round(clamp(Number.isFinite(settings.sessionsBeforeLong) ? settings.sessionsBeforeLong : 4, 2, 12));

/** @returns {State} */
function idle(/** @type {Mode} */ mode, /** @type {number} */ sessions) {
  const total = duration(mode);
  return { mode, total, remaining: total, endsAt: null, finishedAt: null, sessions };
}

/** @type {State} */
let state = idle("focus", 0);
/** False until storage was read, so the default state never overwrites a saved one. */
let loaded = false;
/** The user touched the widget since it finished: stop the glow. */
let acked = false;

/**
 * A saved state, or null when it doesn't look like one.
 * @param {unknown} raw
 * @returns {State | null}
 */
function parseState(raw) {
  if (typeof raw !== "object" || raw === null) return null;
  const saved = new Map(Object.entries(raw));
  const mode = MODES.find((m) => m === saved.get("mode"));
  /** @param {string} key */
  const num = (key) => {
    const v = saved.get(key);
    return typeof v === "number" && Number.isFinite(v) ? v : null;
  };
  const total = num("total");
  const remaining = num("remaining");
  const sessions = num("sessions");
  const endsAt = saved.get("endsAt") === null ? null : num("endsAt");
  const finishedAt = saved.get("finishedAt") === null ? null : num("finishedAt");
  if (!mode || total === null || total <= 0 || remaining === null || remaining < 0 || remaining > total || sessions === null || sessions < 0) return null;
  if (saved.get("endsAt") !== null && endsAt === null) return null;
  if (saved.get("finishedAt") !== null && finishedAt === null) return null;
  return { mode, total, remaining, endsAt, finishedAt, sessions: Math.floor(sessions) };
}

function persist() {
  if (!loaded) return;
  mypaper.storage.set(KEY, state).catch((err) => console.warn("timer: could not save", err));
}

const isRunning = () => state.endsAt !== null;
const isFinished = () => state.finishedAt !== null;
const isIdle = () => !isRunning() && !isFinished() && state.remaining === state.total;
const remainingMs = () => (state.endsAt === null ? state.remaining : Math.max(0, state.endsAt - Date.now()));

/** The mode after the current one finishes or is skipped. */
function nextMode() {
  if (state.mode === "focus") return state.sessions > 0 && state.sessions % cycleLength() === 0 ? "long" : "short";
  return "focus";
}

/** Switches to a fresh, stopped mode. Leaving a long break starts a new cycle. */
function enter(/** @type {Mode} */ mode) {
  state = idle(mode, state.mode === "long" ? 0 : state.sessions);
  acked = false;
}

function start() {
  const left = state.remaining > 0 ? state.remaining : state.total;
  state = { ...state, remaining: left, endsAt: Date.now() + left, finishedAt: null };
}

function pause() {
  state = { ...state, remaining: remainingMs(), endsAt: null };
}

/** The countdown reached zero. Counts a focus session; may roll straight into the next one. */
function finish() {
  const at = state.endsAt ?? Date.now();
  state = { ...state, remaining: 0, endsAt: null, finishedAt: at, sessions: state.sessions + (state.mode === "focus" ? 1 : 0) };
  acked = false;
  // Only a finish we watched happen rolls on by itself; one found after a reload waits for the user.
  if (settings.autoAdvance && state.mode !== "custom" && Date.now() - at < 2500) {
    enter(nextMode());
    start();
  }
}

// --- actions ---------------------------------------------------------------------------------

function change(/** @type {() => void} */ action) {
  action();
  persist();
  update();
}

const toggle = () =>
  change(() => {
    acked = true;
    if (isFinished()) {
      // Done: play starts what comes next (a custom countdown just runs again).
      enter(state.mode === "custom" ? "custom" : nextMode());
      start();
    } else if (isRunning()) pause();
    else start();
  });
const reset = () => change(() => enter(state.mode));
const skip = () => change(() => enter(nextMode()));

els.play.addEventListener("click", toggle);
$("#reset").addEventListener("click", reset);
$("#skip").addEventListener("click", skip);
for (const seg of segs) {
  seg.addEventListener("click", () => {
    const mode = MODES.find((m) => m === seg.dataset.mode);
    if (mode && mode !== state.mode) change(() => enter(mode));
  });
}
// Touching the widget acknowledges a finished timer: the glow stops.
card.addEventListener("pointerdown", () => {
  if (!acked && isFinished()) {
    acked = true;
    render();
  }
});

// --- drawing ---------------------------------------------------------------------------------

/** `25:00`, or `1:30:00` from an hour up. Counts up the displayed second as soon as it starts. */
function clock(/** @type {number} */ ms) {
  const s = Math.ceil(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s % 60)}` : `${pad2(m)}:${pad2(s % 60)}`;
}

/** Completed focus sessions to show as filled dots, and whether one more is in progress. */
function dotState() {
  const n = cycleLength();
  const done = state.sessions === 0 ? 0 : ((state.sessions - 1) % n) + 1;
  return { n, done, current: state.mode === "focus" && !isFinished() && done < n ? done : -1 };
}

function renderDots() {
  const { n, done, current } = dotState();
  const dots = Array.from({ length: n }, (_, i) => {
    const dot = document.createElement("i");
    dot.className = i < done ? "on" : i === current ? "cur" : "";
    return dot;
  });
  els.dots.replaceChildren(...(state.mode === "custom" ? [] : dots));
}

let dotsKey = "";

function render() {
  const left = remainingMs();
  const finished = isFinished();
  const running = isRunning();
  const progress = finished ? 1 : 1 - left / state.total;

  setText(els.time, clock(left));
  els.time.toggleAttribute("data-long", left >= 3_600_000);
  els.prog.style.strokeDashoffset = String((1 - clamp(progress, 0, 1)) * 100);
  els.prog.style.opacity = progress > 0.002 ? "1" : "0";
  card.dataset.state = finished ? "done" : running ? "running" : isIdle() ? "idle" : "paused";
  setText(els.label, finished ? "Done" : MODE_NAMES[state.mode]);

  els.iconPlay.toggleAttribute("hidden", running);
  els.iconPause.toggleAttribute("hidden", !running);
  els.play.setAttribute("aria-label", running ? "Pause" : finished ? "Start next" : "Start");

  for (const seg of segs) seg.setAttribute("aria-selected", String(seg.dataset.mode === state.mode));

  const key = `${state.mode}|${state.sessions}|${cycleLength()}|${finished}`;
  if (key !== dotsKey) {
    dotsKey = key;
    renderDots();
  }
  els.dial.toggleAttribute("data-pulse", finished && !acked && Date.now() - (state.finishedAt ?? 0) < PULSE_MS);
}

// --- ticking ---------------------------------------------------------------------------------
// One timeout per displayed second, only while running and visible. Coming back from a held state
// recomputes everything from the clock, so nothing drifts.

/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
let visible = false;

function schedule() {
  clearTimeout(timer);
  const endsAt = state.endsAt;
  if (!visible || endsAt === null) return;
  const left = endsAt - Date.now();
  // The shown second changes each time `left` drops below a multiple of 1000.
  const wait = left <= 0 ? 0 : ((left - 1) % 1000) + 1 + 5;
  timer = setTimeout(update, wait);
}

/** Applies a finished countdown (if it ran out), repaints, and arms the next tick. */
function update() {
  if (state.endsAt !== null && Date.now() >= state.endsAt) {
    finish();
    persist();
  }
  render();
  schedule();
}

onVisible(
  () => {
    visible = true;
    update();
  },
  () => {
    visible = false;
    clearTimeout(timer);
  },
);

// --- layout ----------------------------------------------------------------------------------
// tile (1x1, 1x2..): ring, readout and play only. row (wide): ring on the left, controls beside it.
// stack: mode switch, ring, controls.

trackSize(({ px: [width, height] }) => {
  const layout = width < 130 || (height < 130 && width < 150) ? "tile" : height < 130 || width / height > 1.4 ? "row" : "stack";
  card.dataset.layout = layout;
  card.dataset.modes = layout === "tile" || height < 130 || width < 175 ? "off" : "on";
  // In a tile the buttons sit inside the ring, under the readout.
  (layout === "tile" ? els.dial : els.side).append(els.controls);
});

// --- startup ---------------------------------------------------------------------------------

/** A stopped timer follows the settings; one that has been started keeps its own length. */
function followSettings() {
  if (isIdle()) state = idle(state.mode, state.sessions);
}

trackSettings(defaults, (next) => {
  settings = next;
  followSettings();
  update();
});

mypaper.storage
  .get(KEY)
  .then((saved) => {
    const parsed = parseState(saved);
    if (parsed) state = parsed;
  })
  .catch(() => {})
  .finally(() => {
    loaded = true;
    followSettings();
    update(); // also settles a countdown that ran out while the widget wasn't running
    persist();
  });
