// The dev host's flyout answers (mock.ts asks here first), so every flyout runs in a plain browser
// with realistic data: `pnpm dev`, then open them from the top bar.

import type { EventMap, EventName, Request } from "../protocol";
import type {
  AppsResult,
  ClipboardHistory,
  Languages,
  MixerState,
  NotificationList,
  QuickState,
  RecentFile,
  WifiState,
} from "./protocol";

const tile = (hue: number, letter: string): string =>
  `data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect x="4" y="4" width="56" height="56" rx="14" fill="hsl(${hue} 8% ${38 + (hue % 20)}%)"/><text x="32" y="42" font-family="Segoe UI" font-size="28" font-weight="600" fill="#fff" text-anchor="middle">${letter}</text></svg>`,
  )}`;

const NAMES = [
  "Calculator", "Calendar", "Camera", "Clock", "Code", "Discord", "Excel", "File Explorer", "Firefox", "GIMP",
  "Mail", "Maps", "Media Player", "Microsoft Edge", "Notepad", "OBS Studio", "Paint", "Photos", "PowerShell",
  "Settings", "Slack", "Snipping Tool", "Spotify", "Steam", "Terminal", "Visual Studio Code", "VLC", "Word", "Xbox", "Zoom",
];

const apps: AppsResult = {
  apps: NAMES.map((name, i) => ({
    id: name.replace(/\s/g, ""),
    name,
    path: `shell:AppsFolder\\${name.replace(/\s/g, "")}`,
    icon: tile(i * 37, name.charAt(0)),
    packaged: i % 3 === 0,
    ...(name === "Visual Studio Code" ? { keywords: ["code"] } : {}),
  })),
  recent: [
    { id: "Terminal", path: "shell:AppsFolder\\Terminal", count: 9, last: Date.now() - 60_000 },
    { id: "Firefox", path: "shell:AppsFolder\\Firefox", count: 4, last: Date.now() - 3_600_000 },
  ],
  user: { name: "Alex" },
};

const files: RecentFile[] = ["Quarterly report", "bars.md", "wallpaper-sketch.png", "Invoice 0142", "notes.txt", "flyouts.ts"].map((name, i) => ({
  name,
  path: `C:\\Recent\\${name}.lnk`,
  at: Date.now() - i * 5_400_000,
}));

let quick: QuickState = {
  radios: [
    { kind: "wifi", name: "Wi-Fi", on: true },
    { kind: "bluetooth", name: "Bluetooth", on: false },
  ],
  brightness: 70,
  power: "balanced",
  saver: false,
  dnd: "off",
  focus: false,
};

const wifi: WifiState = {
  networks: [
    { ssid: "Home-5G", signal: 82, security: "wpa2", connected: true, known: true },
    { ssid: "Home-2.4", signal: 64, security: "wpa2", connected: false, known: true },
    { ssid: "Cafe Guest", signal: 41, security: "open", connected: false, known: false },
    { ssid: "Neighbour", signal: 22, security: "wpa3", connected: false, known: false },
  ],
};

let mixer: MixerState = {
  devices: [
    { id: "speakers", name: "Speakers (USB Audio)", default: true },
    { id: "headset", name: "Headset (Bluetooth)", default: false },
  ],
  sessions: [
    { id: "sys", name: "System sounds", level: 0.8, muted: false, system: true, active: false },
    { id: "spotify", name: "Spotify", icon: tile(140, "S"), level: 0.55, muted: false, system: false, active: true },
    { id: "firefox", name: "Firefox", icon: tile(20, "F"), level: 1, muted: true, system: false, active: false },
  ],
  switchable: false,
};

const languages: Languages = {
  current: "04090409",
  layouts: [
    { id: "04090409", language: "English (United States)", layout: "US", short: "ENG", region: "US" },
    { id: "04070407", language: "German (Germany)", layout: "German", short: "DEU", region: "DE" },
    { id: "e0010411", language: "Japanese (Japan)", layout: "Microsoft IME", short: "JPN", region: "JP" },
  ],
};

let notifications: NotificationList = {
  notifications: [
    { id: 1, app: "Discord", icon: tile(240, "D"), title: "Maya", body: "are we still on for lunch?", at: Date.now() - 4 * 60_000 },
    { id: 2, app: "Discord", icon: tile(240, "D"), title: "#design", body: "Sam shared a new mockup", at: Date.now() - 40 * 60_000 },
    { id: 3, app: "Windows Security", title: "Scan finished", body: "No current threats.", at: Date.now() - 5 * 3_600_000 },
  ],
};

const clipboard: ClipboardHistory = {
  enabled: true,
  entries: [
    { kind: "text", text: "cargo nextest run -p mypaper-bars" },
    { kind: "link", text: "https://learn.microsoft.com/windows/apps/develop/notifications" },
    { kind: "text", text: "Shell_NotifyIcon sends WM_COPYDATA to the first Shell_TrayWnd it finds." },
  ],
};

type Emit = <E extends EventName>(event: E, data: EventMap[E]) => void;

/** The answer to a flyout request, or `undefined` when it isn't one. */
export function flyoutMock(message: Request, emit: Emit): unknown {
  switch (message.method) {
    case "watch":
    case "unwatch":
    case "room":
    case "keyboard":
    case "setBrightness":
    case "wifiDisconnect":
    case "launcherPin":
    case "launcherUnpin":
    case "clipboardPaste":
      return null;
    case "apps":
      return apps;
    case "run":
      return { recent: apps.recent };
    case "recentFiles":
      return files;
    case "clipboard":
      return clipboard;
    case "quick":
      return quick;
    case "setRadio":
      quick = { ...quick, radios: quick.radios.map((r) => (r.kind === message.params.kind ? { ...r, on: message.params.on } : r)) };
      return quick;
    case "setPowerMode":
      quick = { ...quick, power: message.params.mode };
      return quick;
    case "wifi":
      return wifi;
    case "wifiConnect":
      return null;
    case "mixer":
      return mixer;
    case "setSession": {
      const { id, level, mute } = message.params;
      mixer = {
        ...mixer,
        sessions: mixer.sessions.map((s) => (s.id === id ? { ...s, ...(level !== undefined ? { level } : {}), ...(mute !== undefined ? { muted: mute } : {}) } : s)),
      };
      emit("mixer", mixer);
      return null;
    }
    case "setOutput":
      return mixer;
    case "setLayout":
      languages.current = message.params.id;
      emit("language", { ...languages });
      return null;
    case "notifications":
      return notifications;
    case "dismissNotification":
      notifications = { notifications: notifications.notifications.filter((n) => n.id !== message.params.id) };
      emit("notifications", { count: notifications.notifications.length });
      return notifications;
    case "clearNotifications":
      notifications = { notifications: [] };
      emit("notifications", { count: 0 });
      return notifications;
    default:
      return undefined;
  }
}

/** What the dev host sends at start for the flyouts' modules. */
export function flyoutStart(emit: Emit): void {
  emit("language", languages);
  emit("notifications", { count: notifications.notifications.length });
}
