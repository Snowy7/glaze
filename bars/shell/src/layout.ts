// Which top-bar modules render where.

import { MODULES, type Module } from "./protocol";

export interface TopLayout {
  left: Module[];
  center: Module[];
  right: Module[];
}

const isModule = (name: string): name is Module => MODULES.some((m) => m === name);

/**
 * The config's zones with unknown names dropped and each module kept once (first occurrence wins,
 * scanning left, center, right), so a typo or a duplicate in `bars.json` can't break the bar.
 */
export function topLayout(top: { [Z in keyof TopLayout]: readonly string[] }): TopLayout {
  const seen = new Set<string>();
  const zone = (names: readonly string[]): Module[] =>
    names.filter((name): name is Module => {
      if (!isModule(name) || seen.has(name)) return false;
      seen.add(name);
      return true;
    });
  return { left: zone(top.left), center: zone(top.center), right: zone(top.right) };
}
