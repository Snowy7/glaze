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
  /** A section heading in the settings form; shown when it changes from one setting to the next. */
  group?: string;
}

/** A `select` option: a plain value, or an object with a label and an icon glyph (an emoji or symbol). */
export type SelectOption = string | { value: string; label?: string; icon?: string };

/** The value of a select option. */
export const optionValue = (option: SelectOption): string => (typeof option === "string" ? option : option.value);

/** Settings that can also be a column of a `list`. */
export type ListableSetting =
  | SettingBase<"boolean", boolean>
  | SettingBase<"string", string>
  /** `#rrggbb`, or `#rrggbbaa` with `alpha`. */
  | (SettingBase<"color", string> & { alpha?: boolean })
  | (SettingBase<"select", string> & { options: readonly SelectOption[] })
  | (SettingBase<"number", number> & { min?: number; max?: number; step?: number })
  /** A number shown as a slider with a live preview; `min` and `max` are required. */
  | (SettingBase<"slider", number> & { min: number; max: number; step?: number; unit?: string })
  /** A CSS font-family name. */
  | SettingBase<"font", string>;

export type ScalarSetting =
  | ListableSetting
  /** `""`, or the served URL of a file the host copied into the widget's data folder. */
  | (SettingBase<"file", string> & { extensions?: readonly string[] })
  | (SettingBase<"image", string> & { extensions?: readonly string[] });

type OmitEach<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** A column of a `list` setting: any listable setting, with an optional default and no group. */
export type ListField = OmitEach<ListableSetting, "default" | "group"> & {
  default?: string | number | boolean;
};

