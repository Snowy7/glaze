// Shared liquid-glass geometry for the bars and the widget shell. No DOM or host dependencies.

export interface LensShape {
  /** The pane's size (DIPs). */
  width: number;
  height: number;
  radius: number;
  /** How far the lens reaches in from the rim (DIPs). */
  bezel: number;
  /** Edges flush with the screen: no rim there (a strip along the top, a taskbar along the bottom). */
  flush: { top: boolean; right: boolean; bottom: boolean; left: boolean };
  /** Room around the pane in the map, so displaced pixels come from outside it (DIPs). */
  margin: number;
}

/** Which edges lack pixels beyond the pane after clipping a capture to the monitor. */
export function flushEdges(box: [number, number, number, number], captured: [number, number, number, number] | undefined, reach: number): LensShape["flush"] {
  if (!captured) return { top: false, right: false, bottom: false, left: false };
  const [x, y, w, h] = box;
  const [cx, cy, cw, ch] = captured;
  return { top: y - reach < cy, left: x - reach < cx, right: x + w + reach > cx + cw, bottom: y + h + reach > cy + ch };
}

/**
 * The displacement map for `shape` at `scale` (map pixels per DIP): RGBA, red and green the x
 * and y offset (128 is none), pointing out of the pane, strongest at the rim and fading to none
 * across the bezel with a convex lens's profile. Pure, so it can be tested.
 */
export function lensMap(shape: LensShape, scale: number): { width: number; height: number; data: Uint8ClampedArray<ArrayBuffer> } {
  const { width, height, radius, bezel, flush, margin } = shape;
  const mw = Math.max(1, Math.round((width + 2 * margin) * scale));
  const mh = Math.max(1, Math.round((height + 2 * margin) * scale));
  const data = new Uint8ClampedArray(new ArrayBuffer(mw * mh * 4));
  // A flush edge is pushed far out, so its distance never counts.
  const far = 1e5;
  const left = flush.left ? -far : 0;
  const right = flush.right ? width + far : width;
  const top = flush.top ? -far : 0;
  const bottom = flush.bottom ? height + far : height;
  const hw = (right - left) / 2;
  const hh = (bottom - top) / 2;
  const cx = (left + right) / 2;
  const cy = (top + bottom) / 2;
  const r = Math.min(radius, width / 2, height / 2);
  for (let j = 0; j < mh; j++) {
    for (let i = 0; i < mw; i++) {
      const x = (i + 0.5) / scale - margin;
      const y = (j + 0.5) / scale - margin;
      let dx = 0;
      let dy = 0;
      if (x >= 0 && y >= 0 && x <= width && y <= height) {
        // Signed distance to the rounded rect, and the outward normal.
        const px = x - cx;
        const py = y - cy;
        const qx = Math.abs(px) - (hw - r);
        const qy = Math.abs(py) - (hh - r);
        let distance: number;
        let nx: number;
        let ny: number;
        if (qx > 0 && qy > 0) {
          const len = Math.hypot(qx, qy) || 1;
          distance = r - len;
          nx = (qx / len) * Math.sign(px);
          ny = (qy / len) * Math.sign(py);
        } else if (qx > qy) {
          distance = r - qx;
          nx = Math.sign(px);
          ny = 0;
        } else {
          distance = r - qy;
          nx = 0;
          ny = Math.sign(py);
        }
        const t = Math.min(1, Math.max(0, distance / bezel));
        // A convex bezel: steepest at the rim, flat by the time it reaches the clear middle.
        const strength = (1 - t) ** 1.5;
        dx = nx * strength;
        dy = ny * strength;
      }
      const at = (j * mw + i) * 4;
      data[at] = 128 + dx * 127;
      data[at + 1] = 128 + dy * 127;
      data[at + 2] = 128;
      data[at + 3] = 255;
    }
  }
  return { width: mw, height: mh, data };
}

