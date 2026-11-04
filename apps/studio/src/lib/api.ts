// The one place that talks to the Rust side. Mirrors src-tauri/src/commands.rs:
// change a command there, change its wrapper here. Errors arrive as strings.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import type { BarsDoc, FillType, Json, PresetId } from "./barsSchema";

/** The built-in rows of the desktop right-click menu (docs/desktop-extras.md). */
export const MENU_BUILTINS = ["separator", "view", "sort", "refresh", "new", "paste", "wallpaper", "widgets", "display", "personalize", "terminal", "more"] as const;
export type MenuBuiltin = (typeof MENU_BUILTINS)[number];
export type MenuCustom = { label: string; command: string; args: string[] };
export type MenuEntry = MenuBuiltin | MenuCustom;

/** `%APPDATA%\mypaper\config.json` as the engine reads it: every setting, defaults filled in. */
export type Config = {
  theme: string;
  paused: boolean;
  pause_on_battery: boolean;
  pause_on_fullscreen: boolean;
  /** Also set a still of the theme as the Windows wallpaper (see docs/performance.md). */
  static_snapshot: boolean;
  /** Windows 11: the full classic context menu everywhere (needs an Explorer restart). */
  classic_context_menu: boolean;
  desktop_menu: { enabled: boolean; items: MenuEntry[] };
  /** The menu's items when `items` is left out. */
  desktop_menu_default: MenuEntry[];
};

/** What `update_config` takes: only the keys to change. */
export type ConfigPatch = Partial<Omit<Config, "desktop_menu" | "desktop_menu_default">> & {
  desktop_menu?: { enabled?: boolean; items?: MenuEntry[] };
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
  /** `mypaper-dark`, `mypaper-light`, a set in `%APPDATA%\mypaper\cursors`, or a folder of .cur / .ani files. */
  cursors: string | null;
};

export type EngineStatus = {
  running: boolean;
  wallpaperRunning: boolean;
  corePath: string | null;
};

export type ComponentId = "core" | "wallpaper" | "widgets" | "bars";

/** One mypaper program: is it running, and where Studio found it. */
export type Component = { id: ComponentId; exe: string; running: boolean; path: string | null };

/** `mypaper-core --autostart status`: `task` is a Task Scheduler logon task, `run-key` the Run registry value. */
export type Autostart = { enabled: boolean; method: "task" | "run-key" | null; command: string | null };
export type Changes = { config: boolean; themes: boolean; bars: boolean; widgets: boolean };

// --- Appearance (docs/widgets.md, "Appearance") ---

export type Surface = "none" | "glass" | "solid";
export type TextTone = "light" | "dark";

/** The widget design language (docs/widgets.md, "Styles"). Bars ignore it. */
export const WIDGET_STYLES = ["minimal", "depth", "neon", "paper", "aero", "liquid"] as const;
export type WidgetStyle = (typeof WIDGET_STYLES)[number];

