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

export type Autostart = { enabled: boolean; command: string | null };
export type Changes = { config: boolean; themes: boolean };

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
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  startEngine: () => invoke<EngineStatus>("start_engine"),
  stopEngine: () => invoke<EngineStatus>("stop_engine"),
  getAutostart: () => invoke<Autostart>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<Autostart>("set_autostart", { enabled }),
};

/** Runs `handler` when config.json or the user themes folder changes on disk. */
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

/** A URL the webview can load for a local file the backend listed. */
export const fileUrl = convertFileSrc;

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
