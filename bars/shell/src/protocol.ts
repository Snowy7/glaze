// The page ⇄ host wire protocol (docs/bars.md, "Bridge"). The Rust host (crates/bars) speaks the
// other side. Every message is one JSON object through WebView2 `postMessage`.

import type { CalendarConfig, FlyoutEvents, FlyoutMethods, LauncherConfig } from "./flyouts/protocol";

/** Top-bar modules, in the order they are documented. */
export const MODULES = [
  "start",
  "workspaces",
  "app",
  "clock",
  "media",
  "tray",
  "network",
  "volume",
  "battery",
  "quick",
  "power",
  "stats",
  "language",
  "notifications",
] as const;
export type Module = (typeof MODULES)[number];

export type Monitors = "all" | "primary" | string[];
export type TaskbarMode = "keep" | "autohide" | "hide";
export type DockPosition = "bottom" | "left" | "right";
/** Dock layouts; each has a page layout in dock/styles.ts and a band size in the host. */
export const DOCK_STYLES = ["floating"] as const;
export type DockStyle = (typeof DOCK_STYLES)[number];
export type BarKind = "top" | "dock";

export interface Pinned {
  /** An exe, a `.lnk`, a folder, or `shell:AppsFolder\<AUMID>`. */
  path: string;
  name?: string;
}

/** `bars.json`, parsed and with every default filled in by the host. */
export interface BarsConfig {
  version: 1;
  taskbar: { mode: TaskbarMode; alignment?: "center" | "left" };
  top: {
    enabled: boolean;
    monitors: Monitors;
    /** DIPs. */
    height: number;
    left: Module[];
    center: Module[];
    right: Module[];
    clock: { seconds: boolean; format: string };
  };
  dock: {
    enabled: boolean;
    monitors: Monitors;
    style: DockStyle;
    position: DockPosition;
    /** Icon size in DIPs. For `floating` the dock's band is `size + 16` (see DOCK_PAD and DOCK_GAP). */
    size: number;
    autohide: boolean;
    magnify: boolean;
    running: boolean;
    pinned: Pinned[];
  };
  launcher: LauncherConfig;
  tray: { host: boolean };
  calendar: CalendarConfig;
  /** `switchOutput`: the mixer may switch the default output (an undocumented API). */
  audio: { switchOutput: boolean };
}

/** Resolved appearance: every field set (docs/widgets.md, "Appearance"). */
export interface Appearance {
  surface: "none" | "glass" | "solid";
  tint: string;
  opacity: number;
  blur: number;
  radius: number;
  border: boolean;
  text: "light" | "dark";
  accent: string;
  font: string;
  scale: number;
  shadow: boolean;
}

// --- Geometry shared with the host (DIPs) ------------------------------------------------------

/**
 * Extra room the host gives every bar window beyond its band, for flyouts, menus and magnify: the
 * most a flyout can extend past its bar. The page lays out within the window it gets.
 */
export const ROOM = 360;
/** The dock pill's padding around its icons. */
export const DOCK_PAD = 5;
/** Gap between the screen edge and the dock pill. */
export const DOCK_GAP = 6;
/** The dock's band (what it reserves when it doesn't auto-hide): `size + 2 * DOCK_PAD + DOCK_GAP`. */
export const dockBand = (size: number): number => size + 2 * DOCK_PAD + DOCK_GAP;

// --- Host → page ---------------------------------------------------------------------------------

export interface Hello {
  bar: BarKind;
  /** The monitor this bar is on; sizes in DIPs. */
  monitor: { device: string; primary: boolean; width: number; height: number };
  config: BarsConfig;
  appearance: Appearance;
  themeCss: string | null;
  /** The monitor's DPI (96 = 100%). CSS pixels are DIPs. */
  dpi: number;
}

export interface DockWindow {
  hwnd: number;
  title: string;
  /** The window that had the foreground before the bars were clicked. */
  active: boolean;
  minimized: boolean;
}

