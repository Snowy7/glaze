// The `bars.json` schema as the page needs it (docs/bars.md; crates/bars/src/config.rs is the
// source of truth). Studio never mirrors the schema's rules: the Rust side checks every write with
// the host's own types. This file only names things: the lists the controls offer and the shape of
// the document. The `export const X = [ … ] as const` lists are read by a Rust test
// (src-tauri/src/bars.rs) that feeds each entry to the host's parser, so a wrong name fails there.
// Keep those lists free of comments.

import type { Appearance } from "./api";

export const BUILTIN_MODULES = ["start", "search", "workspaces", "app", "clock", "media", "tray", "network", "volume", "battery", "quick", "power", "stats", "language", "notifications"] as const;
export type BuiltinModule = (typeof BUILTIN_MODULES)[number];

export const BUILTIN_ACTIONS = ["default", "none", "start", "search", "notifications", "quick-settings", "task-manager", "settings", "show-desktop", "media-toggle", "media-next", "media-previous", "volume-up", "volume-down", "volume-mute", "workspace-next", "workspace-previous", "lock", "explorer"] as const;
export type BuiltinAction = (typeof BUILTIN_ACTIONS)[number];

export const NAMED_CURVES = ["linear", "ease", "ease-in", "ease-out", "ease-in-out", "smooth", "snappy", "emphasized", "overshoot", "spring", "spring-soft", "spring-bouncy"] as const;
export type NamedCurve = (typeof NAMED_CURVES)[number];

export const TOP_LAYOUTS = ["strip", "floating", "islands"] as const;
export type TopLayout = (typeof TOP_LAYOUTS)[number];
export const TOP_POSITIONS = ["top", "bottom"] as const;
export type TopPosition = (typeof TOP_POSITIONS)[number];
export const DOCK_STYLES = ["floating", "taskbar"] as const;
export type DockStyle = (typeof DOCK_STYLES)[number];
export const DOCK_POSITIONS = ["bottom", "top", "left", "right"] as const;
export type DockPosition = (typeof DOCK_POSITIONS)[number];
export const INDICATORS = ["dot", "bar", "glow", "pill", "none"] as const;
export type Indicator = (typeof INDICATORS)[number];
export const ATTENTIONS = ["pulse", "bounce", "none"] as const;
export type Attention = (typeof ATTENTIONS)[number];
export const LAUNCHER_LAYOUTS = ["win11", "grid", "compact", "dashboard"] as const;
export type LauncherLayout = (typeof LAUNCHER_LAYOUTS)[number];
export const CORNERS = ["round", "squircle", "bevel", "scoop", "notch"] as const;
export type Corner = (typeof CORNERS)[number];
export const MOTION_STYLES = ["slide", "fade", "pop", "scale", "none"] as const;
export type MotionStyle = (typeof MOTION_STYLES)[number];
export const COLOR_TOKENS = ["accent", "text", "transparent", "system-accent", "system-accent-light-1", "system-accent-light-2", "system-accent-light-3", "system-accent-dark-1", "system-accent-dark-2", "system-accent-dark-3"] as const;
export type ColorToken = (typeof COLOR_TOKENS)[number];

export type PresetId = "monochrome" | "macos" | "windows11" | "islands" | "neon-glass" | "floating";
export type TaskbarMode = "keep" | "autohide" | "hide";
export type TaskbarAlignment = "center" | "left";
export type Performance = "auto" | "on" | "off";
export type ReduceMotion = "system" | "always" | "never";
export type Scheme = "auto" | "light" | "dark";
export type Weekday = "monday" | "sunday" | "saturday";

/** Any JSON value; the document's free-form parts (`vars`, `overrides`) hold these. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** `all`, `primary`, or device names such as `\\.\DISPLAY2`. */
export type Monitors = "all" | "primary" | string[];
/** A module list entry: a built-in module, `custom:<id>`, or `|` (a break). */
export type ModuleRef = string;
/** `full`, `auto`, DIPs, or a share such as `"60%"`. */
export type Dimension = "full" | "auto" | number | string;

// --- Look ---

/** `#rgb`…`#rrggbbaa`, a token, or `var(--name)`. */
export type Color = string;
export type Stop = Color | { color: Color; at: number };
export type Gradient = { kind?: "linear" | "radial" | "conic"; angle?: number; stops: Stop[] };
/** A border or item paint: a color, or a gradient. */
export type Paint = Color | Gradient;

export type ImageFit = "cover" | "contain" | "fill" | "tile";
export type Fill =
  | { type: "none" }
  | { type: "solid"; color: Color }
  | ({ type: "gradient" } & Gradient)
  | { type: "image"; path: string; fit?: ImageFit; position?: string; brightness?: number; blur?: number }
  | { type: "wallpaper"; brightness?: number; blur?: number; interval?: number };
export type FillType = Fill["type"];

export type Border = { width?: number; color?: Paint };
export type Glow = { color?: Color; size?: number; opacity?: number };
export type Shadow = { color?: Color; x?: number; y?: number; blur?: number; spread?: number };
export type PerState<T> = { rest?: T; hover?: T; active?: T };
export type ItemState = { background?: Paint; color?: Color; border?: Border; glow?: Glow; scale?: number };

