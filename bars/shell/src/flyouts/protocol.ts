// The flyouts' half of the page ⇄ host protocol (docs/bars.md, "Flyouts"; the host's side is
// crates/bars/src/flyouts). protocol.ts merges these into MethodMap and EventMap.

/** An exe, a `.lnk`, a folder, or `shell:AppsFolder\<AUMID>` (the same shape as the dock's pins). */
export interface Pin {
  path: string;
  name?: string;
}

export const LAUNCHER_LAYOUTS = ["win11", "grid", "compact", "dashboard"] as const;
export type LauncherLayout = (typeof LAUNCHER_LAYOUTS)[number];

/** `bars.json` → `launcher`. */
export interface LauncherConfig {
  /** A lone Windows key opens the launcher (only while the native taskbar is hidden). */
  winKey: boolean;
  layout: LauncherLayout;
  /** Missing: the dock's pins. */
  pinned?: Pin[];
  /** Search recent files too. */
  files: boolean;
}

/** `bars.json` → `calendar`. */
export interface CalendarConfig {
  zones: { zone: string; label?: string }[];
  firstDay?: "monday" | "sunday" | "saturday";
}

/** One app in the launcher's index. */
export interface LauncherApp {
  id: string;
  name: string;
  /** What `launcherRun` opens: `shell:AppsFolder\<id>`. */
  path: string;
  icon?: string;
  /** More words a search matches (a shortcut's target exe). */
  keywords?: string[];
  packaged: boolean;
}

export interface Recent {
  id: string;
  path: string;
  count: number;
  /** ms since the epoch. */
  last: number;
}

export interface RecentFile {
  name: string;
  path: string;
  at: number;
  icon?: string;
}

export interface AppsResult {
  apps: LauncherApp[];
  recent: Recent[];
  /** Who's signed in, for the launcher's footer. */
  user: { name: string };
}

export type RadioKind = "wifi" | "bluetooth" | "cellular";
export type PowerMode = "efficiency" | "balanced" | "performance";

export interface QuickState {
  radios: { kind: RadioKind; name: string; on: boolean }[];
  /** 0–100; missing without a display that has a control. */
  brightness?: number;
  power?: PowerMode;
  /** Battery saver, on machines with a battery. */
  saver?: boolean;
  dnd?: "off" | "priority" | "alarms";
  focus?: boolean;
}

export interface WifiNetwork {
  ssid: string;
  signal: number;
  security: "open" | "wpa2" | "wpa3" | "wpa" | "enterprise" | "other";
  connected: boolean;
  /** A saved profile exists: connecting needs no password. */
  known: boolean;
}

export interface WifiState {
  networks: WifiNetwork[];
  /** `location`: Windows 11 24H2+ needs location access for desktop apps to see networks. */
  error?: "noAdapter" | "location" | "unavailable";
}

export interface AudioDevice {
  id: string;
  name: string;
  default: boolean;
}

export interface AudioSession {
  id: string;
  name: string;
  icon?: string;
  level: number;
  muted: boolean;
  system: boolean;
  active: boolean;
}

export interface MixerState {
  devices: AudioDevice[];
  sessions: AudioSession[];
  /** `audio.switchOutput` is on: devices can be picked here. */
  switchable: boolean;
}

export interface KeyboardLayout {
  /** The HKL in hex: what `setLayout` takes. */
  id: string;
  language: string;
  layout: string;
  short: string;
  region: string;
}

export interface Languages {
  current?: string;
  layouts: KeyboardLayout[];
}

export interface NotificationItem {
  id: number;
  app: string;
  icon?: string;
  title: string;
  body: string;
  at: number;
}

export interface NotificationList {
  notifications: NotificationItem[];
  error?: "denied" | "unavailable";
}

export interface ClipboardEntry {
  kind: "text" | "link" | "image" | "other";
  text: string;
  image?: string;
}

export interface ClipboardHistory {
  enabled: boolean;
  entries: ClipboardEntry[];
}

/** What a flyout can watch: its own live data, or a core topic. */
export type WatchTopic =
  | "mixer"
  | "wifi"
  | "launcherApps"
  | "notificationList"
  | "media"
  | "volume"
  | "network"
  | "battery"
  | "stats"
  | "clock"
  | "language"
  | "notifications";

export interface FlyoutMethods {
  watch: { params: { topic: WatchTopic }; result: null };
  unwatch: { params: { topic: WatchTopic }; result: null };
  /** The window must reach `extent` DIPs past the bar's band; 0 goes back to the default room. */
  room: { params: { extent: number }; result: null };
  /** Take the keyboard (the bar becomes the foreground window) or let it go. `handBack`: no
   * press closed the flyout, so the app that was in front gets the foreground back. */
  keyboard: { params: { on: boolean; handBack?: boolean }; result: null };
  apps: { params: undefined; result: AppsResult };
  /** Opens an app, settings page or file from the launcher and remembers it. */
  launcherRun: { params: { id: string; path: string }; result: { recent: Recent[] } };
  recentFiles: { params: undefined; result: RecentFile[] };
  clipboard: { params: undefined; result: ClipboardHistory };
  clipboardPaste: { params: { index: number }; result: null };
  quick: { params: undefined; result: QuickState };
  setRadio: { params: { kind: RadioKind; on: boolean }; result: QuickState };
  setBrightness: { params: { level: number }; result: null };
  setPowerMode: { params: { mode: PowerMode }; result: QuickState };
  wifi: { params: { scan?: boolean }; result: WifiState };
  wifiConnect: { params: { ssid: string; password?: string }; result: null };
  wifiDisconnect: { params: undefined; result: null };
  mixer: { params: undefined; result: MixerState };
  setSession: { params: { id: string; level?: number; mute?: boolean }; result: null };
  setOutput: { params: { id: string }; result: MixerState };
  setLayout: { params: { id: string }; result: null };
  launcherPin: { params: { path: string; name?: string; index?: number }; result: null };
  launcherUnpin: { params: { path: string }; result: null };
  notifications: { params: undefined; result: NotificationList };
  dismissNotification: { params: { id: number }; result: NotificationList };
  clearNotifications: { params: undefined; result: NotificationList };
}

export interface FlyoutEvents {
  mixer: MixerState;
  wifi: WifiState;
  launcherApps: AppsResult;
  notificationList: NotificationList;
  /** The `language` module's topic. */
  language: Languages;
  /** The `notifications` module's topic; `count` is null when they can't be read. */
  notifications: { count: number | null };
  launcherPins: Pin[];
  /** The Windows key: open the launcher (transient). */
  open: { flyout: "launcher" };
}