export interface DockItem {
  /** Stable id of the item: what `activate { app }` takes. */
  key: string;
  name: string;
  /** What `launch`, `pin`, `unpin` and `reorder` take. */
  path: string;
  /** A URL (https://icons.bars.mypaper/<file>.png), absent while the icon is being made. */
  icon?: string;
  pinned: boolean;
  /** Empty when the app isn't running. Most recent first. */
  windows: DockWindow[];
}

export interface MediaInfo {
  title: string;
  artist: string;
  app: string;
  playing: boolean;
}

export interface NetworkInfo {
  kind: "wifi" | "ethernet" | "none";
  name?: string;
  /** Wi-Fi signal quality, 0–100. */
  signal?: number;
}

export interface TrayItem {
  /** "overflow" is the fallback button that opens the native overflow flyout. */
  id: string;
  name: string;
  icon?: string;
  /** Shown in the bar (the user pinned it in Windows); the rest are in the overflow. */
  pinned: boolean;
}

export interface EventMap extends FlyoutEvents {
  apps: DockItem[];
  foreground: { name: string; icon?: string } | null;
  /** Milliseconds since the Unix epoch; the page formats it. */
  clock: { now: number };
  media: MediaInfo | null;
  /** `level` is 0–1. Null when there is no output device. */
  volume: { level: number; muted: boolean } | null;
  network: NetworkInfo;
  battery: { percent: number; charging: boolean } | null;
  tray: TrayItem[];
  /** `current` is 0-based. */
  workspaces: { count: number; current: number };
  /** `cpu` is percent 0–100; memory in bytes. */
  stats: { cpu: number; memory: { used: number; total: number } };
  appearance: { appearance: Appearance; themeCss: string | null };
  config: BarsConfig;
  /** False while this bar is hidden (a fullscreen app) or suspended: stop animations. */
  visibility: boolean;
  /** Auto-hide dock: the pointer touched the screen edge, slide in. */
  reveal: null;
}

export type EventName = keyof EventMap;
export type HostEvent = { [K in EventName]: { event: K; data: EventMap[K] } }[EventName];

export type ErrorCode = "InvalidParams" | "NotFound" | "Unavailable" | "Internal";

export type HostReply =
  | { id: number; result: unknown }
  | { id: number; error: { code: ErrorCode; message: string } };

// --- Page → host ---------------------------------------------------------------------------------

export interface MethodMap extends FlyoutMethods {
  hello: { params: undefined; result: Hello };
  activate: { params: { app: string } | { hwnd: number }; result: null };
  launch: { params: { path: string; newInstance?: boolean }; result: null };
  minimize: { params: { hwnd: number }; result: null };
  close: { params: { hwnd: number }; result: null };
  /** Pins at `index` among the pinned items (default: last). */
  pin: { params: { path: string; name?: string; index?: number }; result: null };
  unpin: { params: { path: string }; result: null };
  /** The pinned items' new order (a permutation of their paths). */
  reorder: { params: { paths: string[] }; result: null };
  start: { params: undefined; result: null };
  actionCenter: { params: { panel: "notifications" | "quick" }; result: null };
  volume: {
    params: { set?: number; mute?: boolean; delta?: number };
    result: { level: number; muted: boolean };
  };
  media: { params: { action: "toggle" | "next" | "previous" }; result: null };
  /** `rect`: the icon in the bar (DIPs from the window's top-left), for apps that place a popup beside it. */
  tray: {
    params: { id: string; button: "left" | "right" | "middle" | "double"; rect?: [number, number, number, number] };
    result: null;
  };
  power: { params: { action: "lock" | "sleep" | "signout" | "restart" | "shutdown" }; result: null };
  workspace: { params: { direction: "left" | "right" }; result: null };
  /** An `ms-settings:` URI only. */
  openSettings: { params: { uri: string }; result: null };
  /**
   * The parts of the window that are visible and take the pointer, as `[x, y, width, height]` in
   * DIPs from the window's top-left. Everything else clicks through. Send it whenever it changes.
   */
  region: { params: { rects: [number, number, number, number][] }; result: null };
}

export type Method = keyof MethodMap;
export type Request = {
  [M in Method]: { id: number; method: M; params: MethodMap[M]["params"] };
}[Method];
