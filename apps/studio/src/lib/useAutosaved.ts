// A file the user edits live. Edits show at once and are written after a short pause (the
// program that reads the file watches it, so changes apply as you go). Every edit is an undo
// step; quick runs of the same edit merge into one. Changes made elsewhere are picked up unless
// there are unsaved edits here.

import { useCallback, useEffect, useId, useRef, useState } from "react";

import { errorText, onChanged, type Changes } from "./api";
import { useHistory } from "./history";
import { useStudio } from "./studio";

const SAVE_DELAY_MS = 350;

type Options<T> = {
  load: () => Promise<T>;
  /** Writes the value; resolves to what is now on disk. */
  save: (value: T) => Promise<T>;
  /** Whether a file-change event concerns this file. */
  concerns: (changes: Changes) => boolean;
  /** The undo label of an edit that doesn't name itself, e.g. "Change bars". */
  label?: string;
};

export type Autosaved<T> = {
  /** Null until loaded, or when the file can't be read (see `error`). */
  value: T | null;
  error: string | null;
  saving: boolean;
  /** Applies `change` to the current value and schedules a save. `label` names the undo step. */
  edit: (change: (current: T) => T, label?: string) => void;
  /** Re-reads the file now, dropping nothing that is unsaved. */
  reload: () => Promise<void>;
};

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function useAutosaved<T>({ load, save, concerns, label: defaultLabel = "Change" }: Options<T>): Autosaved<T> {
  const { notify } = useStudio();
  const history = useHistory();
  const id = useId();
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // The latest options and value, readable from timers and listeners without re-subscribing.
  const latest = useRef({ load, save, concerns, notify });
  latest.current = { load, save, concerns, notify };
  const current = useRef<T | null>(null);
  const dirty = useRef(false);
  const timer = useRef(0);

  const adopt = useCallback((next: T) => {
    if (same(current.current, next)) return;
    current.current = next;
    setValue(next);
  }, []);

  const reload = useCallback(async () => {
    try {
      const fresh = await latest.current.load();
      setError(null);
      if (!dirty.current) adopt(fresh);
    } catch (e) {
      setError(errorText(e));
      if (!dirty.current) {
        current.current = null;
        setValue(null);
      }
    }
  }, [adopt]);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const sent = current.current;
    if (!dirty.current || sent === null) return;
    setSaving(true);
    try {
      const saved = await latest.current.save(sent);
      // Keep typing if more edits came in while this one was being written.
      if (same(current.current, sent)) {
        dirty.current = false;
        adopt(saved);
      }
    } catch (e) {
      latest.current.notify(errorText(e), "error");
      dirty.current = false;
      await reload();
    } finally {
      setSaving(false);
    }
  }, [adopt, reload]);

  /** Shows `next` and schedules its save, without recording a step (undo and redo use it). */
  const put = useCallback(
    (next: T) => {
      current.current = next;
      dirty.current = true;
      setValue(next);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
    },
    [flush],
  );

  const edit = useCallback(
    (change: (value: T) => T, label?: string) => {
      const before = current.current;
      if (before === null) return;
      const next = change(before);
      if (same(before, next)) return;
      const name = label ?? defaultLabel;
      history.record({ label: name, key: `${id}:${name}`, undo: () => put(before), redo: () => put(next) });
      put(next);
    },
    [put, history, id, defaultLabel],
  );

  useEffect(() => {
    void reload();
    const unlisten = onChanged((changes) => {
      if (latest.current.concerns(changes)) void reload();
    });
    return () => {
      void unlisten.then((stop) => stop());
      // Leaving the page with an edit still waiting: write it now.
      void flush();
    };
  }, [reload, flush]);

  return { value, error, saving, edit, reload };
}
