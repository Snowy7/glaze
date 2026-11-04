// What every Bars panel gets: the document being edited, the values it resolves to, and how to
// change it. The Bars view provides it; panels never touch the file themselves.

import { createContext, use } from "react";

import type { BarsCatalog } from "./api";
import type { BarsDoc } from "./barsSchema";
import type { Path, Resolved } from "./barsDoc";

export type BarsEditor = {
  /** The file as the user wrote it. */
  doc: BarsDoc;
  /** What the host will use: defaults, preset and file merged. */
  resolved: Resolved;
  catalog: BarsCatalog;
  /** Sets the value at `path`; `undefined` removes it (and any object that empties). */
  set: (path: Path, value: unknown) => void;
  /** Changes several things at once. */
  update: (change: (doc: BarsDoc) => BarsDoc) => void;
  /** Whether the file itself sets `path` (rather than the preset or a default). */
  has: (path: Path) => boolean;
};

export const BarsContext = createContext<BarsEditor | null>(null);

export function useBars(): BarsEditor {
  const editor = use(BarsContext);
  if (!editor) throw new Error("useBars must be used inside the Bars page");
  return editor;
}
