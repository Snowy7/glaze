// Working with a `bars.json` document: reading and setting paths in it, and computing the values
// the host would end up with. The host merges the file over its preset over its defaults
// (crates/bars/src/presets.rs `merge`); `resolve` does the same merge from the catalog the Rust
// side sends, so the controls show real values the moment a preset or a field changes.

import type { BarsCatalog } from "./api";
import type {
  Attention,
  BarsDoc,
  DockPosition,
  DockStyle,
  Extra,
  Indicator,
  LauncherLayout,
  Look,
  ModuleOptions,
  ModuleRef,
  Monitors,
  Motion,
  Performance,
  Pinned,
  PresetId,
  TaskbarAlignment,
  TaskbarMode,
  TopLayout,
  TopPosition,
  Weekday,
  Zone,
} from "./barsSchema";

export type Path = readonly string[];

type Obj = Record<string, unknown>;

const isObject = (value: unknown): value is Obj => typeof value === "object" && value !== null && !Array.isArray(value);

export function getIn(root: unknown, path: Path): unknown {
  let node = root;
  for (const key of path) {
    if (!isObject(node)) return undefined;
    node = node[key];
  }
  return node;
}

/**
 * `root` with the value at `path` set. `undefined` removes the key, and any object that leaves
 * empty goes with it, so the file never keeps a hollow `"look": {}` the user didn't write.
 * The root itself is never removed.
 */
export function setIn<T extends object>(root: T, path: Path, value: unknown): T {
  const [key, ...rest] = path;
  if (key === undefined) return root;
  const node: Obj = { ...(root as Obj) };
  const child = rest.length === 0 ? value : setIn(isObject(node[key]) ? node[key] : {}, rest, value);
  if (child === undefined || (rest.length > 0 && isObject(child) && Object.keys(child).length === 0)) delete node[key];
  else node[key] = child;
  return node as T;
}

/** Objects merge key by key; anything else (arrays included) is replaced. Same as the host's. */
export function merge(base: unknown, over: unknown): unknown {
  if (!isObject(base) || !isObject(over)) return over;
  const out: Obj = { ...base };
  for (const [key, value] of Object.entries(over)) out[key] = key in out ? merge(out[key], value) : value;
  return out;
}

/** The values a `BarsDoc` ends up with: defaults, then the preset, then the file. */
export type Resolved = {
  preset: PresetId;
  taskbar: { mode: TaskbarMode; alignment?: TaskbarAlignment };
  top: {
    enabled: boolean;
    monitors: Monitors;
    layout: TopLayout;
    position: TopPosition;
    height: number;
    margin: number | null;
    padding: number | null;
    spacing: number | null;
    width: number | string | null;
    left: ModuleRef[];
    center: ModuleRef[];
    right: ModuleRef[];
    clock: { seconds: boolean; format: string | null };
  };
  dock: {
    enabled: boolean;
    monitors: Monitors;
    style: DockStyle;
    position: DockPosition;
    size: number | null;
    margin: number | null;
    padding: number | null;
    spacing: number | null;
    autohide: boolean;
    magnify: boolean | { enabled?: boolean; scale?: number; range?: number };
    running: boolean | "monitor";
    split: boolean;
    counter: boolean;
    titles: boolean;
    indicator: Indicator;
    preview: boolean | { enabled?: boolean; delay?: number };
    bounce: boolean;
    attention: Attention;
    extras: Extra[];
    align: "start" | "center";
    left: ModuleRef[];
    right: ModuleRef[];
    pinned: Pinned[] | null;
  };
  launcher: { winKey: boolean; layout: LauncherLayout; pinned: Pinned[] | null; files: boolean };
  tray: { host: boolean };
  calendar: { zones: Zone[]; firstDay?: Weekday };
  audio: { switchOutput: boolean };
  look: Look;
  animation: Motion;
  modules: Record<string, ModuleOptions>;
  icons: string | null;
  performance: Performance;
};

export function resolve(catalog: BarsCatalog, doc: BarsDoc): Resolved {
  const preset = catalog.presets.find((p) => p.id === (doc.preset ?? "monochrome"));
  const merged = merge(merge(catalog.defaults, preset?.bundle ?? {}), doc);
  // The host parsed the same merge into its typed config, so the shape is the one described above.
  return merged as Resolved;
}

/** The magnify settings with the host's defaults for a bare `true`. */
export function magnifyOf(value: Resolved["dock"]["magnify"]): { enabled: boolean; scale: number; range: number } {
  if (typeof value === "boolean") return { enabled: value, scale: 1.5, range: 3 };
  return { enabled: value.enabled ?? true, scale: value.scale ?? 1.5, range: value.range ?? 3 };
}

export function previewOf(value: Resolved["dock"]["preview"]): { enabled: boolean; delay: number } {
  if (typeof value === "boolean") return { enabled: value, delay: 450 };
  return { enabled: value.enabled ?? true, delay: value.delay ?? 450 };
}

/** Margin, padding and spacing of the top bar with the layout's defaults filled in. */
export function topGeometry(resolved: Resolved, catalog: BarsCatalog) {
  const fallback = catalog.geometry.top[resolved.top.layout];
  const { top } = resolved;
  return { margin: top.margin ?? fallback.margin, padding: top.padding ?? fallback.padding, spacing: top.spacing ?? fallback.spacing };
}

/** Size, margin, padding and spacing of the dock with the style's defaults filled in. */
export function dockGeometry(resolved: Resolved, catalog: BarsCatalog) {
  const fallback = catalog.geometry.dock[resolved.dock.style];
  const { dock } = resolved;
  return {
    size: dock.size ?? fallback.size,
    margin: dock.margin ?? fallback.margin,
    padding: dock.padding ?? fallback.padding,
    spacing: dock.spacing ?? fallback.spacing,
  };
}

/** Rounds away float noise from sliders (0.1 + 0.2) before it reaches the file. */
export const tidy = (value: number): number => Math.round(value * 1000) / 1000;
