// App state: a mirror of what's on disk and in the process list, refreshed on
// file-watcher events, on focus and on a slow poll for the engine. Every
// mutation goes through the backend and then re-reads, so the UI shows
// what the engine will actually see.

import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import {
  api,
  errorText,
  onChanged,
  type Autostart,
  type Component,
  type Config,
  type ConfigPatch,
  type CreateThemeRequest,
  type EngineStatus,
  type ThemeEntry,
} from "./api";
import { useHistory } from "./history";

/** The current values of the keys `patch` names: the patch that undoes it. */
function inverse(config: Config, patch: ConfigPatch): ConfigPatch {
  const { desktop_menu: menu, ...flat } = patch;
  const keys = Object.keys(flat) as (keyof typeof flat)[];
  const before: ConfigPatch = Object.fromEntries(keys.map((key) => [key, config[key]]));
  if (menu) {
    before.desktop_menu = {
      ...(menu.enabled !== undefined && { enabled: config.desktop_menu.enabled }),
      ...(menu.items !== undefined && { items: config.desktop_menu.items }),
    };
  }
  return before;
}

/** A toast's button, such as Undo. */
export type ToastAction = { label: string; run: () => void };

export type Toast = { id: number; tone: "info" | "error"; message: string; action?: ToastAction };

/** How `updateConfig` records the change: its undo label, or `false` (an undo or redo itself). */
export type ConfigWrite = { label?: string; record?: boolean };

type Studio = {
  config: Config | null;
  /** Set when config.json exists but can't be read. */
  configError: string | null;
  themes: ThemeEntry[];
  roots: string[];
  themesLoaded: boolean;
  engine: EngineStatus | null;
  /** Every mypaper program and whether it's running; null until the first check. */
  components: Component[] | null;
  autostart: Autostart | null;
  /** Shows a message at the bottom of the window, optionally with a button. */
  notify: (message: string, tone?: Toast["tone"], action?: ToastAction) => void;
  /** Keys of in-flight actions, e.g. `apply:<theme id>` or `engine`. */
  busy: ReadonlySet<string>;
  toast: Toast | null;
  /** Ids of video themes whose thumbnail is being made right now. */
  pendingPreviews: ReadonlySet<string>;
  /** Switches the desktop to `theme`, with an Undo in the toast. */
  applyTheme: (theme: ThemeEntry) => Promise<void>;
  /** Copies a theme into the user's themes folder; resolves to the copy. */
  duplicateTheme: (theme: ThemeEntry) => Promise<ThemeEntry | undefined>;
  /** Writes some config keys; recorded for undo unless `write.record` is false. */
  updateConfig: (patch: ConfigPatch, write?: ConfigWrite) => Promise<void>;
  resetConfig: () => Promise<void>;
  startEngine: () => Promise<void>;
  stopEngine: () => Promise<void>;
  setAutostart: (enabled: boolean) => Promise<void>;
  createTheme: (request: CreateThemeRequest) => Promise<ThemeEntry | undefined>;
  dismissToast: () => void;
};

const StudioContext = createContext<Studio | null>(null);

export function useStudio(): Studio {
  const studio = use(StudioContext);
  if (!studio) throw new Error("useStudio must be used inside <StudioProvider>");
  return studio;
}

const ENGINE_POLL_MS = 3000;

