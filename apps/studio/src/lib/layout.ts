// The placed widgets and their look (`widgets.json`), edited live. Every change shows at once, is
// written after a short pause (quick runs merge), and is an undo step. The widget host writes
// the same file, so the file is re-read whenever it changes on disk.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, errorText, onChanged, type Appearance, type InstancePatch, type Layout, type PlacedWidget } from "./api";
import { useHistory } from "./history";
import { useStudio } from "./studio";

const WRITE_DELAY_MS = 300;

export type LayoutModel = {
  layout: Layout | null;
  error: string | null;
  saving: boolean;
  /** Replaces the look of all widgets. */
  setLook: (next: Appearance, label: string) => void;
  /** Adds a widget, or changes some of one's fields. */
  put: (patch: InstancePatch, label: string) => void;
  remove: (item: PlacedWidget) => void;
  setOverlay: (on: boolean) => void;
};

/** `layout` with `patch` applied the way the host will apply it. */
function patched(layout: Layout, patch: InstancePatch): Layout {
  const index = layout.instances.findIndex((item) => item.instance === patch.instance);
  const base: PlacedWidget = layout.instances[index] ?? { instance: patch.instance, widget: patch.widget ?? "", monitor: null, cell: null, size: null, appearance: {}, settings: {} };
  const next: PlacedWidget = {
    ...base,
    ...(patch.monitor !== undefined && { monitor: patch.monitor }),
    ...(patch.cell && { cell: patch.cell }),
    ...(patch.size && { size: patch.size }),
    ...(patch.settings && { settings: patch.settings }),
    ...(patch.appearance && { appearance: patch.appearance }),
  };
  const instances = index < 0 ? [...layout.instances, next] : layout.instances.with(index, next);
  return { ...layout, instances };
}

/** The patch that takes `patch` back on `item` (its current values of the fields `patch` names). */
function inverse(item: PlacedWidget, patch: InstancePatch): InstancePatch {
  return {
    instance: item.instance,
    ...(patch.monitor !== undefined && { monitor: item.monitor }),
    ...(patch.cell && item.cell && { cell: item.cell }),
    ...(patch.size && item.size && { size: item.size }),
    ...(patch.settings && { settings: item.settings }),
    ...(patch.appearance && { appearance: item.appearance }),
  };
}

const restore = (item: PlacedWidget): InstancePatch => ({
  instance: item.instance,
  widget: item.widget,
  monitor: item.monitor,
  ...(item.cell && { cell: item.cell }),
  ...(item.size && { size: item.size }),
  settings: item.settings,
  appearance: item.appearance,
});

export function useLayout(): LayoutModel {
  const { notify } = useStudio();
  const history = useHistory();
  const [layout, setLayout] = useState<Layout | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [inFlight, setInFlight] = useState(0);
  const current = useRef<Layout | null>(null);
  current.current = layout;
  // Writes waiting for their pause, by what they change; a newer edit of the same thing replaces it.
  const pending = useRef(new Map<string, { timer: number; write: () => Promise<Layout> }>());

  const reload = useCallback(async () => {
    if (pending.current.size > 0) return;
    try {
      setLayout(await api.getLayout());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  const run = useCallback(
    async (write: () => Promise<Layout>) => {
      setInFlight((n) => n + 1);
      try {
        const saved = await write();
        if (pending.current.size === 0) setLayout(saved);
      } catch (e) {
        notify(errorText(e), "error");
        await reload();
      } finally {
        setInFlight((n) => n - 1);
      }
    },
    [notify, reload],
  );

  /** Writes after a pause; the latest write for `key` wins. */
  const schedule = useCallback(
    (key: string, write: () => Promise<Layout>) => {
      const waiting = pending.current.get(key);
      if (waiting) window.clearTimeout(waiting.timer);
      const timer = window.setTimeout(() => {
        pending.current.delete(key);
        void run(write);
      }, WRITE_DELAY_MS);
      pending.current.set(key, { timer, write });
    },
    [run],
  );

  useEffect(() => {
    void reload();
    const unlisten = onChanged((changes) => changes.widgets && void reload());
    const waiting = pending.current;
    return () => {
      void unlisten.then((stop) => stop());
      // Leaving with a write still waiting: send it now.
      for (const { timer, write } of waiting.values()) {
        window.clearTimeout(timer);
        void write().catch(() => undefined);
      }
      waiting.clear();
    };
  }, [reload]);

  const applyLook = useCallback(
    (next: Appearance) => {
      setLayout((l) => l && { ...l, appearance: next });
      schedule("look", () => api.setWidgetsAppearance(next));
    },
    [schedule],
  );

  const setLook = useCallback(
    (next: Appearance, label: string) => {
      const before = current.current?.appearance;
      if (!before) return;
      history.record({ label, key: `widgets-look:${label}`, undo: () => applyLook(before), redo: () => applyLook(next) });
      applyLook(next);
    },
    [applyLook, history],
  );

  const applyPatch = useCallback(
    (patch: InstancePatch) => {
      setLayout((l) => l && patched(l, patch));
      const key = `put:${patch.instance}:${Object.keys(patch).sort().join(",")}`;
      schedule(key, () => api.putInstance(patch));
    },
    [schedule],
  );

  const applyRemove = useCallback(
    (instance: string) => {
      setLayout((l) => l && { ...l, instances: l.instances.filter((item) => item.instance !== instance) });
      schedule(`remove:${instance}`, () => api.removeInstance(instance));
    },
    [schedule],
  );

  const put = useCallback(
    (patch: InstancePatch, label: string) => {
      const item = current.current?.instances.find((i) => i.instance === patch.instance);
      const undo = item ? () => applyPatch(inverse(item, patch)) : () => applyRemove(patch.instance);
      history.record({ label, key: `widget:${patch.instance}:${label}`, undo, redo: () => applyPatch(patch) });
      applyPatch(patch);
    },
    [applyPatch, applyRemove, history],
  );

  const remove = useCallback(
    (item: PlacedWidget) => {
      history.record({ label: "Remove widget", undo: () => applyPatch(restore(item)), redo: () => applyRemove(item.instance) });
      applyRemove(item.instance);
    },
    [applyPatch, applyRemove, history],
  );

  const setOverlay = useCallback(
    (on: boolean) => {
      const write = (value: boolean) => {
        setLayout((l) => l && { ...l, overlay: value });
        void run(() => api.setWidgetsOverlay(value));
      };
      history.record({ label: on ? "Show always-on-top widgets" : "Hide always-on-top widgets", undo: () => write(!on), redo: () => write(on) });
      write(on);
    },
    [history, run],
  );

  const saving = inFlight > 0 || pending.current.size > 0;
  return useMemo(() => ({ layout, error, saving, setLook, put, remove, setOverlay }), [layout, error, saving, setLook, put, remove, setOverlay]);
}
