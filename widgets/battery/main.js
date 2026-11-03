// @ts-check
import { $, onVisible, setText, trackSettings } from "./kit.js";
import { batteryState, ringDash, statusText, styleOf } from "./state.js";

/** @typedef {Awaited<ReturnType<typeof mypaper.system.stats>>} Stats */

const defaults = { style: "ring", hideWhenMissing: false };
let settings = defaults;
/** @type {Stats | undefined} */
let stats;

const card = $("#card");
const live = $("#live");
const none = $("#none");
const els = {
  pct: $("#pct"),
  sub: $("#sub"),
  status: $("#status"),
  arc: $("#arc", SVGCircleElement),
  fill: $("#fill"),
};

/** Paints the latest stats with the current settings; cheap, called on every sample and setting. */
function render() {
  card.dataset.style = styleOf(settings.style);
  if (!stats) return; // the card stays hidden until the first sample
  const state = batteryState(stats.battery);

  if (state.kind === "none") {
    // `hideWhenMissing` renders nothing at all: an empty transparent page.
    card.hidden = settings.hideWhenMissing;
    card.dataset.state = "none";
    live.hidden = true;
    none.hidden = false;
    return;
  }

  card.hidden = false;
  live.hidden = false;
  none.hidden = true;
  card.dataset.state = state.kind;
  const status = statusText(state.kind);
  setText(els.pct, String(state.percent));
  setText(els.status, status);
  setText(els.sub, state.kind === "low" ? "Low" : status); // inside the ring, where the chip has no room
  els.arc.style.strokeDasharray = ringDash(state.percent, state.kind === "low");
  els.arc.style.visibility = state.percent > 0 ? "visible" : "hidden"; // a 0 arc would draw a round-cap dot
  els.fill.style.width = `${state.percent}%`;
}

/** @type {(() => void) | undefined} */
let unsubscribe;
onVisible(
  () => {
    /** @param {Stats} next */
    const take = (next) => {
      stats = next;
      render();
    };
    void mypaper.system.stats().then(take, () => {});
    unsubscribe = mypaper.system.subscribe(5000, take);
  },
  () => {
    unsubscribe?.();
    unsubscribe = undefined;
  },
);

trackSettings(defaults, (next) => {
  settings = next;
  render();
});
