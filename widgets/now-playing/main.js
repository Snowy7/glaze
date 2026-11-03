// @ts-check
import { $, clamp, onVisible, trackSettings } from "./kit.js";

/** @typedef {NonNullable<Awaited<ReturnType<typeof mypaper.media.current>>>} Media */

const card = $("#card");
const els = {
  backdrop: $("#backdrop"),
  img: $("#img", HTMLImageElement),
  title: $("#title"),
  artist: $("#artist"),
  fill: $("#fill"),
  pos: $("#pos"),
  dur: $("#dur"),
};

/** @type {Media | null} */
let media = null;
/** performance.now() when `media.position` was true. */
let anchor = 0;
let visible = false;
/** @type {ReturnType<typeof setInterval> | undefined} */
let timer;
/** @type {(() => void) | undefined} */
let unsubscribe;

/** @param {number} seconds */
function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function renderProgress() {
  const duration = media?.duration ?? 0;
  if (!media || duration <= 0) {
    card.style.setProperty("--p", "0");
    els.pos.textContent = els.dur.textContent = "0:00";
    return;
  }
  const elapsed = media.playing ? (performance.now() - anchor) / 1000 : 0;
  const position = clamp((media.position ?? 0) + elapsed, 0, duration);
  card.style.setProperty("--p", String(position / duration));
  els.pos.textContent = clock(position);
  els.dur.textContent = clock(duration);
}

/** The progress bar only needs a timer while a track is actually playing and shown. */
function reschedule() {
  clearInterval(timer);
  timer = undefined;
  renderProgress();
  if (visible && media?.playing && (media.duration ?? 0) > 0) timer = setInterval(renderProgress, 1000);
}

let lastArtwork = "";

/** @param {Media | null} next */
function show(next) {
  media = next;
  anchor = performance.now();
  card.dataset.state = next ? (next.artwork ? "has-art" : "no-art") : "empty";
  card.toggleAttribute("data-playing", Boolean(next?.playing));
  els.title.textContent = next ? next.title || "Unknown title" : "Nothing playing";
  els.artist.textContent = next ? next.artist || next.app : "Start music in any app";
  els.title.title = next ? `${next.title} - ${next.artist}` : "";

  const artwork = next?.artwork ?? "";
  if (artwork !== lastArtwork) {
    lastArtwork = artwork;
    els.img.hidden = !artwork;
    if (artwork) els.img.src = artwork;
    else els.img.removeAttribute("src");
    els.backdrop.style.backgroundImage = artwork ? `url("${artwork}")` : "";
  }
  reschedule();
}

$("#toggle").addEventListener("click", () => void mypaper.media.control("toggle"));
$("#prev").addEventListener("click", () => void mypaper.media.control("previous"));
$("#next").addEventListener("click", () => void mypaper.media.control("next"));

trackSettings({ blurArtwork: true }, (s) => {
  card.dataset.backdrop = s.blurArtwork ? "on" : "off";
});

// Subscribed only while visible (a hidden host sends nothing anyway); the first value is fetched.
onVisible(
  () => {
    visible = true;
    mypaper.media.current().then(show, () => show(null));
    unsubscribe = mypaper.media.onChange(show);
  },
  () => {
    visible = false;
    unsubscribe?.();
    unsubscribe = undefined;
    reschedule();
  },
);
