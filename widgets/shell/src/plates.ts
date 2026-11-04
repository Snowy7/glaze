// Liquid glass plates (docs/widgets.md, "Liquid glass"). A liquid widget reports where its card is
// on its page (`plate`); the shell adds where the widget's slot is on the monitor and tells the host
// (`plates`), which publishes them to the wallpaper. Pure: no DOM.

import type { PlateCard, PlateShape } from "./protocol";
import type { PxRect } from "./grid";

const finite = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isBox = (v: unknown): v is PlateShape["rect"] => Array.isArray(v) && v.length === 4 && v.every(finite);

/** A widget's `plate` params, if they are a plate (anything else, `null` included, is none). */
export function plateShape(params: unknown): PlateShape | null {
  if (typeof params !== "object" || params === null) return null;
  if (!("rect" in params) || !("radius" in params)) return null;
  const { rect, radius } = params;
  if (!isBox(rect) || !finite(radius)) return null;
  const [x, y, width, height] = rect;
  if (width < 1 || height < 1 || width > 1e5 || height > 1e5) return null;
  return { rect: [x, y, width, height], radius: Math.max(0, radius) };
}

/**
 * The cards on the monitor: each plate moved by its slot's position, clipped to the slot (a widget
 * can't put glass outside its own box). Slots without a plate or a position are left out.
 */
export function platesOf(slots: Iterable<{ instance: string; plate: PlateShape | null; rect: PxRect | undefined }>): PlateCard[] {
  const out: PlateCard[] = [];
  for (const { instance, plate, rect: slot } of slots) {
    if (!plate || !slot) continue;
    const [px, py, pw, ph] = plate.rect;
    const x0 = Math.max(0, px);
    const y0 = Math.max(0, py);
    const x1 = Math.min(slot.width, px + pw);
    const y1 = Math.min(slot.height, py + ph);
    if (x1 - x0 < 1 || y1 - y0 < 1) continue;
    out.push({ instance, rect: [slot.x + x0, slot.y + y0, x1 - x0, y1 - y0], radius: Math.min(plate.radius, (x1 - x0) / 2, (y1 - y0) / 2) });
  }
  return out;
}
