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

export type Kind = "shader" | "image" | "slideshow" | "video" | "scene";
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
  active: boolean;
  /** What to write to `config.theme` to select this theme. */
  applyAs: string;
};

export type ThemeList = { themes: ThemeEntry[]; roots: string[] };

export type SourceInfo = {
  path: string;
  kind: Kind;
  usesFit: boolean;
  suggestedName: string;
};

export type CreateThemeRequest = {
  source: string;
  name: string;
  fit: Fit | null;
  apply: boolean;
};

export type EngineStatus = {
  running: boolean;
  wallpaperRunning: boolean;
  corePath: string | null;
};

export type Autostart = { enabled: boolean; command: string | null };
export type ShellInfo = { mica: boolean };
export type Changes = { config: boolean; themes: boolean };

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  updateConfig: (patch: Partial<Config>) => invoke<Config>("update_config", { patch }),
  resetConfig: () => invoke<Config>("reset_config"),
  listThemes: () => invoke<ThemeList>("list_themes"),
  inspectSource: (path: string) => invoke<SourceInfo>("inspect_source", { path }),
  createTheme: (request: CreateThemeRequest) => invoke<ThemeEntry>("create_theme", { request }),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  startEngine: () => invoke<EngineStatus>("start_engine"),
  stopEngine: () => invoke<EngineStatus>("stop_engine"),
  getAutostart: () => invoke<Autostart>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<Autostart>("set_autostart", { enabled }),
  shellInfo: () => invoke<ShellInfo>("shell_info"),
};

/** Runs `handler` when config.json or the user themes folder changes on disk. */
export function onChanged(handler: (changes: Changes) => void): Promise<UnlistenFn> {
  return listen<Changes>("studio:changed", (event) => handler(event.payload));
}

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "heic", "heif"];
const VIDEO_EXTENSIONS = ["mp4", "mkv", "webm", "mov", "m4v"];

/** Native file picker for anything that can become a theme (a folder has its own picker). */
export async function pickFile(): Promise<string | null> {
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

/** A URL the webview can load for a local file the backend listed. */
export const fileUrl = convertFileSrc;

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