/** One layer of how widgets or bars look. An absent field means "use the layer below". */
export type Appearance = {
  style?: WidgetStyle;
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

/** Liquid glass, each 0 to 1 (for `style: "liquid"`): the whole effect, how much the rim bends, the rim's shine. */
export type Glass = { intensity?: number; refraction?: number; highlight?: number };

/** A theme's `widgets` / `bars` section. */
export type SkinEdit = { appearance: Appearance; css: string | null; glass: Glass | null };

export type ActiveSkins = { widgets: Appearance; bars: Appearance };

/** A rectangle in physical pixels. */
export type PixelRect = { x: number; y: number; width: number; height: number };

export type MonitorInfo = {
  device: string;
  primary: boolean;
  /** Physical pixels. */
  width: number;
  height: number;
  /** DPI scale: a DIP is `scale` pixels. */
  scale: number;
  /** Position on the virtual screen. */
  x: number;
  y: number;
  /** Without the taskbar and app bars, relative to the monitor. */
  work: PixelRect;
};

// --- Bars (docs/bars.md) ---

/** `bars.json` as the page edits it: the document itself. The Rust side checks it with the host's schema. */
export type BarsState = {
  /** The file as written (`{ "version": 1 }` when there is none). */
  doc: BarsDoc;
  /** False: no file yet; the first change creates it. */
  exists: boolean;
  path: string;
  /** Why the bars host would refuse the file, when it would. */
  problem: string | null;
};

export type Geometry = { margin: number; padding: number; spacing: number };

/** The host's own defaults, presets and geometry: what a document is merged against. */
export type BarsCatalog = {
  defaults: BarsDoc;
  presets: { id: PresetId; bundle: BarsDoc }[];
  geometry: {
    top: Record<"strip" | "floating" | "islands", Geometry>;
    dock: Record<"floating" | "taskbar", Geometry & { size: number }>;
  };
  namedCurves: string[];
  colorTokens: string[];
  /** For each preset, the `look.background` types a file can choose over it (the host merges them key by key). */
  backgroundTypes: Record<PresetId, FillType[]>;
};

export type VarType = "color" | "number" | "length" | "select" | "bool";

/** One variable a theme's bars CSS declares (docs/bars.md, "Theme variables"). */
export type ThemeVar = {
  name: string;
  type: VarType;
  label: string;
  group?: string;
  description?: string;
  default: Json;
  min?: number;
  max?: number;
  step?: number;
  unit: "px" | "em" | "rem" | "%" | "ms" | "deg";
  options?: string[];
};

export type ThemeVars = { theme: string; css: string | null; vars: ThemeVar[]; error: string | null };

export type IconPackInfo = {
  /** The folder name: what `bars.json` → `icons` takes. */
  name: string;
  title: string;
  /** Ships with the bars page. */
  bundled: boolean;
  mono: boolean;
  entries: number;
  apps: number;
  /** Absolute paths of a few of its icons. */
  previews: string[];
  error: string | null;
};

export type AppIcon = { index: number; kind: "path" | "umid"; target: string; file: string };

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
  /** Where it lives: below windows, always on top, or opened from a bar module. */
  preset: "desktop" | "overlay" | "popup";
  origin: "bundled" | "user";
  dir: string;
  /** The page to load, relative to `dir`. */
  entry: string;
  /** The manifest as written (null when it doesn't load): the preview's handshake and the settings form. */
  manifest: WidgetManifest | null;
  /** The manifest doesn't load. */
  error: string | null;
};

/** A `select` option: a plain value, or one with a label and an icon glyph. */
export type SelectOption = string | { value: string; label?: string; icon?: string };

type SettingBase<T extends string, V> = { key: string; type: T; label: string; group?: string; default?: V };

/** One of a widget's own settings (packages/widget-sdk/src/types.ts, `SettingDef`). */
export type WidgetSetting =
  | SettingBase<"boolean", boolean>
  | SettingBase<"string", string>
  | SettingBase<"font", string>
  | (SettingBase<"color", string> & { alpha?: boolean })
  | (SettingBase<"select", string> & { options: readonly SelectOption[] })
  | (SettingBase<"number", number> & { min?: number; max?: number; step?: number })
  | (SettingBase<"slider", number> & { min: number; max: number; step?: number; unit?: string })
  | (SettingBase<"file", string> & { extensions?: readonly string[] })
  | (SettingBase<"image", string> & { extensions?: readonly string[] })
  | (SettingBase<"list", Record<string, string | number | boolean>[]> & { fields: readonly unknown[]; maxItems?: number })
  | (SettingBase<"place", Place | null> & { search?: { plugin: string; method: string } });

/** A `place` value other than automatic: a picked town. */
export type Place = { name: string; lat?: number; lon?: number; region?: string; country?: string };

/** A town suggestion, as a `place` setting's `search` method answers `{ query }`. */
export type PlaceCandidate = { name: string; admin1?: string | null; admin2?: string | null; country?: string | null; country_code?: string | null; lat: number; lon: number; population?: number | null };

/** The parts of `widget.json` Studio reads; the rest is passed to the widget as is. */
export type WidgetManifest = { id: string; name: string; settings?: readonly WidgetSetting[] } & { [key: string]: Json | readonly WidgetSetting[] | undefined };

export type PlacedWidget = {
  instance: string;
  widget: string;
  monitor: string | null;
  cell: [number, number] | null;
  size: [number, number] | null;
  appearance: Appearance;
  /** The widget's own options; unset keys take the manifest's defaults. */
  settings: Record<string, Json>;
};

/** `widgets.json` → `grid`, in DIPs; `cell` 0 is the desktop icon spacing. */
export type Grid = { cell: number; gap: number; margin: number };

