// What every Bars panel gets: the document being edited, the values it resolves to, and how to
// change it. The Bars view provides it; panels never touch the file themselves.

import { createContext, use } from "react";

import type { BarsCatalog } from "./api";
import type { BarKind, BarsDoc } from "./barsSchema";
import type { Path, Resolved } from "./barsDoc";

export type BarsEditor = {
  /** The file as the user wrote it. */
  doc: BarsDoc;
  /** What the host will use: defaults, preset and file merged. */
  resolved: Resolved;
  catalog: BarsCatalog;
  /** Sets the value at `path`; `undefined` removes it (and any object that empties). `label` names the undo step. */
  set: (path: Path, value: unknown, label?: string) => void;
  /** Changes several things at once; `label` names the undo step. */
  update: (change: (doc: BarsDoc) => BarsDoc, label?: string) => void;
  /** Whether the file itself sets `path` (rather than the preset or a default). */
  has: (path: Path) => boolean;
};

export const BarsContext = createContext<BarsEditor | null>(null);

export function useBars(): BarsEditor {
  const editor = use(BarsContext);
  if (!editor) throw new Error("useBars must be used inside the Bars page");
  return editor;
}

/**
 * The editor as one bar sees it: `look` and `animation` paths address that bar's own
 * (`top.look`, `dock.animation`...), and `resolved.look` / `doc.look` are that bar's, so the Look
 * and Motion panels edit one bar without knowing which.
 */
export function forBar(editor: BarsEditor, bar: BarKind): BarsEditor {
  const map = (path: Path): Path => (path[0] === "look" || path[0] === "animation" ? [bar, ...path] : path);
  const own = editor.doc[bar];
  return {
    ...editor,
    doc: { ...editor.doc, look: own?.look, animation: own?.animation },
    resolved: { ...editor.resolved, look: editor.resolved[bar].look, animation: editor.resolved[bar].animation },
    set: (path, value, label) => editor.set(map(path), value, label),
    has: (path) => editor.has(map(path)),
  };
}
