// Small read-only data hooks for the pages and the stage.

import { useEffect, useState } from "react";

import { api, fileUrl, onChanged, type ActiveSkins, type BarsCatalog, type MonitorInfo, type WidgetInfo } from "./api";
import { resolve, type Resolved } from "./barsDoc";
import type { BarsDoc } from "./barsSchema";
import { useStudio } from "./studio";

/** The wallpaper on screen as a CSS background, for small previews painted over it (look tiles). */
export function useWallpaperBackground(): string | null {
  const { themes } = useStudio();
  const preview = themes.find((t) => t.active)?.preview;
  return preview ? `center / cover no-repeat url("${fileUrl(preview)}")` : null;
}

/** `bars.json` and what the host makes of it (designs, preset, the file), for the stage. Re-read when it changes. */
export type BarsPreview = { doc: BarsDoc; config: Resolved; catalog: BarsCatalog };

export function useBarsPreview(): BarsPreview | null {
  const [preview, setPreview] = useState<BarsPreview | null>(null);
  useEffect(() => {
    let current = true;
    let catalog: Promise<BarsCatalog> | null = null;
    const refresh = () => {
      catalog ??= api.barsCatalog();
      void Promise.all([catalog, api.getBars()]).then(
        ([loaded, state]) => current && !state.problem && setPreview({ doc: state.doc, config: resolve(loaded, state.doc), catalog: loaded }),
        () => undefined,
      );
    };
    refresh();
    const unlisten = onChanged((changes) => changes.bars && refresh());
    return () => {
      current = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  return preview;
}

const NO_SKINS: ActiveSkins = { widgets: {}, bars: {} };

/** The active theme's widget and bar looks: the layer under the user's own choices. */
export function useActiveSkins(): ActiveSkins {
  const [skins, setSkins] = useState<ActiveSkins>(NO_SKINS);
  useEffect(() => {
    let current = true;
    const refresh = () => void api.activeSkins().then((next) => current && setSkins(next), () => undefined);
    refresh();
    const unlisten = onChanged((changes) => (changes.config || changes.themes) && refresh());
    return () => {
      current = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  return skins;
}

/** The connected monitors, read once. Empty until they load. */
export function useMonitors(): MonitorInfo[] {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    let current = true;
    void api.listMonitors().then((list) => current && setMonitors(list), () => undefined);
    return () => {
      current = false;
    };
  }, []);
  return monitors;
}

/** The installed widgets; re-read when a widget folder or the layout changes. */
export function useInstalledWidgets(): WidgetInfo[] | null {
  const [widgets, setWidgets] = useState<WidgetInfo[] | null>(null);
  useEffect(() => {
    let current = true;
    const refresh = () => void api.listWidgets().then((list) => current && setWidgets(list), () => undefined);
    refresh();
    const unlisten = onChanged((changes) => changes.widgets && refresh());
    return () => {
      current = false;
      void unlisten.then((stop) => stop());
    };
  }, []);
  return widgets;
}

// Icons are made once per path per session; a failure is remembered as null.
const icons = new Map<string, Promise<string | null>>();

function iconUrl(path: string): Promise<string | null> {
  let url = icons.get(path);
  if (!url) {
    url = api.dockIcon(path).then(fileUrl, () => null);
    icons.set(path, url);
  }
  return url;
}

/** The shell icon of a pinned item as an image URL: undefined while loading, null if it has none. */
export function useDockIcon(path: string): string | null | undefined {
  const [state, setState] = useState<{ path: string; url: string | null } | null>(null);
  useEffect(() => {
    let current = true;
    void iconUrl(path).then((url) => current && setState({ path, url }));
    return () => {
      current = false;
    };
  }, [path]);
  return state?.path === path ? state.url : undefined;
}
