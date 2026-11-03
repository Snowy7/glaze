// Small read-only data hooks for the Bars and Widgets pages.

import { useEffect, useState } from "react";

import { api, fileUrl, onChanged, type ActiveSkins, type MonitorInfo, type WidgetInfo } from "./api";

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
