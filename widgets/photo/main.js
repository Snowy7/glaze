// @ts-check
import { $, clamp, onVisible, trackSettings } from "./kit.js";
import { captionOf, createRotation, imageUrl, readImages } from "./rotation.js";

const FADE_MS = 600; // keep in sync with --fade in style.css

const frame = $("#frame");
const layers = [$("#a", HTMLImageElement), $("#b", HTMLImageElement)];
const caption = $("#caption");
const hint = $("#hint");
const hintTitle = $("#hint-title");
const hintSub = $("#hint-sub");

const defaults = {
  images: /** @type {unknown} */ ([]),
  intervalSeconds: 30,
  fit: "cover",
  shuffle: false,
  rounded: true,
  showCaption: false,
  motion: "still",
};
let settings = defaults;
/** @type {string[]} */
let files = [];
/** The list as last applied (undefined before the first settings arrive). */
let filesKey = /** @type {string | undefined} */ (undefined);
let rotation = createRotation(0, false);
/** Index into `files` of the picture on screen, once there is one. */
let shown = -1;
/** Which of the two stacked <img> layers is on top. */
let front = 0;
/** Newest advance wins: a click or a settings change can overtake one that is still loading. */
let token = 0;
let swappedAt = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let timer;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let cleanup;

/**
 * Loads an image into the browser cache so swapping it in is instant; resolves whether it worked.
 * @param {string} url
 * @returns {Promise<boolean>}
 */
function preload(url) {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(true);
    img.onerror = () => resolve(false);
    img.src = url;
  });
}

/**
 * @param {string} title
 * @param {string} sub
 */
function showHint(title, sub) {
  frame.toggleAttribute("data-empty", true);
  hint.hidden = false;
  hintTitle.textContent = title;
  hintSub.textContent = sub;
  caption.hidden = true;
}

function clearHint() {
  frame.removeAttribute("data-empty");
  hint.hidden = true;
}

/**
 * Crossfade to `files[index]`: the new layer fades in over the old one, which stays opaque below.
 * @param {number} index
 */
function swap(index) {
  const file = files[index];
  if (file === undefined) return;
  const incoming = layers[1 - front];
  const outgoing = layers[front];
  if (!incoming || !outgoing) return;
  clearTimeout(cleanup);

  incoming.src = imageUrl(file);
  incoming.className = "layer top";
  void incoming.offsetWidth; // start from opacity 0, so the next class change transitions
  incoming.classList.add("show");
  if (shown >= 0) outgoing.className = "layer under";
  cleanup = setTimeout(() => (outgoing.className = "layer"), FADE_MS + 50);

  front = 1 - front;
  shown = index;
  swappedAt = Date.now();
  caption.textContent = captionOf(file);
  caption.hidden = !settings.showCaption;
  clearHint();
}

/** Show the next picture (skipping files that fail to load), then wait for the following one. */
async function advance() {
  if (files.length === 0) return;
  const mine = ++token;
  for (let tries = 0; tries < files.length; tries++) {
    const index = rotation.next();
    if (index === shown && files.length > 1) continue;
    const ok = await preload(imageUrl(files[index] ?? ""));
    if (mine !== token) return;
    if (!ok) continue;
    if (index !== shown) swap(index);
    schedule();
    // Warm the cache with the one after, so the next swap doesn't wait for the disk.
    const upcoming = files[rotation.peek()];
    if (upcoming !== undefined) void preload(imageUrl(upcoming));
    return;
  }
  // Nothing loads: say so, and don't spin.
  showHint("Couldn't load the images", "Check the file names in settings");
}

/** One timer for the next picture, only while visible and only when there is something to cycle. */
function schedule() {
  clearTimeout(timer);
  timer = undefined;
  if (!mypaper.visible || files.length < 2) return;
  timer = setTimeout(() => void advance(), clamp(settings.intervalSeconds, 5, 600) * 1000);
}

function stop() {
  clearTimeout(timer);
  timer = undefined;
}

frame.addEventListener("click", () => {
  // Let a crossfade finish before the next one starts.
  if (files.length > 1 && Date.now() - swappedAt > FADE_MS) void advance();
});

trackSettings(defaults, (next) => {
  const previousFiles = filesKey;
  const previousShuffle = settings.shuffle;
  settings = next;
  frame.dataset.fit = next.fit === "contain" ? "contain" : "cover";
  frame.dataset.rounded = String(next.rounded);
  frame.dataset.motion = next.motion === "kenburns" ? "kenburns" : "still";
  // One slow zoom spans a picture's whole stay (and the crossfade into the next).
  frame.style.setProperty("--kb", `${clamp(next.intervalSeconds, 5, 600) + FADE_MS / 1000 + 1}s`);
  caption.hidden = !next.showCaption || shown < 0;

  files = readImages(next.images);
  filesKey = files.join("\n");
  frame.toggleAttribute("data-multi", files.length > 1);

  if (filesKey !== previousFiles) {
    token++;
    shown = -1;
    for (const layer of layers) {
      layer.className = "layer";
      layer.removeAttribute("src");
    }
    rotation = createRotation(files.length, next.shuffle);
    if (files.length === 0) {
      showHint("No images", "Add images to widgets/photo/images");
      return stop();
    }
    clearHint();
    // Hidden: onVisible below starts it on becoming visible.
    if (mypaper.visible) void advance();
  } else {
    if (next.shuffle !== previousShuffle) rotation = createRotation(files.length, next.shuffle);
    schedule(); // the interval may have changed
  }
});

onVisible(
  () => {
    if (files.length === 0) return;
    if (shown < 0) void advance();
    else schedule();
  },
  stop,
);
