// Which of a file's settings hide a preset's, and removing them. A preset only sets layout, look,
// motion, dock behavior and the icon pack, so only those sections can conflict.

import { getIn, setIn } from "../../lib/barsDoc";
import type { BarsDoc } from "../../lib/barsSchema";

const PRESET_SECTIONS = ["top", "dock", "look", "animation", "icons"] as const;

type Obj = Record<string, unknown>;
const isObject = (value: unknown): value is Obj => typeof value === "object" && value !== null && !Array.isArray(value);

/** Every leaf (a value or a list) under `node`, as a path. */
function leaves(node: unknown, prefix: string[]): string[][] {
  if (!isObject(node)) return [prefix];
  return Object.entries(node).flatMap(([key, child]) => leaves(child, [...prefix, key]));
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Paths the file sets to something other than what `bundle` (a preset) sets at the same place. */
export function conflictsWith(doc: BarsDoc, bundle: BarsDoc): string[][] {
  return PRESET_SECTIONS.flatMap((section) => {
    const own = (doc as Obj)[section];
    if (own === undefined) return [];
    return leaves(own, [section]).filter((path) => {
      const theirs = getIn(bundle, path);
      return theirs !== undefined && !same(getIn(doc, path), theirs);
    });
  });
}

/** `doc` without the values at `paths` (and without the objects that empties). */
export function clearPaths(doc: BarsDoc, paths: readonly string[][]): BarsDoc {
  return paths.reduce((acc, path) => setIn(acc, path, undefined), doc);
}