export function StudioProvider({ children }: { children: ReactNode }) {
  const [config, setConfig] = useState<Config | null>(null);
  const [configError, setConfigError] = useState<string | null>(null);
  const [themes, setThemes] = useState<ThemeEntry[]>([]);
  const [roots, setRoots] = useState<string[]>([]);
  const [themesLoaded, setThemesLoaded] = useState(false);
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [components, setComponents] = useState<Component[] | null>(null);
  const [autostart, setAutostartState] = useState<Autostart | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const [toast, setToast] = useState<Toast | null>(null);
  const toastTimer = useRef<number>(0);
  // Video thumbnails come from Windows' shell and can take a moment, so they
  // are made one at a time after the list shows. A failure keeps the placeholder.
  const [pendingPreviews, setPendingPreviews] = useState<ReadonlySet<string>>(new Set());
  const triedPreviews = useRef(new Set<string>());

  const history = useHistory();
  // The latest config, for building undo steps inside callbacks.
  const configRef = useRef<Config | null>(null);
  configRef.current = config;

  const notify = useCallback((message: string, tone: Toast["tone"] = "info", action?: ToastAction) => {
    window.clearTimeout(toastTimer.current);
    setToast({ id: Date.now(), tone, message, action });
    toastTimer.current = window.setTimeout(() => setToast(null), tone === "error" ? 8000 : action ? 6000 : 3500);
  }, []);

  const dismissToast = useCallback(() => {
    window.clearTimeout(toastTimer.current);
    setToast(null);
  }, []);

  /** Runs one action: tracks it in `busy`, turns a failure into an error toast. */
  const run = useCallback(
    async <T,>(key: string, task: () => Promise<T>): Promise<T | undefined> => {
      setBusy((keys) => new Set(keys).add(key));
      try {
        return await task();
      } catch (error) {
        notify(errorText(error), "error");
        return undefined;
      } finally {
        setBusy((keys) => {
          const next = new Set(keys);
          next.delete(key);
          return next;
        });
      }
    },
    [notify],
  );

  const refreshConfig = useCallback(async () => {
    try {
      setConfig(await api.getConfig());
      setConfigError(null);
    } catch (error) {
      setConfigError(errorText(error));
    }
  }, []);

  const refreshThemes = useCallback(async () => {
    try {
      const list = await api.listThemes();
      setThemes(list.themes);
      setRoots(list.roots);
    } catch (error) {
      notify(errorText(error), "error");
    } finally {
      setThemesLoaded(true);
    }
  }, [notify]);

  const refreshEngine = useCallback(async () => {
    try {
      const [status, programs] = await Promise.all([api.engineStatus(), api.components()]);
      setEngine(status);
      setComponents(programs);
    } catch {
      // A missed poll just keeps the last known status.
    }
  }, []);

  const refreshAutostart = useCallback(async () => {
    try {
      setAutostartState(await api.getAutostart());
    } catch {
      // Same: keep what we had.
    }
  }, []);

  useEffect(() => {
    void Promise.all([refreshConfig(), refreshThemes(), refreshEngine(), refreshAutostart()]);
    const unlisten = onChanged((changes) => {
      if (changes.config) void refreshConfig();
      // The active theme follows the config, so a config change refreshes the gallery too.
      if (changes.themes || changes.config) void refreshThemes();
    });
    return () => void unlisten.then((stop) => stop());
  }, [refreshConfig, refreshThemes, refreshEngine, refreshAutostart]);

  useEffect(() => {
    const waiting = themes.filter((t) => t.previewPending && !triedPreviews.current.has(t.id));
    setPendingPreviews(new Set(waiting.map((t) => t.id)));
    const next = waiting[0];
    if (!next) return;
    triedPreviews.current.add(next.id);
    api.generatePreview(next.dir).then(refreshThemes, refreshThemes);
  }, [themes, refreshThemes]);

  useEffect(() => {
    const poll = window.setInterval(() => {
      if (!document.hidden) void refreshEngine();
    }, ENGINE_POLL_MS);
    // The registry and process list can change while we're in the background.
    const onFocus = () => void Promise.all([refreshEngine(), refreshAutostart()]);
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(poll);
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshEngine, refreshAutostart]);

  const updateConfig = useCallback(
    async (patch: ConfigPatch, write: ConfigWrite = {}): Promise<void> => {
      const before = configRef.current;
      if (before && write.record !== false) {
        const undo = inverse(before, patch);
        history.record({
          label: write.label ?? "Change setting",
          key: `config:${Object.keys(patch).join(",")}`,
          undo: () => updateConfig(undo, { record: false }),
          redo: () => updateConfig(patch, { record: false }),
        });
      }
      const { desktop_menu, ...flat } = patch;
      // Shown at once; the saved config replaces it when the write returns.
      setConfig((current) => current && { ...current, ...flat, desktop_menu: { ...current.desktop_menu, ...desktop_menu } });
      const saved = await run("config", () => api.updateConfig(patch));
      if (saved) setConfig(saved);
      else await refreshConfig();
    },
    [run, refreshConfig, history],
  );

  const applyTheme = useCallback(
    async (theme: ThemeEntry) => {
      const previous = configRef.current?.theme;
      const saved = await run(`apply:${theme.id}`, () => api.updateConfig({ theme: theme.applyAs }));
      if (!saved) return;
      setConfig(saved);
      await refreshThemes();
      if (previous === undefined || previous === theme.applyAs) return notify(`Applied ${theme.name}`);
      const undo = () => updateConfig({ theme: previous }, { record: false });
      history.record({ label: `Apply ${theme.name}`, undo, redo: () => updateConfig({ theme: theme.applyAs }, { record: false }) });
      notify(`Applied ${theme.name}`, "info", { label: "Undo", run: history.undo });
    },
    [run, refreshThemes, notify, updateConfig, history],
  );

  const duplicateTheme = useCallback(
    async (theme: ThemeEntry) => {
      const copy = await run(`duplicate:${theme.id}`, () => api.duplicateTheme(theme.dir));
      if (!copy) return undefined;
      await refreshThemes();
      notify(`Copied ${theme.name} to your themes`);
      return copy;
    },
    [run, refreshThemes, notify],
  );

  const resetConfig = useCallback(async () => {
    const saved = await run("config", () => api.resetConfig());
    if (!saved) return;
    setConfig(saved);
    setConfigError(null);
    await refreshThemes();
  }, [run, refreshThemes]);

  const startEngine = useCallback(async () => {
    const status = await run("engine", () => api.startEngine());
    if (status) setEngine(status);
    await refreshEngine();
  }, [run, refreshEngine]);

  const stopEngine = useCallback(async () => {
    const status = await run("engine", () => api.stopEngine());
    if (status) setEngine(status);
    await refreshEngine();
  }, [run, refreshEngine]);

  const setAutostart = useCallback(
    async (enabled: boolean) => {
      const next = await run("autostart", () => api.setAutostart(enabled));
      if (next) setAutostartState(next);
    },
    [run],
  );

  const createTheme = useCallback(
    async (request: CreateThemeRequest) => {
      const created = await run("create", () => api.createTheme(request));
      if (!created) return undefined;
      await Promise.all([refreshConfig(), refreshThemes()]);
      notify(request.apply ? `Created and applied ${created.name}` : `Created ${created.name}`);
      return created;
    },
    [run, refreshConfig, refreshThemes, notify],
  );

  const value = useMemo<Studio>(
    () => ({
      config,
      configError,
      themes,
      roots,
      themesLoaded,
      engine,
      components,
      autostart,
      notify,
      busy,
      toast,
      pendingPreviews,
      applyTheme,
      duplicateTheme,
      updateConfig,
      resetConfig,
      startEngine,
      stopEngine,
      setAutostart,
      createTheme,
      dismissToast,
    }),
    [
      config,
      configError,
      themes,
      roots,
      themesLoaded,
      engine,
      components,
      autostart,
      notify,
      busy,
      toast,
      pendingPreviews,
      applyTheme,
      duplicateTheme,
      updateConfig,
      resetConfig,
      startEngine,
      stopEngine,
      setAutostart,
      createTheme,
      dismissToast,
    ],
  );

  return <StudioContext value={value}>{children}</StudioContext>;
}
