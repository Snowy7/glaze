// Everything the desktop stage shows, in one place: the monitors (and which one is on stage), the
// widget layout and grid, the installed widgets, the theme's looks and the bars.

import { useState } from "react";

import type { BarsCatalog, Grid, MonitorInfo, PlacedWidget } from "./api";
import { dockGeometry, topGeometry, type Resolved } from "./barsDoc";
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

type Insets = { top: number; bottom: number; left: number; right: number };

/**
 * The space the bars reserve once they run, in DIPs (docs/bars.md, "Reserved space"): the top bar's
 * band (height and its margins) on its edge, and a dock's that doesn't auto-hide on its own.
 */
export function reservedBy(config: Resolved, catalog: BarsCatalog): Insets {
  const insets: Insets = { top: 0, bottom: 0, left: 0, right: 0 };
  if (config.top.enabled) insets[config.top.position] += config.top.height + 2 * topGeometry(config, catalog).margin;
  if (config.dock.enabled && !config.dock.autohide) {
    const g = dockGeometry(config, catalog);
    insets[config.dock.position] += g.size + 2 * g.padding + g.margin;
  }
  return insets;
}

/** `monitor` with the work area it will have once the bars hold `insets`. */
function withReserved(monitor: MonitorInfo, insets: Insets): MonitorInfo {
  const k = monitor.scale > 0 ? monitor.scale : 1;
  const left = Math.max(monitor.work.x, insets.left * k);
  const top = Math.max(monitor.work.y, insets.top * k);
  const right = Math.min(monitor.work.x + monitor.work.width, monitor.width - insets.right * k);
  const bottom = Math.min(monitor.work.y + monitor.work.height, monitor.height - insets.bottom * k);
  return { ...monitor, work: { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) } };
}

export function useDesktop() {
  const { themes, components } = useStudio();
  const monitors = useMonitors();
  const [device, setDevice] = useState<string | null>(null);
  const monitor = monitors.find((m) => m.device === device) ?? monitors.find((m) => m.primary) ?? monitors[0] ?? FALLBACK_MONITOR;
  const model = useLayout();
  const installed = useInstalledWidgets() ?? [];
  const skins = useActiveSkins();
  const bars = useBarsPreview();
  const layout = model.layout;
  // Running bars already took their space from the work area; otherwise show where they will.
  const barsRunning = components?.some((c) => c.id === "bars" && c.running) ?? false;
  const space = bars && !barsRunning ? withReserved(monitor, reservedBy(bars.config, bars.catalog)) : monitor;
  const grid = desktopGrid(space, layout?.grid ?? FALLBACK_GRID, layout?.iconCell ?? [96, 96]);
  const items = (layout?.instances ?? []).filter((item) => shownOn(item, monitor, monitors));
  const placements: Placement[] = items.map((item) => ({ instance: item.instance, cell: item.cell ?? [0, 0], size: sizeOf(item, installed.find((w) => w.id === item.widget)) }));
  const active = themes.find((t) => t.active) ?? null;
  return { monitors, monitor, setDevice, model, installed, skins, bars, grid, items, placements, active };
}

export type Desktop = ReturnType<typeof useDesktop>;
