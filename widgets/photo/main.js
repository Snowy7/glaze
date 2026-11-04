// @ts-check
// Photo frame: two stacked slides that crossfade, each moving with a slow Ken Burns pan and zoom
// (a CSS animation on transform only). The slide that is about to be shown is loaded and decoded in
// the hidden layer ahead of time. The frame (matte, bleed, polaroid) is CSS keyed on html[data-variant].
// Nothing runs while the widget is hidden: the timer stops and base.css pauses the animations.
import { $, clamp, onVisible, trackSettings } from "./kit.js";
import { createRotation, kenBurns, moveSeconds, readImages } from "./frame.js";
import { SCENES } from "./scenes.js";

/** @typedef {{ src: string, title: string }} Source */
/** @typedef {{ slide: HTMLElement, img: HTMLImageElement, loaded: number }} Layer */

/** Crossfade length; keep in sync with --fade in style.css. */
const FADE_S = 1.2;

const root = document.documentElement;
const caption = $("#caption");

/** @param {string} selector @returns {Layer} */
function makeLayer(selector) {
  const slide = $(selector);
  const img = slide.firstElementChild;
  if (!(img instanceof HTMLImageElement)) throw new Error(`missing picture in ${selector}`);
  return { slide, img, loaded: -1 };
}
const layerA = makeLayer("#a");
const layerB = makeLayer("#b");

const VARIANTS = ["matte", "bleed", "polaroid"];
const defaults = {
  variant: "bleed",
  image1: "",
  image2: "",
  image3: "",
  image4: "",
  image5: "",
  image6: "",
  interval: 15,
  motion: 40,
  shuffle: false,
  fit: "cover",
  caption: "",
};
let settings = defaults;
/** What is shown: the user's pictures, or the built-in scenes. @type {readonly Source[]} */
let sources = SCENES;
let usingScenes = true;
let rotation = createRotation(sources.length, false);
/** What the current set of sources was made from, to tell a real change from a slider move. */
let sourcesKey = "";
/** Index into `sources` of the picture on screen (-1: none yet), and the layer that shows it. */
let shown = -1;
let front = layerA;
/** A newer begin() or settings change cancels the older one's pending work. */
let epoch = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let retireTimer;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let preloadTimer;

/** The layer that is not showing. */
const back = () => (front === layerA ? layerB : layerA);

// ---------------------------------------------------------------- loading

/**
 * Puts a picture into a layer (hidden) and waits until it is decoded, so showing it later is free.
 * @param {Layer} layer
 * @param {number} index
 * @returns {Promise<boolean>}
 */
async function load(layer, index) {
  const source = sources[index];
  if (!source) return false;
  if (layer.loaded === index) return true;
  layer.loaded = -1;
  layer.img.src = source.src;
  try {
    await layer.img.decode();
  } catch {
    return false;
  }
  layer.loaded = index;
  return true;
}

/** Loads the picture after the current one into the layer that is not showing. */
function preloadNext() {
  if (sources.length > 1) void load(back(), rotation.peek());
}

// ---------------------------------------------------------------- showing

/**
 * Starts a layer's Ken Burns and fades it in.
 * @param {Layer} layer
 * @param {number} index
 */
function present(layer, index) {
  const move = kenBurns(clamp(settings.motion, 0, 100));
  const interval = clamp(settings.interval, 5, 120);
  const style = layer.slide.style;
  style.setProperty("--x0", `${move.x0}%`);
  style.setProperty("--y0", `${move.y0}%`);
  style.setProperty("--s0", String(move.s0));
  style.setProperty("--x1", `${move.x1}%`);
  style.setProperty("--y1", `${move.y1}%`);
  style.setProperty("--s1", String(move.s1));
  const alone = sources.length < 2;
  // One picture drifts back and forth for good; several each move once and are replaced.
  style.setProperty("--dur", `${alone ? Math.max(20, interval) : moveSeconds(interval, FADE_S)}s`);
  layer.slide.classList.toggle("loop", alone);
  layer.slide.classList.remove("kb");
  void layer.slide.offsetWidth; // restart the animation
  layer.slide.classList.add("kb", "in");
  shown = index;
  caption.textContent = captionFor(index);
}

/** @param {number} index */
function captionFor(index) {
  return settings.caption.trim() || (usingScenes ? (sources[index]?.title ?? "") : "");
}

/** Fades a layer out; its move runs on until the fade is over. @param {Layer} layer */
function retire(layer) {
  layer.slide.classList.remove("in");
  clearTimeout(retireTimer);
  retireTimer = setTimeout(() => layer.slide.classList.remove("kb"), FADE_S * 1000 + 100);
}

/** Shows the first picture of a (new) set, then lines up the next one. */
async function begin() {
  const mine = ++epoch;
  clearTimeout(timer);
  clearTimeout(preloadTimer);
  for (const layer of [layerA, layerB]) {
    layer.slide.classList.remove("in", "kb");
    layer.loaded = -1;
  }
  front = layerA;
  shown = -1;
  for (let tries = 0; tries < sources.length; tries++) {
    const index = rotation.next();
    const ok = await load(front, index);
    if (mine !== epoch) return;
    if (ok) {
      present(front, index);
      preloadNext();
      schedule();
      return;
    }
  }
  // Nothing loads (the files are gone): fall back to the built-in scenes.
  if (!usingScenes) useSources([], false);
}

/** The next picture: fades in over the current one; it was loaded into the back layer already. */
async function advance() {
  const mine = epoch;
  if (sources.length < 2) return;
  const incoming = back();
  for (let tries = 0; tries < sources.length; tries++) {
    const index = rotation.next();
    if (index === shown) continue;
    const ok = await load(incoming, index);
    if (mine !== epoch) return;
    if (ok) {
      present(incoming, index);
      retire(front);
      front = incoming;
      // The layer that just faded out is still on screen for a moment: refill it afterwards.
      clearTimeout(preloadTimer);
      preloadTimer = setTimeout(preloadNext, FADE_S * 1000 + 150);
      schedule();
      return;
    }
  }
  schedule();
}

/** One timer for the next picture, only while visible and only when there is something to cycle. */
function schedule() {
  clearTimeout(timer);
  timer = undefined;
  if (!mypaper.visible || sources.length < 2 || shown < 0) return;
  timer = setTimeout(() => void advance(), clamp(settings.interval, 5, 120) * 1000);
}

/**
 * Switches to a set of pictures and starts over.
 * @param {readonly string[]} urls the user's pictures; none means the built-in scenes
 * @param {boolean} shuffle
 */
function useSources(urls, shuffle) {
  usingScenes = urls.length === 0;
  sources = usingScenes ? SCENES : urls.map((src) => ({ src, title: "" }));
  rotation = createRotation(sources.length, shuffle);
  void begin();
}

// ---------------------------------------------------------------- settings and visibility

trackSettings(defaults, (next) => {
  settings = { ...next, variant: VARIANTS.includes(next.variant) ? next.variant : "bleed" };
  root.dataset["variant"] = settings.variant;
  root.dataset["fit"] = next.fit === "contain" ? "contain" : "cover";

  const urls = readImages(next);
  const key = JSON.stringify([urls, next.shuffle]);
  if (key !== sourcesKey) {
    sourcesKey = key;
    useSources(urls, next.shuffle);
  } else {
    if (shown >= 0) caption.textContent = captionFor(shown);
    schedule(); // the interval may have changed
  }
});

onVisible(schedule, () => {
  clearTimeout(timer);
  timer = undefined;
});
