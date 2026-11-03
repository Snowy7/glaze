// The one place that talks to the Rust side. Mirrors src-tauri/src/commands.rs:
// change a command there, change its wrapper here. Errors arrive as strings.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

/** `%APPDATA%\mypaper\config.json`, exactly as the engine reads it. */
export type Config = {
  theme: string;
  paused: boolean;
  pause_on_battery: boolean;
  pause_on_fullscreen: boolean;
};

export type Kind = "shader" | "image" | "slideshow" | "video" | "scene" | "web" | "app";
export type Fit = "cover" | "contain" | "stretch" | "center";
export type Origin = "user" | "bundled" | "external";

export type ThemeEntry = {
  id: string;
  name: string;
  dir: string;
  origin: Origin;
  /** Null when theme.json fails to load; see `error`. */
  kind: Kind | null;
  error: string | null;
  /** Absolute path of an image to show on the card. */
  preview: string | null;
  /** A video theme whose thumbnail is still being made. */
  previewPending: boolean;
  /** The theme has a `windows` styling section. */
  styled: boolean;
  /** The theme sets how widgets / the bars look. */
  widgetsSkin: boolean;
  barsSkin: boolean;
  active: boolean;
  /** What to write to `config.theme` to select this theme. */
  applyAs: string;
};

export type ThemeList = { themes: ThemeEntry[]; roots: string[] };

export type PresetInfo = { id: string; label: string; args: string };

export type SourceInfo = {
  /** The absolute path, or the web address. */
  source: string;
  kind: Kind;
  usesFit: boolean;
  suggestedName: string;
  /** Launch-argument presets to offer (programs only). */
  presets: PresetInfo[];
  /** Arguments to start with, e.g. the Unity preset for a Unity player. */
  defaultArgs: string;
};

export type CreateThemeRequest = {
  source: string;
  name: string;
  fit: Fit | null;
  /** Launch arguments for programs, as one command-line style string. */
  args: string | null;
  apply: boolean;
};

export type Corners = "default" | "round" | "small" | "square";
export type Backdrop = "auto" | "none" | "mica" | "acrylic" | "tabbed";

/** A theme's `windows` section. Null means "don't change". Colors are `#rrggbb`; accent and border also take `"auto"`. */
export type WindowsStyle = {
  accent: string | null;
  border: string | null;
  caption: string | null;
  caption_text: string | null;
  corners: Corners | null;
  backdrop: Backdrop | null;
  dark_mode: boolean | null;
};

export type EngineStatus = {
  running: boolean;
  wallpaperRunning: boolean;
  corePath: string | null;
};

export type ComponentId = "core" | "wallpaper" | "widgets" | "bars";

/** One mypaper program: is it running, and where Studio found it. */
export type Component = { id: ComponentId; exe: string; running: boolean; path: string | null };

export type Autostart = { enabled: boolean; command: string | null };
export type Changes = { config: boolean; themes: boolean; bars: boolean; widgets: boolean };

// --- Appearance (docs/widgets.md, "Appearance") ---

export type Surface = "none" | "glass" | "solid";
export type TextTone = "light" | "dark";

/** One layer of how widgets or bars look. An absent field means "use the layer below". */
export type Appearance = {
  surface?: Surface;
  /** `#rrggbb`. */
  tint?: string;
  /** 0 to 1. */
  opacity?: number;
  /** 0 to 64 px. */
  blur?: number;
  /** 0 to 48 px. */
  radius?: number;
  border?: boolean;
  text?: TextTone;
  /** `#rrggbb` or `auto`. */
  accent?: string;
  /** A CSS font-family list. */
  font?: string;
  /** 0.5 to 2. */
  scale?: number;
  shadow?: boolean;
};

export type SkinKind = "widgets" | "bars";

/** A theme's `widgets` / `bars` section. */
export type SkinEdit = { appearance: Appearance; css: string | null };

export type ActiveSkins = { widgets: Appearance; bars: Appearance };

// --- Bars (docs/bars.md, "Config") ---

export const BAR_MODULES = [
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
] as const;
export type BarModule = (typeof BAR_MODULES)[number];

/** `all`, `primary`, or device names such as `\\.\DISPLAY2`. */
export type Monitors = "all" | "primary" | string[];
export type TaskbarMode = "keep" | "autohide" | "hide";
export type TaskbarAlignment = "center" | "left";
export type DockPosition = "bottom" | "left" | "right";

export type PinnedItem = { path: string; name?: string };

export type BarsConfig = {
  version: number;
  taskbar: { mode: TaskbarMode; alignment?: TaskbarAlignment };
  top: {
    enabled: boolean;
    monitors: Monitors;
    height: number;
    left: BarModule[];
    center: BarModule[];
    right: BarModule[];
    clock?: { seconds: boolean; format?: string };
  };
  dock: {
    enabled: boolean;
    monitors: Monitors;
    position: DockPosition;
    size: number;
    autohide: boolean;
    magnify: boolean;
    running: boolean;
    pinned: PinnedItem[];
  };
  appearance: Appearance;
};

/** Size limits `bars.json` is checked against (mirrors bars.rs). */
export const TOP_HEIGHT = { min: 20, max: 64 } as const;
export const DOCK_SIZE = { min: 24, max: 128 } as const;

export type BarsState = {
  config: BarsConfig;
  /** False: these are the built-in defaults and no file has been written yet. */
  exists: boolean;
  path: string;
  warnings: string[];
};

export type MonitorInfo = { device: string; primary: boolean; width: number; height: number };

