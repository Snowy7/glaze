// Wire types for mypaper widgets. Mirrors docs/widgets.md and the widget SDK
// (packages/widget-sdk/src/types.ts, the widget side of the wire); keep all three in sync.
// Types only: nothing here may emit runtime code.

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

interface SettingBase<K extends string, V> {
  key: string;
  type: K;
  label: string;
  default: V;
}

export type ScalarSetting =
  | SettingBase<"boolean", boolean>
  | SettingBase<"string", string>
  | SettingBase<"color", string> // "#rrggbb"
  | (SettingBase<"select", string> & { options: readonly string[] })
  | (SettingBase<"number", number> & { min?: number; max?: number; step?: number });

type OmitEach<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** A column of a `list` setting: any scalar setting, with an optional default. */
export type ListField = OmitEach<ScalarSetting, "default"> & {
  default?: string | number | boolean;
};

/** A user-editable list of records (for example launcher shortcuts). */
export interface ListSetting extends SettingBase<"list", Record<string, string | number | boolean>[]> {
  fields: readonly ListField[];
  maxItems?: number;
}

export type SettingDef = ScalarSetting | ListSetting;

export interface WidgetManifest {
  $schema?: string;
  id: string;
  name: string;
  version?: string;
  author?: string;
  description?: string;
  entry?: string; // default "index.html"
  size: { default: Vec2; min?: Vec2; max?: Vec2 };
  resizable?: boolean; // default true
  permissions?: readonly Permission[];
  plugins?: readonly string[];
  settings?: readonly SettingDef[];
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
  | { type: "event"; instance: string; event: string; data: unknown } // the host sends event "sub"
  | { type: "reload"; widget: string };

export interface LayoutChange {
  instance: string;
  cell: Vec2;
  size: Vec2;
}

export type ShellToHost =
  | { type: "ready" }
  | { type: "hello"; instance: string }
  | { type: "call"; instance: string; id: number | string; method: string; params?: unknown }
  | { type: "layout"; changes: LayoutChange[] }
  | { type: "add"; widget: string; cell: Vec2; size: Vec2 }
  | { type: "duplicate"; instance: string; cell: Vec2 }
  | { type: "remove"; instance: string }
  | { type: "settings"; instance: string; values: Record<string, unknown> }
  | { type: "edit"; on: boolean };

// ---- iframe ⇄ shell (the SDK's wire) ----

export type MypaperErrorCode = "PermissionDenied" | "NotFound" | "InvalidParams" | "PluginUnavailable" | "Internal";

export interface SizeInfo {
  cells: [number, number];
  px: [number, number]; // CSS px
  dpi: number;
}

export type Settings = Record<string, unknown>;

/** The shell's answer to `hello`. */
export interface HelloResult {
  instance: string;
  widget: WidgetManifest;
  settings: Settings;
  size: SizeInfo;
  visible: boolean;
  editing: boolean;
}

export type SubscribeParams =
  | { sub: string; topic: "system"; intervalMs: number }
  | { sub: string; topic: "media" }
  | { sub: string; topic: "theme" }
  | { sub: string; topic: "plugin"; plugin: string; event: string };

/** Request methods the widget can send; the shell answers `hello` itself and forwards the rest. */
export type WireMethod =
  | "hello"
  | "storage.get"
  | "storage.set"
  | "system.stats"
  | "media.current"
  | "media.control"
  | "launch"
  | "theme.current"
  | "plugin"
  | "subscribe"
  | "unsubscribe";

export interface WireRequest {
  mypaper: 1;
  id: number | string; // the SDK sends numbers
  method: string; // anything the shell doesn't know is forwarded; the host answers NotFound
  params?: unknown;
}

export type WireResponse =
  | { mypaper: 1; id: number | string; result: unknown }
  | { mypaper: 1; id: number | string; error: { code: MypaperErrorCode | (string & {}); message: string } };

/** Events pushed to a widget. */
export interface WireEvents {
  visibility: boolean;
  editmode: boolean;
  settings: Settings;
  size: SizeInfo;
  sub: { sub: string; value: unknown };
}

export type WireEvent = {
  [E in keyof WireEvents]: { mypaper: 1; event: E; data: WireEvents[E] };
}[keyof WireEvents];
