// Moving modules between a bar's lists (`top.left`, `dock.right`…), as a drag on the stage or the
// inspector's lists does it. Lists hold module refs and `|` (a gap: a separator, or a new island).
// The page shows each module once, in list order, so a place among the shown modules maps to an
// index in the list. Only the lists that change are written; the rest keep following the design.

import { setIn, type Resolved } from "./barsDoc";
import type { BarKind, BarsDoc, ModuleRef } from "./barsSchema";
import { BREAK } from "./barsSchema";
import type { ZoneId } from "./barsGeometry";

/** What each list is called, in words. */
export const ZONE_NAMES: Record<ZoneId, string> = { left: "Start", center: "Middle", right: "End" };

/** The list a bar's zone resolves to (the file's, else the design's). */
export function listOf(config: Resolved, bar: BarKind, zone: ZoneId): ModuleRef[] {
  if (bar === "top") return [...config.top[zone]];
  return zone === "left" ? [...config.dock.left] : zone === "right" ? [...config.dock.right] : [];
}

/** The list index where the `ordinal`-th shown module sits (gaps don't count); the end past the last. */
export function indexOfShown(list: readonly ModuleRef[], ordinal: number): number {
  let seen = 0;
  for (let i = 0; i < list.length; i++) {
    if (list[i] === BREAK) continue;
    if (seen === ordinal) return i;
    seen++;
  }
  return list.length;
}

const write = (doc: BarsDoc, bar: BarKind, zone: ZoneId, list: ModuleRef[]) => setIn(doc, [bar, zone], list);

/**
 * `doc` with `ref` taken out of (`from.bar`, `from.zone`) and, unless `to` is null (dropped off the
 * bars), put before the `to.ordinal`-th shown module of (`to.bar`, `to.zone`).
 */
export function moveModule(doc: BarsDoc, config: Resolved, from: { bar: BarKind; zone: ZoneId; ref: ModuleRef }, to: { bar: BarKind; zone: ZoneId; ordinal: number } | null): BarsDoc {
  const source = listOf(config, from.bar, from.zone);
  const at = source.indexOf(from.ref);
  if (at < 0) return doc;
  const shownBefore = source.slice(0, at).filter((ref) => ref !== BREAK).length;
  source.splice(at, 1);
  if (!to) return write(doc, from.bar, from.zone, source);
  const same = to.bar === from.bar && to.zone === from.zone;
  const target = same ? source : listOf(config, to.bar, to.zone).filter((ref) => ref !== from.ref);
  // Taking it out first shifts the later places in the same list down by one.
  const ordinal = same && to.ordinal > shownBefore ? to.ordinal - 1 : to.ordinal;
  target.splice(indexOfShown(target, ordinal), 0, from.ref);
  const moved = same ? doc : write(doc, from.bar, from.zone, source);
  return write(moved, to.bar, to.zone, target);
}

/** `doc` with `ref` added at the end of (`bar`, `zone`). */
export function addModule(doc: BarsDoc, config: Resolved, bar: BarKind, zone: ZoneId, ref: ModuleRef): BarsDoc {
  return write(doc, bar, zone, [...listOf(config, bar, zone), ref]);
}

/** `doc` with the list (`bar`, `zone`) replaced, e.g. reordered in the inspector. */
export function setList(doc: BarsDoc, bar: BarKind, zone: ZoneId, list: ModuleRef[]): BarsDoc {
  return write(doc, bar, zone, list);
}

/** The zone (`left`, `center`, `right`) of a bar that holds `ref`, if any. */
export function zoneHolding(config: Resolved, bar: BarKind, ref: ModuleRef, zones: readonly ZoneId[]): ZoneId | null {
  return zones.find((zone) => listOf(config, bar, zone).includes(ref)) ?? null;
}