// --- Widgets (docs/widgets.md) ---

export type WidgetInfo = {
  id: string;
  name: string;
  version: string | null;
  author: string | null;
  description: string | null;
  /** In grid cells, `[columns, rows]`. */
  size: { default: [number, number]; min: [number, number]; max: [number, number] | null };
  resizable: boolean;
  permissions: string[];
  origin: "bundled" | "user";
  dir: string;
  /** The manifest doesn't load. */
  error: string | null;
};

export type PlacedWidget = {
  instance: string;
  widget: string;
  monitor: string | null;
  cell: [number, number] | null;
  size: [number, number] | null;
  appearance: Appearance;
};

export type Layout = {
  /** The user layer: `widgets.json` → `appearance`. */
  appearance: Appearance;
  instances: PlacedWidget[];
  exists: boolean;
};

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  updateConfig: (patch: Partial<Config>) => invoke<Config>("update_config", { patch }),
  resetConfig: () => invoke<Config>("reset_config"),
  listThemes: () => invoke<ThemeList>("list_themes"),
  inspectSource: (path: string) => invoke<SourceInfo>("inspect_source", { path }),
  inspectUrl: (url: string) => invoke<SourceInfo>("inspect_url", { url }),
  createTheme: (request: CreateThemeRequest) => invoke<ThemeEntry>("create_theme", { request }),
  generatePreview: (dir: string) => invoke<void>("generate_preview", { dir }),
  duplicateTheme: (dir: string) => invoke<ThemeEntry>("duplicate_theme", { dir }),
  getStyle: (dir: string) => invoke<WindowsStyle>("get_style", { dir }),
  setStyle: (dir: string, style: WindowsStyle) => invoke<WindowsStyle>("set_style", { dir, style }),
  getSkin: (dir: string, kind: SkinKind) => invoke<SkinEdit>("get_skin", { dir, kind }),
  setSkin: (dir: string, kind: SkinKind, skin: SkinEdit) => invoke<SkinEdit>("set_skin", { dir, kind, skin }),
  /** Copies a style sheet into the theme; resolves to the path for the skin's `css`. */
  importSkinCss: (dir: string, kind: SkinKind, source: string) => invoke<string>("import_skin_css", { dir, kind, source }),
  activeSkins: () => invoke<ActiveSkins>("active_skins"),
  getBars: () => invoke<BarsState>("get_bars"),
  setBars: (config: BarsConfig) => invoke<BarsState>("set_bars", { config }),
  resetBars: () => invoke<BarsState>("reset_bars"),
  listMonitors: () => invoke<MonitorInfo[]>("list_monitors"),
  /** The absolute path of a PNG icon for a dock item; load it with `fileUrl`. */
  dockIcon: (path: string) => invoke<string>("dock_icon", { path }),
  listWidgets: () => invoke<WidgetInfo[]>("list_widgets"),
  getLayout: () => invoke<Layout>("get_layout"),
  setWidgetsAppearance: (appearance: Appearance) => invoke<Layout>("set_widgets_appearance", { appearance }),
  removeInstance: (instance: string) => invoke<Layout>("remove_instance", { instance }),
  resetInstanceLook: (instance: string) => invoke<Layout>("reset_instance_look", { instance }),
  toggleWidgetEdit: () => invoke<void>("toggle_widget_edit"),
  components: () => invoke<Component[]>("components"),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  startEngine: () => invoke<EngineStatus>("start_engine"),
  stopEngine: () => invoke<EngineStatus>("stop_engine"),
  getAutostart: () => invoke<Autostart>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<Autostart>("set_autostart", { enabled }),
};

/** Runs `handler` when a file Studio follows changes on disk. */
export function onChanged(handler: (changes: Changes) => void): Promise<UnlistenFn> {
  return listen<Changes>("studio:changed", (event) => handler(event.payload));
}

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "heic", "heif"];
const VIDEO_EXTENSIONS = ["mp4", "mkv", "webm", "mov", "m4v"];

/** Native file picker for pictures, videos, shaders and 3D scenes. */
export function pickMedia(): Promise<string | null> {
  return open({
    multiple: false,
    directory: false,
    filters: [
      {
        name: "Wallpaper sources",
        extensions: [...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS, "wgsl", "glb", "gltf"],
      },
      { name: "Images", extensions: IMAGE_EXTENSIONS },
      { name: "Videos", extensions: VIDEO_EXTENSIONS },
      { name: "Shaders", extensions: ["wgsl"] },
      { name: "3D scenes", extensions: ["glb", "gltf"] },
    ],
  });
}

export function pickFolder(): Promise<string | null> {
  return open({ multiple: false, directory: true });
}

export function pickHtml(): Promise<string | null> {
  return open({ multiple: false, directory: false, filters: [{ name: "Web pages", extensions: ["html", "htm"] }] });
}

export function pickProgram(): Promise<string | null> {
  return open({ multiple: false, directory: false, filters: [{ name: "Programs", extensions: ["exe"] }] });
}

/** A program or a shortcut to pin to the dock. */
export function pickDockApp(): Promise<string | null> {
  return open({
    multiple: false,
    directory: false,
    filters: [{ name: "Programs and shortcuts", extensions: ["exe", "lnk"] }],
  });
}

export function pickStyleSheet(): Promise<string | null> {
  return open({ multiple: false, directory: false, filters: [{ name: "Style sheets", extensions: ["css"] }] });
}

/** A URL the webview can load for a local file the backend listed. */
export const fileUrl = convertFileSrc;

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
