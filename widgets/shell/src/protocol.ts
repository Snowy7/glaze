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

// ---- appearance (docs/widgets.md, "Appearance"; mirrors `mypaper_theme::Appearance`) ----

export type SurfaceKind = "none" | "glass" | "solid";
export type TextTone = "light" | "dark";

/** Every field of an appearance, set. `accent` is `#rrggbb`, or `auto` in an unresolved layer. */
export interface AppearanceValues {
  surface: SurfaceKind;
  tint: string; // #rrggbb
  opacity: number; // 0..1
  blur: number; // px, 0..64
  radius: number; // px, 0..48
  border: boolean;
  text: TextTone;
  accent: string; // "auto" | #rrggbb
  font: string; // a CSS font-family list
  scale: number; // 0.5..2
  shadow: boolean;
}

/** One layer: only the fields it sets. */
export type Layer = Partial<AppearanceValues>;

/** An edit to a layer: a value sets the field, `null` clears it ("Use default"). */
export type LayerPatch = { [K in keyof AppearanceValues]?: AppearanceValues[K] | null };

// ---- host ⇄ shell ----

export interface InstanceState {
  instance: string;
  widget: string;
  cell: Vec2;
  size: Vec2;
  settings: Record<string, unknown>;
  /** What the widget shows: every layer merged, `accent` a color. */
  appearance: AppearanceValues;
  /** Only what this instance sets itself (the top layer). */
  own: Layer;
}

export interface ShellState {
  type: "state";
  monitor: { device: string; dpi: number; width: number; height: number };
  work: { x: number; y: number; width: number; height: number };
  grid: { origin: Vec2; cell: Vec2; gap: number; cols: number; rows: number };
  instances: InstanceState[];
  widgets: WidgetManifest[];
  /** The layers under an instance's own: built-in defaults and the theme (`base`), the user's
   *  choice (`user`), and the color `auto` stands for (`accent`). */
  appearance: { base: AppearanceValues; user: Layer; accent: string };
  /** The theme's widget style sheet, as text. */
  themeCss: string | null;
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
  /** Edits the user's appearance layer, or an instance's own when `instance` is set. */
  | { type: "appearance"; instance?: string; values: LayerPatch }
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
  appearance: AppearanceValues;
  themeCss: string | null;
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
  appearance: { appearance: AppearanceValues; themeCss: string | null };
  sub: { sub: string; value: unknown };
}

export type WireEvent = {
  [E in keyof WireEvents]: { mypaper: 1; event: E; data: WireEvents[E] };
}[keyof WireEvents];
