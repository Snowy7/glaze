// @ts-check
// Pure logic for the now-playing widget: time math and the accent derived from the artwork.
// Tested in packages/widget-sdk/test/now-playing.test.ts.

/** @param {number} n @param {number} lo @param {number} hi */
const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n));

/**
 * `3:07`, or `1:02:03` past an hour.
 * @param {number} seconds
 */
export function clock(seconds) {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/**
 * Where the track is now: the sampled `position`, advanced by the time since it was sampled while
 * playing, clamped to the duration. 0 when the duration is unknown.
 * @param {{ position?: number, duration?: number, playing: boolean }} media
 * @param {number} sinceMs milliseconds since `position` was true
 */
export function positionNow(media, sinceMs) {
  const duration = media.duration ?? 0;
  if (!(duration > 0)) return 0;
  const advanced = media.playing ? Math.max(0, sinceMs) / 1000 : 0;
  return clamp((media.position ?? 0) + advanced, 0, duration);
}

/**
 * 0..1 of the track played (0 when the duration is unknown).
 * @param {number} position
 * @param {number | undefined} duration
 */
export const fraction = (position, duration) => (duration && duration > 0 ? clamp(position / duration, 0, 1) : 0);

/**
 * `-1:23` for the time left.
 * @param {number} position
 * @param {number} duration
 */
export const remaining = (position, duration) => `-${clock(Math.max(0, duration - position))}`;

/**
 * Hue (0..360), saturation and lightness (0..1) of an sRGB color given as 0..255 channels.
 * @param {number} r @param {number} g @param {number} b
 */
export function rgbToHsl(r, g, b) {
  const R = r / 255;
  const G = g / 255;
  const B = b / 255;
  const max = Math.max(R, G, B);
  const min = Math.min(R, G, B);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  const s = d / (1 - Math.abs(2 * l - 1));
  const h = max === R ? ((G - B) / d) % 6 : max === G ? (B - R) / d + 2 : (R - G) / d + 4;
  return { h: (h * 60 + 360) % 360, s, l };
}

/**
 * The artwork's dominant hue from RGBA pixels (a tiny downsample is plenty): hues are bucketed
 * in 15 degree steps, each pixel voting with its saturation (washed out and near-black or
 * near-white pixels barely count), and the winning bucket's circular mean is returned. `null` when
 * the picture has no color worth taking (grayscale cover): the caller keeps the style's own accent.
 * @param {ArrayLike<number>} pixels RGBA, 4 numbers per pixel
 * @returns {{ h: number, s: number } | null}
 */
export function dominantHue(pixels) {
  const BUCKETS = 24;
  const weight = new Array(BUCKETS).fill(0);
  const sin = new Array(BUCKETS).fill(0);
  const cos = new Array(BUCKETS).fill(0);
  const sat = new Array(BUCKETS).fill(0);
  let counted = 0;
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    if ((pixels[i + 3] ?? 0) < 200) continue;
    counted++;
    const { h, s, l } = rgbToHsl(pixels[i] ?? 0, pixels[i + 1] ?? 0, pixels[i + 2] ?? 0);
    const vote = s * s * (1 - Math.abs(2 * l - 1)) ** 2;
    if (vote < 0.01) continue;
    const b = Math.floor(h / (360 / BUCKETS)) % BUCKETS;
    weight[b] += vote;
    sat[b] += s * vote;
    sin[b] += Math.sin((h * Math.PI) / 180) * vote;
    cos[b] += Math.cos((h * Math.PI) / 180) * vote;
  }
  if (counted === 0) return null;
  let best = 0;
  for (let b = 1; b < BUCKETS; b++) if (weight[b] > weight[best]) best = b;
  // Not enough color: less than 3% of a fully saturated, mid-light picture.
  if (weight[best] / counted < 0.03) return null;
  const h = ((Math.atan2(sin[best], cos[best]) * 180) / Math.PI + 360) % 360;
  return { h, s: sat[best] / weight[best] };
}

/**
 * The saturation the widget actually uses for an accent: vivid, never neon and never gray.
 * @param {number} s 0..1
 */
export const accentSaturation = (s) => clamp(s, 0.5, 0.82);
