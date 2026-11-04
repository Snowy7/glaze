// @ts-check
// The photo frame's pure logic: which pictures, in which order, and the Ken Burns move of a slide.

const IMAGE_KEYS = ["image1", "image2", "image3", "image4", "image5", "image6"];

/**
 * The pictures set in the settings (`image1`..`image6`), in order and without duplicates. A value is
 * a served URL from the host's file picker (or, in the dev harness, a blob or data URL); anything
 * else is dropped, since the settings come from a file the user can edit.
 * @param {Record<string, unknown>} settings
 * @returns {string[]}
 */
export function readImages(settings) {
  const urls = new Set();
  for (const key of IMAGE_KEYS) {
    const value = settings[key];
    if (typeof value === "string" && isImageUrl(value.trim())) urls.add(value.trim());
  }
  return [...urls];
}

/** @param {string} value */
function isImageUrl(value) {
  if (value === "") return false;
  try {
    const { protocol } = new URL(value);
    if (protocol === "https:" || protocol === "http:" || protocol === "blob:") return true;
    return protocol === "data:" && value.startsWith("data:image/");
  } catch {
    return false;
  }
}

/**
 * The order pictures are shown in, forever: in list order, or shuffled in rounds (every picture once
 * per round, never the same one twice in a row across rounds). `peek` says what `next` will return,
 * so the widget can load it before it is needed.
 * @param {number} count
 * @param {boolean} shuffle
 * @param {() => number} [random] in [0, 1)
 * @returns {{ next(): number, peek(): number }}
 */
export function createRotation(count, shuffle, random = Math.random) {
  /** @type {number[]} */
  const queue = [];
  let last = -1;

  const refill = () => {
    const round = Array.from({ length: count }, (_, i) => i);
    if (shuffle) {
      for (let i = round.length - 1; i > 0; i--) {
        const j = Math.floor(random() * (i + 1));
        [round[i], round[j]] = [round[j] ?? 0, round[i] ?? 0];
      }
      if (round.length > 1 && round[0] === last) round.push(round.shift() ?? 0);
    }
    queue.push(...round);
    last = round[round.length - 1] ?? last;
  };

  return {
    next() {
      if (queue.length === 0 && count > 0) refill();
      return queue.shift() ?? 0;
    },
    peek() {
      if (queue.length === 0 && count > 0) refill();
      return queue[0] ?? 0;
    },
  };
}

/**
 * @typedef {Object} KenBurns
 * @property {number} x0 translation at the start, percent of the picture's width
 * @property {number} y0 same, of its height
 * @property {number} s0 scale at the start
 * @property {number} x1
 * @property {number} y1
 * @property {number} s1
 */

/**
 * A slow pan and zoom for one slide: it drifts in a random direction while zooming in (mostly) or
 * out. The move never uncovers an edge: at scale `s` the picture may be shifted by at most
 * `(s - 1) / 2` of its size, and both ends obey that (the path between them is a straight line, so
 * it does too). `motion` is the intensity, 0 (still) to 100 (about a 22% zoom).
 * @param {number} motion
 * @param {() => number} [random] in [0, 1)
 * @returns {KenBurns}
 */
export function kenBurns(motion, random = Math.random) {
  const m = Number.isFinite(motion) ? Math.min(1, Math.max(0, motion / 100)) : 0;
  if (m === 0) return { x0: 0, y0: 0, s0: 1, x1: 0, y1: 0, s1: 1 };

  const round = (/** @type {number} */ n) => Math.round(n * 100) / 100;
  const near = 1.02;
  const far = round(1.05 + 0.17 * m);
  const zoomIn = random() < 0.65;
  const s0 = zoomIn ? near : far;
  const s1 = zoomIn ? far : near;
  const limit = (/** @type {number} */ s) => (s - 1) * 50;

  // A direction with some length: a pan that goes nowhere looks like a bug.
  let dx = random() * 2 - 1;
  let dy = random() * 2 - 1;
  if (Math.abs(dx) + Math.abs(dy) < 0.6) dx = dx < 0 ? -1 : 1;
  // Rounded down, so rounding can't push a shift past its limit.
  const shift = (/** @type {number} */ n) => Math.trunc(n * 100) / 100;
  return {
    x0: shift(-dx * limit(s0)),
    y0: shift(-dy * limit(s0)),
    s0,
    x1: shift(dx * limit(s1)),
    y1: shift(dy * limit(s1)),
    s1,
  };
}

/**
 * How long a slide's move lasts, in seconds: its time on screen plus the two crossfades, so it is
 * still moving while it fades in and out.
 * @param {number} intervalSeconds
 * @param {number} fadeSeconds
 */
export const moveSeconds = (intervalSeconds, fadeSeconds) => intervalSeconds + 2 * fadeSeconds;