/** A user-editable list of records (for example launcher shortcuts). */
export interface ListSetting
  extends SettingBase<"list", Record<string, string | number | boolean>[]> {
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

/**
 * A place: `null` is automatic (where this PC is), else a picked town. The form searches as you
 * type with `search`, a method of one of the widget's plugins. A plain string saved by an older
 * text setting arrives as `{ name }`.
 */
export interface PlaceSetting extends SettingBase<"place", Place | null> {
  search?: { plugin: string; method: string };
}

export type SettingDef = ScalarSetting | ListSetting | PlaceSetting;
export type SettingType = SettingDef["type"];

/** Where a widget lives: the desktop layer, an always-on-top overlay, or a bar's popup. */
export type Preset = "desktop" | "overlay" | "popup";

/** How many instances the user may add: one in total, any number, or one per monitor. */
export type InstanceRule = "single" | "multiple" | "per-monitor";

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
  /** Default `desktop`. */
  preset?: Preset;
  /** Default `multiple`. */
  instances?: InstanceRule;
  permissions?: readonly Permission[];
  plugins?: readonly string[];
  settings?: readonly SettingDef[];
  /** Ids this widget took over, with the settings their instances get. The host reads it; widgets never see it. */
  replaces?: Readonly<Record<string, Settings>>;
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
  /** Absent on machines without a battery. */
  battery?: { percent: number; charging: boolean };
  /** Seconds since boot. */
  uptime: number;
  /** The active connection; absent when the host can't tell. */
  network?: { kind: "wifi" | "ethernet" | "none"; name?: string; signal?: number };
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

export interface UserInfo {
  /** The display name ("Alex K."), else the account name. */
  name: string;
  /** The account name ("you"). */
  account: string;
}

/** The result of a file pick: where the host copied the file to, and its name. */
export interface PickedFile {
  /** Served from the widget's own origin. */
  url: string;
  name: string;
}

export interface PickFileOptions {
  /** Allowed extensions, without dots: `["jpg", "png"]`. */
  extensions?: string[];
  title?: string;
}

/** Smallest and largest number of audio bins a widget can ask for. */
export const AUDIO_BINS = { min: 64, max: 128, default: 64 } as const;

export type WidgetStyle = "minimal" | "depth" | "neon" | "paper" | "aero" | "liquid";

/**
 * How a widget should look, resolved by the host (docs/widgets.md, "Appearance"). Every field is
 * set. The SDK applies it to <html> for you; read it only for what CSS can't do.
 */
export interface Appearance {
  /** `none`: no background, the text sits on the wallpaper. */
  surface: "none" | "glass" | "solid";
  /** `#rrggbb`, the surface color. */
  tint: string;
  /** 0 to 1. */
  opacity: number;
  /** Pixels, 0 to 64 (glass only). */
  blur: number;
  /** Pixels, 0 to 48. */
  radius: number;
  border: boolean;
  /** `light` text is for dark wallpapers, `dark` for light ones. */
  text: "light" | "dark";
  /** `#rrggbb`. */
  accent: string;
  /** A CSS font-family list. */
  font: string;
  /** 0.5 to 2. */
  scale: number;
  /** A soft shadow under the text, for legibility with `surface: none`. */
  shadow: boolean;
  /** The design language; see docs/widgets.md, "Styles". Hosts older than v2 send none (`minimal`). */
  style: WidgetStyle;
}

/** The appearance plus the theme's widget CSS (null when the theme has none). */
export interface AppearanceState {
  appearance: Appearance;
  themeCss: string | null;
  /**
   * The shell has drawn this widget's blurred wallpaper lens beneath its content.
   * Without it (the overlay layer, the dev harness, older hosts) the kit draws its own rim.
   */
  plate?: boolean;
}

/** Where a liquid widget's glass goes: `[x, y, width, height]` in the page's CSS px, and its radius. */
export interface PlateShape {
  rect: [number, number, number, number];
  radius: number;
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
    /** Saves some of the widget's own settings (checked against the manifest like a form edit); `onChange` follows. */
    set(values: Settings): Promise<void>;
  };
  /** The resolved look. The drop-in already applies it to <html>; see `applyAppearance`. */
  appearance: {
    get(): Promise<AppearanceState>;
    onChange(cb: (s: AppearanceState) => void): () => void;
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
    /**
     * A namespace shared by every widget that declares `storage.shared:<namespace>` (for example the
     * todo list and the heatmap share `activity`). Give each writer its own keys: there is no
     * locking, the last write to a key wins.
     */
    shared(namespace: string): {
      get<T>(key: string): Promise<T | undefined>;
      set(key: string, value: unknown): Promise<void>;
    };
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
  /** Needs "audio". */
  audio: {
    /**
     * `bins` floats in 0..1 (64 to 128, default 64), lowest frequency first, about 30 times a
     * second while the widget is visible. The host captures only while someone listens.
     */
    spectrum(cb: (bins: number[]) => void, opts?: { bins?: number }): () => void;
  };
  /** Needs "user". */
  user: { info(): Promise<UserInfo> };
  /** The system file dialog; the file is copied into the widget's data folder. `null` if cancelled. */
  pickFile(opts?: PickFileOptions): Promise<PickedFile | null>;
  /** Needs "launch". The icon of an exe, file, folder or `shell:AppsFolder\<AUMID>`, as a PNG data URL. */
  appIcon(target: string, size?: number): Promise<string | null>;
  /**
   * Where the shell should draw this widget's liquid glass (`null`: nowhere). The drop-in
   * reports the `.card`'s box by itself while the look is liquid glass; call it only to put the
   * glass somewhere else.
   */
  plate(shape: PlateShape | null): void;
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
  /** Older hosts send neither; the SDK then keeps the default look. */
  appearance?: Appearance;
  themeCss?: string | null;
  plate?: boolean;
}

/** Subscription topics, with their `subscribe` params. */
export type SubscribeParams =
  | { sub: string; topic: "system"; intervalMs: number }
  | { sub: string; topic: "media" }
  | { sub: string; topic: "theme" }
  | { sub: string; topic: "audio"; bins?: number }
  | { sub: string; topic: "plugin"; plugin: string; event: string };

/** Everything the widget can ask the shell, as `method -> [params, result]`. */
export interface WireMethods {
  hello: [params: { sdk: number }, result: HelloResult];
  "storage.get": [{ key: string; shared?: string }, unknown];
  "storage.set": [{ key: string; value: unknown; shared?: string }, null];
  "system.stats": [undefined, SystemStats];
  "media.current": [undefined, MediaState | null];
  "media.control": [{ action: MediaAction }, null];
  launch: [{ target: string }, null];
  "theme.current": [undefined, ThemeInfo];
  plugin: [{ plugin: string; method: string; params?: unknown }, unknown];
  "user.info": [undefined, UserInfo];
  "settings.set": [{ values: Settings }, null];
  pickFile: [PickFileOptions, PickedFile | null];
  appIcon: [{ target: string; size?: number }, string | null];
  plate: [PlateShape | null, null];
  subscribe: [SubscribeParams, null];
  unsubscribe: [{ sub: string }, null];
}

/** Pushed events, as `event -> data`. */
export interface WireEvents {
  visibility: boolean;
  editmode: boolean;
  settings: Settings;
  size: SizeInfo;
  appearance: AppearanceState;
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
