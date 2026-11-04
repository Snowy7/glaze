// The page ⇄ host wire protocol (docs/bars.md, "Bridge"). The Rust host (crates/bars) speaks the
// other side. Every message is one JSON object through WebView2 `postMessage`.

import type { CalendarConfig, FlyoutEvents, FlyoutMethods, FlyoutName, LauncherConfig } from "./flyouts/protocol";

/** Built-in modules, in the order they are documented. */
export const MODULES = [
  "start",
  "search",
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
/** A built-in module or `custom:<id>` (a command's output). */
export type ModuleName = Module | `custom:${string}`;
/** A module list entry: a module, or `|` (a break between islands, a separator elsewhere). */
export type ModuleEntry = ModuleName | "|";

export type Monitors = "all" | "primary" | string[];
export type TaskbarMode = "keep" | "autohide" | "hide";
export type DockPosition = "bottom" | "top" | "left" | "right";
/** Dock layouts; each has a page layout in dock/styles.ts. */
export const DOCK_STYLES = ["floating", "taskbar"] as const;
export type DockStyle = (typeof DOCK_STYLES)[number];
export type BarKind = "top" | "dock";
/** A pairing of designs (crates/bars/presets/<name>.json). */
export const PRESETS = ["monochrome", "macos", "windows11", "windows10", "chromeos", "islands", "neon-glass", "floating"] as const;
export type Preset = (typeof PRESETS)[number];
/** Top bar designs (crates/bars/presets/top/<id>.json + designs/top.css). */
export const TOP_DESIGNS = ["monochrome", "macos", "fluent", "capsule", "minimal", "islands", "neon", "floating"] as const;
export type TopDesign = (typeof TOP_DESIGNS)[number];
/** Dock designs (crates/bars/presets/dock/<id>.json + designs/dock.css). */
export const DOCK_DESIGNS = ["monochrome", "macos", "windows11", "windows10", "chromeos", "fluent", "islands", "neon", "floating"] as const;
export type DockDesign = (typeof DOCK_DESIGNS)[number];
/** The OS family a bar's flyouts and menus are drawn after. */
export type Family = "mono" | "fluent" | "macos" | "material";
/** The module glyph set. */
export type Glyphs = "default" | "fluent" | "symbols";
/** Liquid Glass (refraction, liquid.ts), frosted (blur), or no glass. */
export type GlassStyle = "liquid" | "frosted" | "none";
export type TopLayout = "strip" | "floating" | "islands";
/** `full`, `auto` (hug the content), DIPs, or `"<n>%"`. */
export type Dimension = "full" | "auto" | number | `${number}%`;

export interface Pinned {
  /** An exe, a `.lnk`, a folder, or `shell:AppsFolder\<AUMID>`. */
  path: string;
  name?: string;
}

// --- Look ------------------------------------------------------------------------------------------

/** `#rgb[a]`, `#rrggbb[aa]`, a token (`accent`, `text`, `transparent`, `system-accent[-light|-dark-1..3]`) or `var(--name)`. */
export type ColorRef = string;

export interface Gradient {
  kind: "linear" | "radial" | "conic";
  /** Degrees (linear, conic). */
  angle: number;
  stops: (ColorRef | { color: ColorRef; at: number })[];
}
export type Paint = ColorRef | Gradient;

export type Fill =
  | { type: "none" }
  | { type: "solid"; color: ColorRef }
  | ({ type: "gradient" } & Gradient)
  | {
      type: "image";
      /** As written in bars.json; `assets` maps it to a URL. */
      path: string;
      fit: "cover" | "contain" | "fill" | "tile";
      position: string;
      brightness: number;
      blur: number;
    }
  | { type: "wallpaper"; brightness: number; blur: number; interval: number }
  /** Live blur: what's behind the window, blurred by Windows (crates/bars/src/blur.rs). */
  | { type: "blur"; brightness: number; blur: number };

export interface PerState<T> {
  rest?: T;
  hover?: T;
  active?: T;
}
export interface BorderStyle {
  width?: number;
  color?: Paint;
}
export interface GlowStyle {
  color?: ColorRef;
  size?: number;
  opacity?: number;
}
export interface ShadowStyle {
  color?: ColorRef;
  x?: number;
  y?: number;
  blur?: number;
  spread?: number;
}
export interface ItemStyle {
  background?: Paint;
  color?: ColorRef;
  border?: BorderStyle;
  glow?: GlowStyle;
  scale?: number;
}
export type Corners = "round" | "squircle" | "bevel" | "scoop" | "notch";

/** `bars.json` → `look` (docs/bars.md, "Look"). */
export interface Look {
  scheme?: "auto" | "light" | "dark";
  family?: Family;
  glyphs?: Glyphs;
  font: { family?: string; size?: number; weight?: number };
  /** The glass material and its knobs (0–1; blur in DIPs). */
  glass: {
    style?: GlassStyle;
    intensity?: number;
    refraction?: number;
    highlight?: number;
    tint?: number;
    blur?: number;
    sheen?: number;
    depth?: number;
  };
  /** The flyouts this bar opens: live blur behind them, and their tint's opacity. */
  flyouts: { blur?: boolean; tint?: number };
  background?: Fill;
  tint?: number;
  saturation?: number;
  noise?: number;
  radius?: number;
  corners?: Corners;
  border: PerState<BorderStyle>;
  glow: PerState<GlowStyle>;
  shadow: PerState<ShadowStyle>;
  items: { radius?: number } & PerState<ItemStyle>;
  indicator: { running?: ColorRef; active?: ColorRef; attention?: ColorRef };
}

// --- Motion ----------------------------------------------------------------------------------------

export interface Spring {
  mass: number;
  stiffness: number;
  damping: number;
}
/** A named curve, a cubic-bezier `[x1, y1, x2, y2]`, or a spring. */
export type Curve = string | [number, number, number, number] | Spring;
/** `morph` (flyouts): grows out of the button that opened it. */
export type MotionStyle = "slide" | "fade" | "pop" | "scale" | "morph" | "none";

/** A node of the animation tree (docs/bars.md, "Animation"). */
export interface Motion {
  enabled?: boolean;
  duration?: number;
  curve?: Curve;
  style?: MotionStyle;
  reduceMotion?: "system" | "always" | "never";
  bar?: Motion;
  top?: Motion;
  dock?: Motion;
  hover?: Motion;
  press?: Motion;
  open?: Motion;
  magnify?: Motion;
  bounce?: Motion;
  attention?: Motion;
}

// --- Modules ---------------------------------------------------------------------------------------

export const BUILTIN_ACTIONS = [
  "default",
  "none",
  "start",
  "search",
  "notifications",
  "quick-settings",
  "calendar",
  "network",
  "mixer",
  "native-start",
  "native-notifications",
  "native-quick-settings",
  "task-manager",
  "settings",
  "show-desktop",
  "media-toggle",
  "media-next",
  "media-previous",
  "volume-up",
  "volume-down",
  "volume-mute",
  "workspace-next",
  "workspace-previous",
  "lock",
  "explorer",
] as const;
export type BuiltinAction = (typeof BUILTIN_ACTIONS)[number];
export type Action = BuiltinAction | { exec: string; args: string[] } | { open: string };
export type Trigger = "left" | "middle" | "right" | "wheelUp" | "wheelDown";

export interface ModuleStyle {
  color?: ColorRef;
  background?: Paint;
  border?: BorderStyle;
  padding?: number;
  radius?: number;
  size?: number;
  weight?: number;
}

export interface ModuleOptions {
  click: Partial<Record<Trigger, Action>>;
  style: ModuleStyle;
  /** An icon-pack key or an image file (see `assets`). */
  icon?: string;
  command?: string;
  args?: string[];
  interval?: number;
}

export interface ClockConfig {
  seconds: boolean;
  format: string | null;
}

export interface SystemClock {
  time: string;
  timeWithSeconds: string;
  date: string;
}

/** `bars.json`, resolved for this bar's monitor, with every default filled in by the host. */
export interface BarsConfig {
  version: 1;
  preset: Preset;
  taskbar: { mode: TaskbarMode; alignment?: "center" | "left" };
  top: {
    design: TopDesign;
    enabled: boolean;
    monitors: Monitors;
    layout: TopLayout;
    position: "top" | "bottom";
    /** DIPs. */
    height: number;
    margin: number;
    padding: number;
    spacing: number;
    width: Dimension;
    left: ModuleEntry[];
    center: ModuleEntry[];
    right: ModuleEntry[];
    clock: ClockConfig;
    look: Look;
    animation: Motion;
  };
  dock: {
    design: DockDesign;
    enabled: boolean;
    monitors: Monitors;
    style: DockStyle;
    position: DockPosition;
    /** Icon size in DIPs. The band is `size + 2 * padding + 2 * margin` (see `dockBand`). */
    size: number;
    margin: number;
    padding: number;
    spacing: number;
    autohide: boolean;
    magnify: { enabled: boolean; scale: number; range: number };
    /** Running apps that aren't pinned: all, none, or those on this dock's monitor. */
    running: boolean | "monitor";
    split: boolean;
    counter: boolean;
    titles: boolean;
    indicator: "dot" | "bar" | "glow" | "pill" | "none";
    preview: { enabled: boolean; delay: number };
    bounce: boolean;
    attention: "pulse" | "bounce" | "none";
    extras: ("desktop" | "trash")[];
    align: "start" | "center";
    left: ModuleEntry[];
    right: ModuleEntry[];
    /** A `taskbar` task button's length along the bar, DIPs (missing: the icon plus 16). */
    button?: number;
    clock: ClockConfig;
    pinned: Pinned[];
    look: Look;
    animation: Motion;
  };
  modules: Partial<Record<ModuleName, ModuleOptions>>;
  icons?: string;
  performance: "auto" | "on" | "off";
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
  /** The appearance's design style by name (the widgets'; `liquid` makes glass liquid by default). */
  style?: string;
}

/** Windows' colors: `[base, light-1, light-2, light-3, dark-1, dark-2, dark-3]`. */
export interface SystemLook {
  accent: [string, string, string, string, string, string, string];
  scheme: "light" | "dark";
}

export interface IconPackEntry {
  umid?: string;
  path?: string;
  extension?: string;
  key?: string;
  icon: string;
  mono?: boolean;
}
export interface IconPack {
  name: string;
  mono: boolean;
  missing?: string;
  entries: IconPackEntry[];
  /** Where the pack's files are served; ends with `/`. */
  base: string;
}

/** Everything about a bar's look besides its config (the `appearance` event). */
export interface LookPayload {
  appearance: Appearance;
  themeCss: string | null;
  system: SystemLook;
  /** The theme's variables, as CSS values. */
  vars: Record<string, string>;
  iconPack: IconPack | null;
  /** Image paths in bars.json → their URLs. */
  assets: Record<string, string>;
  /** The theme's liquid-glass knobs (`bars.glass`): what the bar's `look.glass` leaves out. */
  glass?: { intensity?: number; refraction?: number; highlight?: number } | null;
}

export interface Mode {
  reduceMotion: boolean;
  /** No blur and no animation (on battery, or chosen). */
  performance: boolean;
  /** Live blur draws under this window's glass (Windows 11, transparency effects on, not performance mode). */
  blur: boolean;
  /** Liquid glass may draw (transparency effects on, not performance mode). */
  liquid: boolean;
}

/** A piece of glass for live blur: `rect` in DIPs from the window's top-left. */
export interface Pane {
  rect: [number, number, number, number];
  radius: number;
  opacity: number;
  saturation: number;
  brightness: number;
  blur: number;
}

/** The wallpaper snapshot of this bar's monitor: its size and the window's offset on it, DIPs. */
export interface Backdrop {
  url: string;
  width: number;
  height: number;
  x: number;
  y: number;
}

// --- Geometry shared with the host (DIPs) ------------------------------------------------------

/**
 * Extra room the host gives every bar window beyond its band, for flyouts, menus and magnify: the
 * most a flyout can extend past its bar. The page lays out within the window it gets.
 */
export const ROOM = 360;
/** What a dock reserves: its icons, padding, and its margin on both sides (room for its shadow). */
export const dockBand = ({ size, padding, margin }: Pick<BarsConfig["dock"], "size" | "padding" | "margin">): number =>
  size + 2 * padding + 2 * margin;
/** What a top bar reserves: its height and its margin on both sides. */
export const topBand = ({ height, margin }: Pick<BarsConfig["top"], "height" | "margin">): number => height + 2 * margin;

// --- Host → page ---------------------------------------------------------------------------------

export interface Hello {
  bar: BarKind;
  /** The monitor this bar is on; sizes in DIPs. */
  monitor: { device: string; primary: boolean; width: number; height: number };
  config: BarsConfig;
  look: LookPayload;
  mode: Mode;
  backdrop: Backdrop | null;
  /** The monitor's DPI (96 = 100%). CSS pixels are DIPs. */
  dpi: number;
}

export interface DockWindow {
  hwnd: number;
  title: string;
  /** The window that had the foreground before the bars were clicked. */
  active: boolean;
  minimized: boolean;
  /** Flashing for attention since it was last in front. */
  attention: boolean;
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
  /** What identifies the app, for icon packs. */
  aumid?: string;
  /** Lowercase. */
  exe?: string;
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

export interface CustomOutput {
  text: string;
  icon?: string;
  tooltip?: string;
  class?: string;
  error?: boolean;
}

export interface EventMap extends FlyoutEvents {
  apps: DockItem[];
  foreground: { name: string; icon?: string } | null;
  /** Milliseconds since the Unix epoch; the page formats it. */
  clock: { now: number; system?: SystemClock | null };
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
  /** Custom modules' outputs, by id. */
  custom: Record<string, CustomOutput>;
  trash: { full: boolean };
  appearance: LookPayload;
  mode: Mode;
  backdrop: Backdrop | null;
  config: BarsConfig;
  /** False while this bar is hidden (a fullscreen app) or suspended: stop animations. */
  visibility: boolean;
  /** Auto-hide dock: the pointer touched the screen edge, slide in. */
  reveal: null;
  /** What's behind this bar may have changed (its window moved, the wallpaper changed): liquid glass captures again. */
  behind: unknown;
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
  /** A flyout failed to open: the host logs it and opens the native panel in its place. */
  flyoutFailed: { params: { flyout: FlyoutName; error: string }; result: null };
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
  /** Where the page's live-blur glass is; sent every frame one moves (glass.ts). */
  glass: { params: { panes: Pane[] }; result: null };
  /** What's behind `rect` (DIPs from the window's top-left) without our windows, for liquid glass: a URL and the rect it covers. */
  capture: { params: { rect: [number, number, number, number] }; result: { url: string; rect: [number, number, number, number] } };
  /** Runs the `exec`/`open` action bars.json binds to a module's input. */
  run: { params: { module: ModuleName; trigger: Trigger }; result: null };
  /** Windows Search. */
  search: { params: undefined; result: null };
  /** Minimize everything, or bring it back. */
  showDesktop: { params: undefined; result: null };
  trash: { params: { action: "open" | "empty" }; result: null };
  /** Registers live thumbnails of these windows into the bar; returns their sizes in px. */
  previewOpen: { params: { hwnds: number[] }; result: { hwnd: number; width: number; height: number }[] };
  /** Shows each thumbnail at its rect (DIPs from the window's top-left); unlisted ones hide. */
  previewPlace: { params: { windows: { hwnd: number; rect: [number, number, number, number] }[] }; result: null };
  previewClose: { params: undefined; result: null };
}

export type Method = keyof MethodMap;
export type Request = {
  [M in Method]: { id: number; method: M; params: MethodMap[M]["params"] };
}[Method];
