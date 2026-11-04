// Everything the desktop stage shows, in one place: the monitors (and which one is on stage), the
// widget layout and grid, the installed widgets, the theme's looks and the bars.

import { useState } from "react";

import type { Grid, MonitorInfo, PlacedWidget } from "./api";
import { desktopGrid, sizeOf, type Placement } from "./desktopGrid";
import { useActiveSkins, useBarsPreview, useInstalledWidgets, useMonitors } from "./hooks";
import { useLayout } from "./layout";
import { useStudio } from "./studio";

/** Until the monitors load (or in a browser): a plain 1080p screen. */
const FALLBACK_MONITOR: MonitorInfo = { device: "\\\\.\\DISPLAY1", primary: true, width: 1920, height: 1080, scale: 1, x: 0, y: 0, work: { x: 0, y: 0, width: 1920, height: 1032 } };
const FALLBACK_GRID: Grid = { cell: 0, gap: 8, margin: 16 };

/** Whether a placed widget shows on `monitor`: its own, or the primary one when its monitor is gone. */
function shownOn(item: PlacedWidget, monitor: MonitorInfo, monitors: readonly MonitorInfo[]): boolean {
  if (item.monitor === monitor.device) return true;
  const connected = item.monitor !== null && monitors.some((m) => m.device === item.monitor);
  return !connected && monitor.primary;
}

export function useDesktop() {
  const { themes } = useStudio();
  const monitors = useMonitors();
  const [device, setDevice] = useState<string | null>(null);
  const monitor = monitors.find((m) => m.device === device) ?? monitors.find((m) => m.primary) ?? monitors[0] ?? FALLBACK_MONITOR;
  const model = useLayout();
  const installed = useInstalledWidgets() ?? [];
  const skins = useActiveSkins();
  const bars = useBarsPreview();
  const layout = model.layout;
  const grid = desktopGrid(monitor, layout?.grid ?? FALLBACK_GRID, layout?.iconCell ?? [96, 96]);
  const items = (layout?.instances ?? []).filter((item) => shownOn(item, monitor, monitors));
  const placements: Placement[] = items.map((item) => ({ instance: item.instance, cell: item.cell ?? [0, 0], size: sizeOf(item, installed.find((w) => w.id === item.widget)) }));
  const active = themes.find((t) => t.active) ?? null;
  return { monitors, monitor, setDevice, model, installed, skins, bars, grid, items, placements, active };
}

export type Desktop = ReturnType<typeof useDesktop>;