/** What `put_instance` takes: only the fields to change. A new `instance` id adds the widget. */
export type InstancePatch = {
  instance: string;
  widget?: string;
  /** Null: the primary monitor. */
  monitor?: string | null;
  cell?: [number, number];
  size?: [number, number];
  settings?: Record<string, Json>;
  appearance?: Appearance;
};

export type Layout = {
  /** The user layer: `widgets.json` → `appearance`. */
  appearance: Appearance;
  instances: PlacedWidget[];
  grid: Grid;
  /** The desktop icon spacing in DIPs, what `grid.cell` 0 stands for. */
  iconCell: [number, number];
  /** Whether the topmost overlay widgets are shown (`widgets.json` → `overlay`; on unless turned off). */
  overlay: boolean;
  exists: boolean;
};

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  updateConfig: (patch: ConfigPatch) => invoke<Config>("update_config", { patch }),
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
  getInteractive: (dir: string) => invoke<boolean | null>("get_interactive", { dir }),
  setInteractive: (dir: string, on: boolean) => invoke<boolean | null>("set_interactive", { dir, on }),
  listCursorSets: () => invoke<string[]>("list_cursor_sets"),
  getBars: () => invoke<BarsState>("get_bars"),
  barsCatalog: () => invoke<BarsCatalog>("bars_catalog"),
  setBars: (doc: BarsDoc) => invoke<BarsState>("set_bars", { doc }),
  resetBars: () => invoke<BarsState>("reset_bars"),
  themeVars: () => invoke<ThemeVars>("theme_vars"),
  listIconPacks: () => invoke<IconPackInfo[]>("list_icon_packs"),
  iconPackApps: (pack: string) => invoke<AppIcon[]>("icon_pack_apps", { pack }),
  createIconPack: (name: string) => invoke<void>("create_icon_pack", { name }),
  setAppIcon: (pack: string, target: string, image: string) => invoke<void>("set_app_icon", { pack, target, image }),
  removeAppIcon: (pack: string, index: number) => invoke<void>("remove_app_icon", { pack, index }),
  listMonitors: () => invoke<MonitorInfo[]>("list_monitors"),
  /** The absolute path of a PNG icon for a dock item; load it with `fileUrl`. */
  dockIcon: (path: string) => invoke<string>("dock_icon", { path }),
  listWidgets: () => invoke<WidgetInfo[]>("list_widgets"),
  getLayout: () => invoke<Layout>("get_layout"),
  setWidgetsAppearance: (appearance: Appearance) => invoke<Layout>("set_widgets_appearance", { appearance }),
  setWidgetsOverlay: (on: boolean) => invoke<Layout>("set_widgets_overlay", { on }),
  removeInstance: (instance: string) => invoke<Layout>("remove_instance", { instance }),
  resetInstanceLook: (instance: string) => invoke<Layout>("reset_instance_look", { instance }),
  putInstance: (patch: InstancePatch) => invoke<Layout>("put_instance", { patch }),
  /** One of a widget's plugin methods, run for it (the widget must list the plugin). */
  pluginCall: (widget: string, plugin: string, method: string, params: unknown) => invoke<unknown>("plugin_call", { widget, plugin, method, params }),
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

/** A picture for an icon, a background or a module: the types the bars page can draw. */
export function pickImage(): Promise<string | null> {
  return open({
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions: ["svg", "png", "ico", "webp", "jpg", "jpeg"] }],
  });
}

/** Any file, for the program or document a click should run. */
export function pickAnyFile(): Promise<string | null> {
  return open({ multiple: false, directory: false });
}

export function pickStyleSheet(): Promise<string | null> {
  return open({ multiple: false, directory: false, filters: [{ name: "Style sheets", extensions: ["css"] }] });
}

/** A URL the webview can load for a local file the backend listed. */
export const fileUrl = convertFileSrc;

/**
 * A URL for a file inside `dir` whose relative links keep working (`fileUrl` encodes the whole
 * path as one segment). For pages Studio runs from disk, such as widgets.
 */
export function folderUrl(dir: string, file: string): string {
  return fileUrl("") + [...dir.split(/[\\/]/), ...file.split("/")].filter((part) => part !== "").map(encodeURIComponent).join("/");
}

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
