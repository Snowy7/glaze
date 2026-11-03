// @ts-check
// The photo widget's pure logic: which files, in which order, and how to name them.

const MAX_IMAGES = 24;
const IMAGE_FILE = /^[^\\/:*?"<>|]+\.(?:svg|png|jpe?g|webp|gif|avif|bmp)$/i;

/**
 * The `images` list setting arrives as plain JSON from the user's layout file: keep unique file
 * names (no folders, only image extensions; a widget can only ever read its own folder anyway).
 * @param {unknown} value
 * @returns {string[]}
 */
export function readImages(value) {
  if (!Array.isArray(value)) return [];
  const files = new Set();
  for (const entry of value) {
    if (typeof entry !== "object" || entry === null || !("file" in entry) || typeof entry.file !== "string") continue;
    const file = entry.file.trim();
    if (IMAGE_FILE.test(file) && file !== "." && file !== "..") files.add(file);
  }
  return [...files].slice(0, MAX_IMAGES);
}

/** Where an image is served from: `images/` next to index.html. @param {string} file */
export const imageUrl = (file) => `images/${encodeURIComponent(file)}`;

/**
 * The file name without its extension, as words: `misty-lake_01.jpg` is `misty lake 01`.
 * @param {string} file
 */
export const captionOf = (file) => file.replace(/\.[^.]+$/, "").replace(/[-_]+/g, " ").trim();

/**
 * The order images are shown in, forever: in list order, or shuffled in rounds (every image once
 * per round, and never the same one twice in a row across rounds). `peek` tells what `next` will
 * return, so the widget can load it ahead of time.
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
      // A round must not start with the image the last one ended on.
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
