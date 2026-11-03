// Public types for the widget API. Mirrors docs/widgets.md.

// ---------------------------------------------------------------- errors

export type MypaperErrorCode =
  | "PermissionDenied"
  | "NotFound"
  | "InvalidParams"
  | "PluginUnavailable"
  | "Internal";

/** The error object on the wire. Discriminated on `code`. */
export type MypaperErrorInfo = {
  [C in MypaperErrorCode]: { code: C; message: string };
}[MypaperErrorCode];

// ---------------------------------------------------------------- manifest

export type Cells = readonly [columns: number, rows: number];

export type Permission =
  | "system.stats"
  | "media"
  | "launch"
  | "theme"
  | "clipboard.write"
  | `network:${string}`;

interface SettingBase<K extends string, V> {
  key: string;
  type: K;
  label: string;
  default: V;
}

export type ScalarSetting =
  | SettingBase<"boolean", boolean>
  | SettingBase<"string", string>
  | (SettingBase<"color", string>)
  | (SettingBase<"select", string> & { options: readonly string[] })
  | (SettingBase<"number", number> & { min?: number; max?: number; step?: number });

type OmitEach<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** A column of a `list` setting: any scalar setting, with an optional default. */
export type ListField = OmitEach<ScalarSetting, "default"> & {
  default?: string | number | boolean;
};

/** A user-editable list of records (for example launcher shortcuts). */
export interface ListSetting
  extends SettingBase<"list", Record<string, string | number | boolean>[]> {
  fields: readonly ListField[];
  maxItems?: number;
}

export type SettingDef = ScalarSetting | ListSetting;
export type SettingType = SettingDef["type"];

export interface WidgetManifest {
  $schema?: string;
  id: string;
  name: string;
  version: string;
  author?: string;
  description?: string;
  entry: string;
  size: { default: Cells; min: Cells; max: Cells };
  resizable?: boolean;
  permissions?: readonly Permission[];
  plugins?: readonly string[];
  settings?: readonly SettingDef[];
}

export interface PluginManifest {
  $schema?: string;
  id: string;
  name: string;
  version: string;
  command: string;
  args?: readonly string[];
  methods: readonly string[];
  events?: readonly string[];
  permissions?: readonly Permission[];
}

// ---------------------------------------------------------------- data

export type Settings = Record<string, unknown>;

export interface SizeInfo {
  cells: [columns: number, rows: number];
  px: [width: number, height: number];
  dpi: number;
}

export interface SystemStats {
  /** Percent, 0 to 100. */
  cpu: number;
  /** Bytes. */
  memory: { used: number; total: number };
  /** Percent, 0 to 100, when the host can read it. */
  gpu?: number;
  /** Bytes per second. */
  net?: { up: number; down: number };
  /** Bytes. */
  disks?: { name: string; used: number; total: number }[];
}

export interface MediaState {
  title: string;
  artist: string;
  album?: string;
  /** The playing app, for example "Spotify.exe". */
  app: string;
  playing: boolean;
  /** Seconds. */
  position?: number;
  /** Seconds. */
  duration?: number;
  /** A data URL. */
  artwork?: string;
}

export type MediaAction = "play" | "pause" | "toggle" | "next" | "previous";

export interface ThemeInfo {
  name: string;
  palette?: { dominant: string; accent: string };
}

// ---------------------------------------------------------------- api

/** Push events every widget receives without subscribing. */
export interface WidgetEvents {
  visibility: boolean;
  editmode: boolean;
}

export interface Mypaper {
  /** Resolves once the shell has answered the handshake. Everything else waits for it too. */
  readonly ready: Promise<void>;
  /** The instance id, for example "clock-1". Valid after `ready`. */
  readonly instance: string;
  /** The parsed manifest. Valid after `ready`. */
  readonly widget: WidgetManifest;
  /** Whether the widget is currently shown (false while the host is held). */
  readonly visible: boolean;
  /** Whether the layout is being edited. */
  readonly editing: boolean;

  settings: {
    get(): Promise<Settings>;
    onChange(cb: (s: Settings) => void): () => void;
  };
  size: {
    get(): Promise<SizeInfo>;
    onChange(cb: (s: SizeInfo) => void): () => void;
  };
  /** Pause your timers and animations when `visibility` is false. */
  on<E extends keyof WidgetEvents>(event: E, cb: (value: WidgetEvents[E]) => void): () => void;
  storage: {
    get<T>(key: string): Promise<T | undefined>;
    set(key: string, value: unknown): Promise<void>;
  };
  /** Needs "system.stats". */
  system: {
    stats(): Promise<SystemStats>;
    /** `intervalMs` is at least 1000. Paused by the host while hidden. */
    subscribe(intervalMs: number, cb: (s: SystemStats) => void): () => void;
  };
  /** Needs "media". */
  media: {
    current(): Promise<MediaState | null>;
    onChange(cb: (m: MediaState | null) => void): () => void;
    control(action: MediaAction): Promise<void>;
  };
  /** Needs "launch". A path or URL, opened with ShellExecute. */
  launch(target: string): Promise<void>;
  /** Needs "theme". */
  theme: {
    current(): Promise<ThemeInfo>;
    onChange(cb: (t: ThemeInfo) => void): () => void;
  };
  /** The plugin id must be in the manifest's "plugins". */
  plugin<T = unknown>(id: string, method: string, params?: unknown): Promise<T>;
  onPlugin(id: string, event: string, cb: (data: unknown) => void): () => void;
}

// ---------------------------------------------------------------- wire

export const WIRE_VERSION = 1;

/** The state the shell hands over in the handshake. */
export interface HelloResult {
  instance: string;
  widget: WidgetManifest;
  settings: Settings;
  size: SizeInfo;
  visible: boolean;
  editing: boolean;
}

/** Subscription topics, with their `subscribe` params. */
export type SubscribeParams =
  | { sub: string; topic: "system"; intervalMs: number }
  | { sub: string; topic: "media" }
  | { sub: string; topic: "theme" }
  | { sub: string; topic: "plugin"; plugin: string; event: string };

/** Everything the widget can ask the shell, as `method -> [params, result]`. */
export interface WireMethods {
  hello: [params: { sdk: number }, result: HelloResult];
  "storage.get": [{ key: string }, unknown];
  "storage.set": [{ key: string; value: unknown }, null];
  "system.stats": [undefined, SystemStats];
  "media.current": [undefined, MediaState | null];
  "media.control": [{ action: MediaAction }, null];
  launch: [{ target: string }, null];
  "theme.current": [undefined, ThemeInfo];
  plugin: [{ plugin: string; method: string; params?: unknown }, unknown];
  subscribe: [SubscribeParams, null];
  unsubscribe: [{ sub: string }, null];
}

/** Pushed events, as `event -> data`. */
export interface WireEvents {
  visibility: boolean;
  editmode: boolean;
  settings: Settings;
  size: SizeInfo;
  /** `value` is a SystemStats, MediaState | null, ThemeInfo or plugin event data, by subscription. */
  sub: { sub: string; value: unknown };
}

export interface WireRequest {
  mypaper: typeof WIRE_VERSION;
  id: number;
  method: keyof WireMethods;
  params?: unknown;
}

export type WireResponse =
  | { mypaper: typeof WIRE_VERSION; id: number; result: unknown }
  | { mypaper: typeof WIRE_VERSION; id: number; error: MypaperErrorInfo };

export type WireEvent = {
  [E in keyof WireEvents]: { mypaper: typeof WIRE_VERSION; event: E; data: WireEvents[E] };
}[keyof WireEvents];

export type WireMessage = WireRequest | WireResponse | WireEvent;
