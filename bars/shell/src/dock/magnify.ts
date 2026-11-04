// macOS-style dock magnification: pure falloff math. The DOM layer applies the results as transforms.

export interface MagnifyOptions {
  /** Scale of the icon under the pointer. */
  max: number;
  /** Distance from the pointer (px) at which an icon is back to scale 1. */
  radius: number;
  /** Unscaled icon size (px). */
  size: number;
}

export interface Magnified {
  scales: number[];
  /** Translation along the dock axis for each item, so growing icons push neighbors apart. */
  shifts: number[];
}

/** Smooth bump: 1 at the pointer, 0 at `radius`. */
export function falloff(distance: number, radius: number): number {
  const d = Math.abs(distance) / radius;
  return d >= 1 ? 0 : (1 + Math.cos(Math.PI * d)) / 2;
}

/**
 * Scale and shift for each item given the pointer position along the axis. Icons grow from the
 * screen edge (the caller sets the transform origin), so only the along-axis spread is computed
 * here: each item is pushed by the extra width of those before it and half its own, then the whole
 * row is anchored so the point under the pointer stays put (continuous as the pointer moves).
 */
export function magnify(centers: readonly number[], pointer: number, { max, radius, size }: MagnifyOptions): Magnified {
  const scales = centers.map((c) => 1 + (max - 1) * falloff(c - pointer, radius));
  const unanchored: number[] = [];
  let before = 0;
  for (const s of scales) {
    const extra = (s - 1) * size;
    unanchored.push(before + extra / 2);
    before += extra;
  }
  const anchor = interpolate(centers, unanchored, pointer);
  return { scales, shifts: unanchored.map((u) => u - anchor) };
}

/** Linear interpolation of `values` (at ascending `xs`) at `x`, clamped to the ends. */
function interpolate(xs: readonly number[], values: readonly number[], x: number): number {
  const last = xs.length - 1;
  if (last < 0) return 0;
  const first = xs[0] ?? 0;
  const end = xs[last] ?? 0;
  if (x <= first) return values[0] ?? 0;
  if (x >= end) return values[last] ?? 0;
  const i = xs.findIndex((c) => c > x) - 1;
  const x0 = xs[i] ?? 0;
  const x1 = xs[i + 1] ?? x0 + 1;
  const t = (x - x0) / (x1 - x0);
  const v0 = values[i] ?? 0;
  return v0 + t * ((values[i + 1] ?? v0) - v0);
}
