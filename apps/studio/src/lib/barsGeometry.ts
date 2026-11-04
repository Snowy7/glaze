// Where things are inside a live bar page (bars/shell, run in a same-origin frame): its bands,
// its module zones and each module, in the frame's own coordinates (DIPs, the stage's units).
// The shell marks them: `[data-band]` on each surface, `[data-zone]` on the top bar's zones,
// `.tb-start` / `.tb-end` on a taskbar dock's, `[data-module]` on each module.

import type { Box } from "./desktopGrid";
import type { BarKind, ModuleRef } from "./barsSchema";

/** A module list of a bar: the top bar's left / center / right, a taskbar dock's left / right. */
export type ZoneId = "left" | "center" | "right";

export type ModuleSpot = { bar: BarKind; zone: ZoneId; ref: ModuleRef; box: Box; /** Its place among the zone's modules. */ ordinal: number };
export type ZoneSpot = { bar: BarKind; zone: ZoneId; box: Box };

export type BarGeometry = {
  bar: BarKind;
  /** The bar's surfaces (one, or one per island). */
  bands: Box[];
  zones: ZoneSpot[];
  modules: ModuleSpot[];
};

const boxOf = (el: Element): Box => {
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, width: r.width, height: r.height };
};

function zoneOf(el: Element): ZoneId | null {
  const top = el.closest<HTMLElement>("[data-zone]")?.dataset.zone;
  if (top === "left" || top === "center" || top === "right") return top;
  if (el.closest(".tb-start")) return "left";
  if (el.closest(".tb-end")) return "right";
  return null;
}

/** Reads a bar page's layout. Empty when the page hasn't drawn yet. */
export function readGeometry(doc: Document, bar: BarKind): BarGeometry {
  const bands = [...doc.querySelectorAll("[data-band]")].map(boxOf).filter((b) => b.width > 0 && b.height > 0);
  const zones: ZoneSpot[] = [...doc.querySelectorAll("[data-zone], .tb-start, .tb-end")].flatMap((el) => {
    const zone = zoneOf(el);
    return zone ? [{ bar, zone, box: boxOf(el) }] : [];
  });
  const counts = new Map<ZoneId, number>();
  const modules: ModuleSpot[] = [...doc.querySelectorAll<HTMLElement>("[data-module]")].flatMap((el) => {
    const zone = zoneOf(el);
    const ref = el.dataset.module;
    if (!zone || !ref) return [];
    const ordinal = counts.get(zone) ?? 0;
    counts.set(zone, ordinal + 1);
    return [{ bar, zone, ref, box: boxOf(el), ordinal }];
  });
  return { bar, bands, zones, modules };
}

/** The smallest box around `boxes`, grown by `pad`; null for none. */
export function union(boxes: readonly Box[], pad = 0): Box | null {
  if (boxes.length === 0) return null;
  const x = Math.min(...boxes.map((b) => b.x)) - pad;
  const y = Math.min(...boxes.map((b) => b.y)) - pad;
  const right = Math.max(...boxes.map((b) => b.x + b.width)) + pad;
  const bottom = Math.max(...boxes.map((b) => b.y + b.height)) + pad;
  return { x, y, width: right - x, height: bottom - y };
}

export const contains = (box: Box, x: number, y: number, pad = 0) => x >= box.x - pad && x <= box.x + box.width + pad && y >= box.y - pad && y <= box.y + box.height + pad;

/** Where a module dropped at (`x`, `y`) lands: a zone and a place among its modules; null off the bars. */
export function dropTarget(geometries: readonly BarGeometry[], zonesOf: (bar: BarKind) => readonly ZoneId[], x: number, y: number): { bar: BarKind; zone: ZoneId; ordinal: number } | null {
  for (const g of geometries) {
    const band = union(g.bands);
    const zones = zonesOf(g.bar);
    if (!band || zones.length === 0 || !contains(band, x, y, 20)) continue;
    // Along the bar: across for a horizontal one, down for a dock on a side edge.
    const vertical = band.height > band.width;
    const along = vertical ? y : x;
    const start = vertical ? band.y : band.x;
    const length = vertical ? band.height : band.width;
    const middle = (b: Box) => (vertical ? b.y + b.height / 2 : b.x + b.width / 2);
    // A zone under the pointer, else the band split evenly between the bar's zones.
    const hit = g.zones.find((z) => (vertical ? z.box.height : z.box.width) > 4 && contains(z.box, vertical ? middle(band) : x, vertical ? y : middle(band), 6));
    const share = Math.min(zones.length - 1, Math.max(0, Math.floor(((along - start) / length) * zones.length)));
    const zone = hit?.zone ?? zones[share];
    if (!zone) continue;
    const before = g.modules.filter((m) => m.zone === zone && middle(m.box) < along).length;
    return { bar: g.bar, zone, ordinal: before };
  }
  return null;
}
