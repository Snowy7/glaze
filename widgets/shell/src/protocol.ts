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
  | "audio"
  | "user"
  | "clipboard.write"
  | `storage.shared:${string}`
  | `network:${string}`;

interface SettingBase<K extends string, V> {
  key: string;
  type: K;
  label: string;
  default: V;
  /** A section heading in the form. */
  group?: string;
}

/** A `select` option: a plain value, or an object with a label and an icon glyph. */
export type SelectOption = string | { value: string; label?: string; icon?: string };

/** Settings that can also be a column of a `list`. */
export type ListableSetting =
  | SettingBase<"boolean", boolean>
  | SettingBase<"string", string>
  | (SettingBase<"color", string> & { alpha?: boolean }) // "#rrggbb", or "#rrggbbaa" with `alpha`
  | (SettingBase<"select", string> & { options: readonly SelectOption[] })
  | (SettingBase<"number", number> & { min?: number; max?: number; step?: number })
  | (SettingBase<"slider", number> & { min: number; max: number; step?: number; unit?: string })
  | SettingBase<"font", string>;

export type ScalarSetting =
  | ListableSetting
  // "" or the served URL of a file the host copied into the widget's data folder.
  | (SettingBase<"file", string> & { extensions?: readonly string[] })
  | (SettingBase<"image", string> & { extensions?: readonly string[] });

type OmitEach<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** A column of a `list` setting: any listable setting, with an optional default and no group. */
export type ListField = OmitEach<ListableSetting, "default" | "group"> & {
  default?: string | number | boolean;
};

/** A user-editable list of records (for example launcher shortcuts). */
export interface ListSetting extends SettingBase<"list", Record<string, string | number | boolean>[]> {
  fields: readonly ListField[];
  maxItems?: number;
}

/** A `place` value other than automatic: a name, and its coordinates when known. */
export interface Place {
  name: string;
  lat?: number;
  lon?: number;
  /** For showing: the state or province (a candidate's `admin1`). */
  region?: string;
  country?: string;
}

/** A town suggestion, as a `place` setting's `search` method answers `{ query }`. */
export interface PlaceCandidate {
  name: string;
  admin1?: string | null;
  admin2?: string | null;
  country?: string | null;
  country_code?: string | null;
  lat: number;
  lon: number;
  population?: number | null;
}

/** A place: null is automatic, else a picked town; the form searches with `search`. */
export interface PlaceSetting extends SettingBase<"place", Place | null> {
  search?: { plugin: string; method: string };
}

export type SettingDef = ScalarSetting | ListSetting | PlaceSetting;

/** Where a widget lives: the desktop layer, an always-on-top overlay, or a bar's popup. */
export type Preset = "desktop" | "overlay" | "popup";

/** How many instances the user may add: one in total, any number, or one per monitor. */
export type InstanceRule = "single" | "multiple" | "per-monitor";

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
  preset?: Preset; // default "desktop"
  instances?: InstanceRule; // default "multiple"
  permissions?: readonly Permission[];
  plugins?: readonly string[];
  settings?: readonly SettingDef[];
}

// ---- appearance (docs/widgets.md, "Appearance"; mirrors `mypaper_theme::Appearance`) ----

export type SurfaceKind = "none" | "glass" | "solid";
export type TextTone = "light" | "dark";
export type WidgetStyle = "minimal" | "depth" | "neon" | "paper" | "aero" | "liquid";

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
  style: WidgetStyle; // the design language
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
  /** Which window this page lives in; `instances` holds only what that layer shows. Absent from
   *  older hosts: `desktop`. */
  layer?: "desktop" | "overlay";
  /** Widget id -> instances on ALL monitors (the palette's `instances` rule). Absent from older hosts. */
  counts?: Record<string, number>;
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

/** Where a liquid widget's glass goes on its page: `[x, y, width, height]` in CSS px, and its radius. */
export interface PlateShape {
  rect: [number, number, number, number];
  radius: number;
}

/** A liquid widget's card on the monitor (DIPs from its top-left), for the wallpaper's glass. */
export interface PlateCard {
  instance: string;
  rect: [number, number, number, number];
  radius: number;
}

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
  /** A `file`/`image` setting: the host shows the dialog, copies the file, sets the setting. */
  | { type: "pickFile"; instance: string; key: string; extensions?: readonly string[] }
  /** Edits the user's appearance layer, or an instance's own when `instance` is set. */
  | { type: "appearance"; instance?: string; values: LayerPatch }
  | { type: "edit"; on: boolean }
  /** The liquid widgets' cards as they are on screen now: whenever they change, at most once a frame. */
  | { type: "plates"; plates: PlateCard[] };

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
  /** The wallpaper draws liquid glass here (the desktop layer); the kit draws its own rim otherwise. */
  plate: boolean;
}

export type SubscribeParams =
  | { sub: string; topic: "system"; intervalMs: number }
  | { sub: string; topic: "media" }
  | { sub: string; topic: "theme" }
  | { sub: string; topic: "audio"; bins?: number }
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
  | "user.info"
  | "pickFile"
  | "appIcon"
  | "plate"
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
  appearance: { appearance: AppearanceValues; themeCss: string | null; plate: boolean };
  sub: { sub: string; value: unknown };
}

export type WireEvent = {
  [E in keyof WireEvents]: { mypaper: 1; event: E; data: WireEvents[E] };
}[keyof WireEvents];
