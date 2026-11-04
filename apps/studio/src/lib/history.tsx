// App-wide undo and redo. Every editor writes live, so every write records how to take it back:
// `record({ label, undo, redo })`. Quick runs of the same edit (a slider drag, typing) share a
// `key` and merge into one step. Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) work anywhere except in text
// fields, which keep their own undo.

import { createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type Step = {
  /** What the step did, for "Undo <label>". */
  label: string;
  undo: () => unknown;
  redo: () => unknown;
  /** Steps with the same key in quick succession merge (the first undo, the last redo). */
  key?: string;
};

type Entry = Step & { at: number };

type History = {
  record: (step: Step) => void;
  undo: () => void;
  redo: () => void;
  /** The label of the step Undo would take back, if any. */
  undoLabel: string | null;
  redoLabel: string | null;
};

const HistoryContext = createContext<History | null>(null);

export function useHistory(): History {
  const history = use(HistoryContext);
  if (!history) throw new Error("useHistory must be used inside <HistoryProvider>");
  return history;
}

const MERGE_MS = 1200;
const LIMIT = 100;

/** Text fields keep the platform's own undo. */
function typingIn(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target instanceof HTMLTextAreaElement) return true;
  return target instanceof HTMLInputElement && !["checkbox", "radio", "range", "color", "button"].includes(target.type);
}

export function HistoryProvider({ children }: { children: ReactNode }) {
  const past = useRef<Entry[]>([]);
  const future = useRef<Entry[]>([]);
  // Re-renders the labels; the stacks themselves live in refs so closures always see the latest.
  const [, setVersion] = useState(0);
  const bump = useCallback(() => setVersion((v) => v + 1), []);

  const record = useCallback(
    (step: Step) => {
      const now = Date.now();
      const top = past.current.at(-1);
      if (top && step.key !== undefined && top.key === step.key && now - top.at < MERGE_MS) {
        top.redo = step.redo;
        top.label = step.label;
        top.at = now;
      } else {
        past.current = [...past.current.slice(-(LIMIT - 1)), { ...step, at: now }];
      }
      future.current = [];
      bump();
    },
    [bump],
  );

  const undo = useCallback(() => {
    const step = past.current.pop();
    if (!step) return;
    future.current.push(step);
    void step.undo();
    bump();
  }, [bump]);

  const redo = useCallback(() => {
    const step = future.current.pop();
    if (!step) return;
    past.current.push({ ...step, at: 0 });
    void step.redo();
    bump();
  }, [bump]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || typingIn(event.target)) return;
      const key = event.key.toLowerCase();
      if (key === "z" && !event.shiftKey) undo();
      else if ((key === "z" && event.shiftKey) || key === "y") redo();
      else return;
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);

  const undoLabel = past.current.at(-1)?.label ?? null;
  const redoLabel = future.current.at(-1)?.label ?? null;
  const value = useMemo<History>(() => ({ record, undo, redo, undoLabel, redoLabel }), [record, undo, redo, undoLabel, redoLabel]);
  return <HistoryContext value={value}>{children}</HistoryContext>;
}