export const STATES = ["rest", "hover", "active"] as const;
export type State = (typeof STATES)[number];

export type Look = {
  scheme?: Scheme;
  background?: Fill;
  tint?: number;
  saturation?: number;
  noise?: number;
  radius?: number;
  corners?: Corner;
  border?: PerState<Border>;
  glow?: PerState<Glow>;
  shadow?: PerState<Shadow>;
  items?: { radius?: number } & PerState<ItemState>;
  indicator?: { running?: Color; active?: Color; attention?: Color };
};

// --- Animation ---

export type Spring = { mass: number; stiffness: number; damping: number };
export type Curve = NamedCurve | [number, number, number, number] | Spring;

/** One node of the animation tree (docs/bars.md, "Animation"). */
export type Motion = {
  enabled?: boolean;
  duration?: number;
  curve?: Curve;
  style?: MotionStyle;
  reduceMotion?: ReduceMotion;
  bar?: Motion;
  top?: Motion;
  dock?: Motion;
  hover?: Motion;
  press?: Motion;
  open?: Motion;
  magnify?: Motion;
  bounce?: Motion;
  attention?: Motion;
};

// --- Modules ---

export type Action = BuiltinAction | { exec: string; args?: string[] } | { open: string };
export const TRIGGERS = ["left", "middle", "right", "wheelUp", "wheelDown"] as const;
export type Trigger = (typeof TRIGGERS)[number];
export type Clicks = { [T in Trigger]?: Action };
export type ModuleStyle = { color?: Color; background?: Paint; border?: Border; padding?: number; radius?: number; size?: number; weight?: number };
export type ModuleOptions = {
  click?: Clicks;
  style?: ModuleStyle;
  icon?: string;
  command?: string;
  args?: string[];
  interval?: number;
};

// --- Sections ---

export type Pinned = { path: string; name?: string };
export type Zone = { zone: string; label?: string };
export type Magnify = boolean | { enabled?: boolean; scale?: number; range?: number };
export type Preview = boolean | { enabled?: boolean; delay?: number };
export type Running = boolean | "monitor";
export type Extra = "desktop" | "trash";

export type TopSection = {
  enabled?: boolean;
  monitors?: Monitors;
  layout?: TopLayout;
  position?: TopPosition;
  height?: number;
  margin?: number | null;
  padding?: number | null;
  spacing?: number | null;
  width?: Dimension | null;
  left?: ModuleRef[];
  center?: ModuleRef[];
  right?: ModuleRef[];
  clock?: { seconds?: boolean; format?: string | null };
};

export type DockSection = {
  enabled?: boolean;
  monitors?: Monitors;
  style?: DockStyle;
  position?: DockPosition;
  size?: number | null;
  margin?: number | null;
  padding?: number | null;
  spacing?: number | null;
  autohide?: boolean;
  magnify?: Magnify;
  running?: Running;
  split?: boolean;
  counter?: boolean;
  titles?: boolean;
  indicator?: Indicator;
  preview?: Preview;
  bounce?: boolean;
  attention?: Attention;
  extras?: Extra[];
  align?: "start" | "center";
  left?: ModuleRef[];
  right?: ModuleRef[];
  pinned?: Pinned[] | null;
};

/** `appearance`: the user's layer shared with the theme and widgets. */
export type AppearanceLayer = Appearance;

/** A `bars.json` document, or the part of one an override may hold. Everything is optional. */
export type BarsDoc = {
  version?: number;
  preset?: PresetId;
  taskbar?: { mode?: TaskbarMode; alignment?: TaskbarAlignment };
  top?: TopSection;
  dock?: DockSection;
  launcher?: { winKey?: boolean; layout?: LauncherLayout; pinned?: Pinned[] | null; files?: boolean };
  tray?: { host?: boolean };
  calendar?: { zones?: Zone[]; firstDay?: Weekday };
  audio?: { switchOutput?: boolean };
  look?: Look;
  animation?: Motion;
  modules?: Record<string, ModuleOptions>;
  icons?: string | null;
  performance?: Performance;
  vars?: Record<string, Json>;
  appearance?: AppearanceLayer;
  overrides?: Record<string, BarsDoc>;
};

/** Sections a per-monitor override may set (docs/bars.md, "Per-monitor overrides"). */
export const OVERRIDE_SECTIONS = ["preset", "top", "dock", "look", "animation", "modules", "performance", "appearance"] as const;

export function isBuiltinModule(ref: ModuleRef): ref is BuiltinModule {
  return (BUILTIN_MODULES as readonly string[]).includes(ref);
}

export const BREAK: ModuleRef = "|";

export function isCustom(ref: ModuleRef): boolean {
  return ref.startsWith("custom:");
}

/** `custom:weather` -> `weather`. */
export function customId(ref: ModuleRef): string {
  return ref.slice("custom:".length);
}

/** Ids of custom modules are 1 to 32 of `a-z 0-9 - _`. */
export const CUSTOM_ID = /^[a-z0-9_-]{1,32}$/;
