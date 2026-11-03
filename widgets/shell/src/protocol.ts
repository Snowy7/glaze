// Wire types for mypaper widgets. Mirrors docs/widgets.md (the contract); keep both in sync.
// Types only: nothing here may emit runtime code (bridge.ts imports it into a classic script).

/** `[columns, rows]` for sizes, `[column, row]` for cells. */
export type Vec2 = [number, number];

// ---- widget.json ----

export type Permission =
  | "system.stats"
  | "media"
  | "launch"
  | "theme"
  | "clipboard.write"
  | `network:${string}`;

interface SettingBase {
  key: string;
  label?: string;
}

export type SettingSpec =
  | (SettingBase & { type: "boolean"; default: boolean })
  | (SettingBase & { type: "number"; default: number; min?: number; max?: number; step?: number })
  | (SettingBase & { type: "string"; default: string })
  | (SettingBase & { type: "select"; options: string[]; default: string })
  | (SettingBase & { type: "color"; default: string }); // "#rrggbb"

export interface WidgetManifest {
  id: string;
  name: string;
  version?: string;
  author?: string;
  description?: string;
  entry?: string; // default "index.html"
  size: { default: Vec2; min?: Vec2; max?: Vec2 };
  resizable?: boolean; // default true
  permissions?: Permission[];
  plugins?: string[];
  settings?: SettingSpec[];
}

// ---- host ⇄ shell ----

export interface InstanceState {
  instance: string;
  widget: string;
  cell: Vec2;
  size: Vec2;
  settings: Record<string, unknown>;
}

export interface ShellState {
  type: "state";
  monitor: { device: string; dpi: number; width: number; height: number };
  work: { x: number; y: number; width: number; height: number };
  grid: { origin: Vec2; cell: Vec2; gap: number; cols: number; rows: number };
  instances: InstanceState[];
  widgets: WidgetManifest[];
  editing: boolean;
  visible: boolean;
  error?: string;
}

export interface WireError {
  code: string;
  message: string;
}

export type HostToShell =
  | ShellState
  | { type: "reply"; instance: string; id: number | string; result?: unknown; error?: WireError }
  | { type: "event"; instance: string; event: string; data: unknown }
  | { type: "reload"; widget: string };

export interface LayoutChange {
  instance: string;
  cell: Vec2;
  size: Vec2;
}

export type ShellToHost =
  | { type: "ready" }
  | { type: "call"; instance: string; id: number | string; method: string; params?: unknown }
  | { type: "layout"; changes: LayoutChange[] }
  | { type: "add"; widget: string; cell: Vec2; size: Vec2 }
  | { type: "duplicate"; instance: string; cell: Vec2 }
  | { type: "remove"; instance: string }
  | { type: "settings"; instance: string; values: Record<string, unknown> }
  | { type: "edit"; on: boolean };

// ---- iframe ⇄ shell ----

export type ErrorCode = "PermissionDenied" | "NotFound" | "InvalidParams" | "PluginUnavailable" | "Internal";

export interface WidgetRequest {
  mypaper: 1;
  id: number | string;
  method: string;
  params?: unknown;
}

export type WidgetReply =
  | { mypaper: 1; id: number | string; result: unknown }
  | { mypaper: 1; id: number | string; error: WireError };

export interface WidgetEvent {
  mypaper: 1;
  event: string;
  data: unknown;
}

// ---- the `mypaper` global ----

export interface SizeInfo {
  cells: Vec2;
  px: Vec2;
  dpi: number;
}

export interface SystemStats {
  cpu: number;
  memory: { used: number; total: number };
  gpu?: number;
  net?: { up: number; down: number };
  disks?: { name: string; used: number; total: number }[];
}

export interface MediaState {
  title: string;
  artist: string;
  album?: string;
  app: string;
  playing: boolean;
  position?: number;
  duration?: number;
  artwork?: string; // data URL
}

export interface ThemeInfo {
  name: string;
  palette?: { dominant: string; accent: string };
}

export interface Mypaper {
  readonly instance: string;
  readonly widget: WidgetManifest;
  settings: {
    get(): Promise<Record<string, unknown>>;
    onChange(cb: (s: Record<string, unknown>) => void): () => void;
  };
  size: {
    get(): Promise<SizeInfo>;
    onChange(cb: (s: SizeInfo) => void): () => void;
  };
  on(event: "visibility", cb: (visible: boolean) => void): () => void;
  on(event: "editmode", cb: (editing: boolean) => void): () => void;
  storage: {
    get<T>(key: string): Promise<T | undefined>;
    set(key: string, value: unknown): Promise<void>;
  };
  system: {
    stats(): Promise<SystemStats>;
    subscribe(intervalMs: number, cb: (s: SystemStats) => void): () => void;
  };
  media: {
    current(): Promise<MediaState | null>;
    onChange(cb: (m: MediaState | null) => void): () => void;
    control(action: "play" | "pause" | "toggle" | "next" | "previous"): Promise<void>;
  };
  launch(target: string): Promise<void>;
  theme: {
    current(): Promise<ThemeInfo>;
    onChange(cb: (t: ThemeInfo) => void): () => void;
  };
  plugin<T = unknown>(id: string, method: string, params?: unknown): Promise<T>;
  onPlugin(id: string, event: string, cb: (data: unknown) => void): () => void;
}
