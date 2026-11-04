// @ts-check
// Now playing: artwork-first media card with three looks (card, backdrop, strip), set with
// `data-variant` on <html> and laid out entirely in CSS. JS only feeds it: track text, the cover, an
// accent taken from the cover (once per track), and progress once a second while playing.
import { $, onVisible, setText, trackSettings } from "./kit.js";
import { accentSaturation, clock, dominantHue, fraction, positionNow, remaining } from "./media.js";

/** @typedef {NonNullable<Awaited<ReturnType<typeof mypaper.media.current>>>} Media */

const VARIANTS = /** @type {const} */ (["card", "blur", "strip"]);
const SAMPLE = 32; // the cover is shrunk to 32x32: for the accent, and as the backdrop's source

const root = $("#root");
const els = {
  img: $("#img", HTMLImageElement),
  wash: $("#wash", HTMLImageElement),
  title: $("#title"),
  artist: $("#artist"),
  app: $("#app"),
  fill: $("#fill"),
  thumb: $("#thumb"),
  pos: $("#pos"),
  rem: $("#rem"),
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
let lastP = 0;

// ---- progress

function renderProgress() {
  const duration = media?.duration ?? 0;
  const position = media ? positionNow(media, performance.now() - anchor) : 0;
  const p = fraction(position, duration);
  // A seek, a new track or a long gap jumps: no sweep through the transition.
  const jump = p < lastP || p - lastP > 0.03;
  lastP = p;
  if (jump) root.toggleAttribute("data-jump", true);
  els.fill.style.transform = `scaleX(${p})`;
  els.thumb.style.left = `${p * 100}%`;
  if (jump) {
    void root.offsetWidth; // commit the jump before the transition comes back
    root.toggleAttribute("data-jump", false);
  }
  setText(els.pos, clock(position));
  setText(els.rem, duration > 0 ? remaining(position, duration) : "");
}

/** The bar only needs a tick while a track is playing and shown; the CSS transition fills the second. */
function reschedule() {
  clearInterval(timer);
  timer = undefined;
  renderProgress();
  if (visible && media?.playing && (media.duration ?? 0) > 0) timer = setInterval(renderProgress, 1000);
}

// ---- artwork: decoded once per track, then reused

const sampler = document.createElement("canvas");
sampler.width = sampler.height = SAMPLE;
const sctx = sampler.getContext("2d", { willReadFrequently: true });
let lastArtwork = "";
let artToken = 0;

/** Accent and backdrop from the cover already in `els.img`. */
async function analyse() {
  const token = ++artToken;
  try {
    await els.img.decode();
  } catch {
    return; // not an image we can read: the style's own accent stays
  }
  if (token !== artToken || !sctx) return;
  const { naturalWidth: w, naturalHeight: h } = els.img;
  const side = Math.min(w, h) || 1; // a centered square crop, like the cover itself
  sctx.drawImage(els.img, (w - side) / 2, (h - side) / 2, side, side, 0, 0, SAMPLE, SAMPLE);
  const found = dominantHue(sctx.getImageData(0, 0, SAMPLE, SAMPLE).data);
  if (found) {
    root.style.setProperty("--art-h", found.h.toFixed(0));
    root.style.setProperty("--art-s", `${(accentSaturation(found.s) * 100).toFixed(0)}%`);
  }
  root.toggleAttribute("data-tinted", Boolean(found));
  els.wash.src = sampler.toDataURL("image/jpeg", 0.8);
}

/** @param {string} artwork */
function setArtwork(artwork) {
  if (artwork === lastArtwork) return;
  lastArtwork = artwork;
  artToken++;
  els.img.hidden = !artwork;
  if (artwork) {
    els.img.src = artwork;
    void analyse();
  } else {
    els.img.removeAttribute("src");
    els.wash.removeAttribute("src");
    root.toggleAttribute("data-tinted", false);
  }
}

// ---- state

/** @param {Media | null} next */
function show(next) {
  media = next;
  anchor = performance.now();
  root.dataset["state"] = next ? (next.artwork ? "has-art" : "no-art") : "empty";
  root.toggleAttribute("data-playing", Boolean(next?.playing));
  root.toggleAttribute("data-live", Boolean(next) && !((next?.duration ?? 0) > 0));
  setText(els.title, next ? next.title || "Unknown title" : "Nothing playing");
  setText(els.artist, next ? next.artist || next.album || "" : "Start music in any app");
  setText(els.app, next?.app ? next.app.replace(/\.exe$/i, "") : "Now playing");
  els.title.title = next ? `${next.title} - ${next.artist}` : "";
  setArtwork(next?.artwork ?? "");
  reschedule();
}

const toggle = () => void mypaper.media.control("toggle");
$("#toggle").addEventListener("click", toggle);
$("#over").addEventListener("click", toggle);
$("#prev").addEventListener("click", () => void mypaper.media.control("previous"));
$("#next").addEventListener("click", () => void mypaper.media.control("next"));

/** @template {readonly string[]} L @param {string} v @param {L} list @param {L[number]} fallback @returns {L[number]} */
const pick = (v, list, fallback) => list.find((x) => x === v) ?? fallback;

trackSettings({ variant: /** @type {(typeof VARIANTS)[number]} */ ("card"), tint: true, glow: 55 }, (s) => {
  document.documentElement.dataset["variant"] = pick(s.variant, VARIANTS, "card");
  root.dataset["tint"] = s.tint ? "on" : "off";
  root.style.setProperty("--glow-amt", String(Math.min(1, Math.max(0, s.glow / 100))));
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
