// Dev-only host: lets the page run in a plain browser (and for screenshots) with realistic data.
// Loaded by `connect()` only when `window.chrome.webview` is absent.
//
//   ?bar=top|dock   ?pos=bottom|left|right   ?size=48   ?h=28   ?autohide=1
//   ?text=light|dark   ?surface=none|glass|solid   ?wall=dark|light

import { flyoutMock, flyoutStart } from "./flyouts/mock";
import { LAUNCHER_LAYOUTS } from "./flyouts/protocol";
import type { Transport } from "./host";
import type {
  Appearance,
  BarKind,
  BarsConfig,
  DockItem,
  DockPosition,
  EventMap,
  EventName,
  Hello,
  Request,
} from "./protocol";

function isRequest(data: unknown): data is Request {
  return (
    typeof data === "object" &&
    data !== null &&
    "id" in data &&
    typeof data.id === "number" &&
    "method" in data &&
    typeof data.method === "string"
  );
}

// --- Fake icons: tinted tiles with a simple glyph, as inline SVG data URLs ---------------------------

const tile = (from: string, to: string, glyph: string): string => {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs>` +
    `<rect x="2" y="2" width="60" height="60" rx="15" fill="url(#g)"/>${glyph}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
};
const S = (width = 4) => `fill="none" stroke="#fff" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round"`;
const ICONS = {
  files: tile("#6c7a89", "#3b4652", `<path d="M14 24h14l4 5h18v19H14Z" ${S()}/>`),
  browser: tile("#8a8f98", "#4a4e57", `<circle cx="32" cy="32" r="14" ${S()}/><path d="M18 32h28M32 18c8 8 8 20 0 28M32 18c-8 8-8 20 0 28" ${S(3)}/>`),
  terminal: tile("#3c3c42", "#17171a", `<path d="M18 22l10 10-10 10M34 44h14" ${S()}/>`),
  notes: tile("#c9b458", "#9b872c", `<path d="M20 20h24M20 30h24M20 40h14" ${S()}/>`),
  music: tile("#b0566c", "#7a2c42", `<path d="M26 44V20l18-4v24" ${S()}/><circle cx="22" cy="44" r="5" fill="#fff"/><circle cx="40" cy="40" r="5" fill="#fff"/>`),
  mail: tile("#5f86b8", "#34567f", `<rect x="14" y="20" width="36" height="26" rx="4" ${S()}/><path d="M16 24l16 12 16-12" ${S(3)}/>`),
  photos: tile("#7da58b", "#436a52", `<rect x="14" y="16" width="36" height="32" rx="4" ${S(3)}/><path d="M16 44l10-12 8 8 6-6 8 10" ${S(3)}/>`),
  code: tile("#5b5fc7", "#32358a", `<path d="M26 22L16 32l10 10M38 22l10 10-10 10" ${S()}/>`),
  calc: tile("#888", "#555", `<rect x="18" y="14" width="28" height="36" rx="4" ${S(3)}/><path d="M24 24h16M24 34h4M32 34h4M24 42h4M32 42h4" ${S(3)}/>`),
};

const win = (hwnd: number, title: string, active = false, minimized = false) => ({ hwnd, title, active, minimized });

const initialApps = (): DockItem[] => [
  { key: "files", name: "Files", path: "explorer.exe", icon: ICONS.files, pinned: true, windows: [win(101, "Documents")] },
  { key: "browser", name: "Browser", path: "browser.exe", icon: ICONS.browser, pinned: true, windows: [win(201, "mypaper — bars", true), win(202, "Docs: Windows 11 shell")] },
  { key: "terminal", name: "Terminal", path: "wt.exe", icon: ICONS.terminal, pinned: true, windows: [win(301, "pwsh")] },
  { key: "notes", name: "Notes", path: "notes.exe", icon: ICONS.notes, pinned: true, windows: [] },
  { key: "music", name: "Music", path: "music.exe", icon: ICONS.music, pinned: true, windows: [] },
  { key: "mail", name: "Mail", path: "mail.exe", icon: ICONS.mail, pinned: true, windows: [] },
  { key: "photos", name: "Photos", path: "photos.exe", icon: ICONS.photos, pinned: true, windows: [] },
  { key: "code", name: "Code", path: "code.exe", icon: ICONS.code, pinned: false, windows: [win(401, "dock.ts — bars")] },
  { key: "calc", name: "Calculator", path: "calc.exe", pinned: false, windows: [win(501, "Calculator", false, true)] },
];

const TRACKS = [
  { title: "Weightless", artist: "Marconi Union", app: "Music" },
  { title: "An Ending (Ascent)", artist: "Brian Eno", app: "Music" },
  { title: "Teardrop", artist: "Massive Attack", app: "Music" },
];

export function createMockTransport(): Transport {
  const q = new URLSearchParams(location.search);
  const bar: BarKind = q.get("bar") === "dock" ? "dock" : "top";
  const pos = q.get("pos");
  const position: DockPosition = pos === "left" || pos === "right" ? pos : "bottom";
  const dark = q.get("text") === "dark";
  const surfaceParam = q.get("surface");
  const surface: Appearance["surface"] = surfaceParam === "none" || surfaceParam === "solid" ? surfaceParam : "glass";
  const wallDark = (q.get("wall") ?? (dark ? "light" : "dark")) !== "light";
  const autohide = q.has("autohide");

  const appearance: Appearance = {
    surface,
    tint: dark ? "#f2f2f2" : "#101010",
    opacity: surface === "solid" ? 0.96 : 0.55,
    blur: 24,
    radius: bar === "dock" ? 16 : 0,
    border: true,
    text: dark ? "dark" : "light",
    accent: "#ffffff",
    font: "Segoe UI Variable",
    scale: 1,
    shadow: true,
  };
  const config: BarsConfig = {
    version: 1,
    taskbar: { mode: "autohide" },
    top: {
      enabled: true,
      monitors: "all",
      height: Number(q.get("h")) || 28,
      left: ["start", "workspaces", "app"],
      center: ["media"],
      right: ["tray", "stats", "language", "network", "volume", "battery", "quick", "notifications", "clock", "power"],
      clock: { seconds: false, format: "ddd d MMM  HH:mm" },
    },
    dock: {
      enabled: true,
      monitors: "all",
      style: "floating",
      position,
      size: Number(q.get("size")) || 48,
      autohide,
      magnify: true,
      running: true,
      pinned: [],
    },
    launcher: {
      winKey: true,
      layout: LAUNCHER_LAYOUTS.find((l) => l === q.get("launcher")) ?? "win11",
      pinned: ["Terminal", "Firefox", "Visual Studio Code", "File Explorer", "Spotify", "Settings", "Photos", "Calculator"].map((name) => ({
        path: `shell:AppsFolder\\${name.replace(/\s/g, "")}`,
        name,
      })),
      files: true,
    },
    tray: { host: false },
    calendar: { zones: [{ zone: "America/New_York" }, { zone: "Asia/Tokyo", label: "Tokyo office" }] },
    audio: { switchOutput: false },
  };

  // A stand-in wallpaper, so glass and shadows read the way they will on the desktop.
  const style = document.createElement("style");
  style.textContent = wallDark
    ? `html{background:radial-gradient(900px 520px at 18% 30%,#4a5160,transparent),radial-gradient(800px 500px at 85% 80%,#3b3346,transparent),#17181c}`
    : `html{background:radial-gradient(900px 520px at 18% 30%,#ffffff,transparent),radial-gradient(800px 500px at 85% 80%,#d8dde6,transparent),#eceef2}`;
  document.head.append(style);

  let listener: ((event: { data: unknown }) => void) | undefined;
  const emit = <E extends EventName>(event: E, data: EventMap[E]) => listener?.({ data: { event, data } });
  const reply = (id: number, result: unknown) => setTimeout(() => listener?.({ data: { id, result } }), 0);

  let apps = initialApps();
  let volume = { level: 0.62, muted: false };
  let workspaces = { count: 4, current: 1 };
  let track = 0;
  let playing = true;
  let nextHwnd = 900;
  const emitApps = () => {
    apps = [...apps.filter((a) => a.pinned), ...apps.filter((a) => !a.pinned)];
    emit("apps", structuredClone(apps));
  };
  const emitMedia = () => emit("media", { ...(TRACKS[track] ?? TRACKS[0] ?? { title: "", artist: "", app: "" }), playing });

  function start() {
    emitApps();
    emit("foreground", { name: "Browser", icon: ICONS.browser });
    emit("clock", { now: Date.now() });
    emitMedia();
    emit("volume", volume);
    emit("network", { kind: "wifi", name: "Home-5G", signal: 72 });
    emit("battery", { percent: 84, charging: true });
    emit("tray", [
      { id: "sync", name: "Dropbox", pinned: true },
      { id: "chat", name: "Signal", icon: ICONS.mail, pinned: true },
      { id: "music", name: "Spotify", icon: ICONS.music, pinned: false },
      { id: "photos", name: "Photos backup", icon: ICONS.photos, pinned: false },
      { id: "code", name: "Code helper", icon: ICONS.code, pinned: false },
    ]);
    emit("workspaces", workspaces);
    flyoutStart(emit);
    let cpu = 18;
    let mem = 9.4e9;
    const stats = () => {
      cpu = Math.max(2, Math.min(95, cpu + (Math.random() - 0.5) * 14));
      mem = Math.max(6e9, Math.min(14e9, mem + (Math.random() - 0.5) * 3e8));
      emit("stats", { cpu, memory: { used: mem, total: 16e9 } });
    };
    stats();
    setInterval(stats, 1500);
    setInterval(() => emit("clock", { now: Date.now() }), 1000);
  }

  // The pointer touching the screen edge reveals an auto-hide dock (the real host does this).
  if (bar === "dock" && autohide) {
    let atEdge = false;
    window.addEventListener("pointermove", (e) => {
      const hit =
        position === "bottom" ? e.clientY >= innerHeight - 2 : position === "left" ? e.clientX <= 1 : e.clientX >= innerWidth - 2;
      if (hit && !atEdge) emit("reveal", null);
      atEdge = hit;
    });
  }

  const request = (message: Request) => {
    const flyout = flyoutMock(message, emit);
    if (flyout !== undefined) return reply(message.id, flyout);
    switch (message.method) {
      case "hello": {
        const result: Hello = {
          bar,
          monitor: { device: "\\\\.\\DISPLAY1", primary: true, width: innerWidth, height: innerHeight },
          config,
          appearance,
          themeCss: null,
          dpi: 96,
        };
        reply(message.id, result);
        setTimeout(start, 20);
        return;
      }
      case "volume": {
        const { set, delta, mute } = message.params;
        if (set !== undefined) volume.level = set;
        if (delta !== undefined) volume.level += delta;
        volume.level = Math.max(0, Math.min(1, volume.level));
        if (mute !== undefined) volume.muted = mute;
        volume = { ...volume };
        emit("volume", volume);
        reply(message.id, volume);
        return;
      }
      case "media": {
        const { action } = message.params;
        if (action === "toggle") playing = !playing;
        else track = (track + (action === "next" ? 1 : TRACKS.length - 1)) % TRACKS.length;
        emitMedia();
        break;
      }
      case "workspace": {
        const next = workspaces.current + (message.params.direction === "left" ? -1 : 1);
        workspaces = { ...workspaces, current: Math.max(0, Math.min(workspaces.count - 1, next)) };
        emit("workspaces", workspaces);
        break;
      }
      case "activate": {
        const p = message.params;
        apps = apps.map((a) => ({
          ...a,
          windows: a.windows.map((w, i) => ({
            ...w,
            active: "hwnd" in p ? w.hwnd === p.hwnd : a.key === p.app && i === 0,
            minimized: false,
          })),
        }));
        const front = apps.find((a) => a.windows.some((w) => w.active));
        if (front) emit("foreground", { name: front.name, icon: front.icon });
        emitApps();
        break;
      }
      case "launch": {
        const app = apps.find((a) => a.path === message.params.path);
        if (app && (app.windows.length === 0 || message.params.newInstance)) {
          app.windows.unshift(win(nextHwnd++, `${app.name} — new window`));
          emitApps();
        }
        break;
      }
      case "close": {
        const { hwnd } = message.params;
        apps = apps
          .map((a) => ({ ...a, windows: a.windows.filter((w) => w.hwnd !== hwnd) }))
          .filter((a) => a.pinned || a.windows.length > 0);
        emitApps();
        break;
      }
      case "pin": {
        const { path, index } = message.params;
        const item = apps.find((a) => a.path === path);
        if (item) {
          apps = apps.filter((a) => a !== item);
          const pinned = apps.filter((a) => a.pinned);
          pinned.splice(index ?? pinned.length, 0, { ...item, pinned: true });
          apps = [...pinned, ...apps.filter((a) => !a.pinned)];
          emitApps();
        }
        break;
      }
      case "unpin": {
        const { path } = message.params;
        apps = apps
          .map((a) => (a.path === path ? { ...a, pinned: false } : a))
          .filter((a) => a.pinned || a.windows.length > 0);
        emitApps();
        break;
      }
      case "reorder": {
        const { paths } = message.params;
        const pinned = paths.flatMap((p) => apps.filter((a) => a.pinned && a.path === p));
        apps = [...pinned, ...apps.filter((a) => !a.pinned)];
        emitApps();
        break;
      }
      default:
        break;
    }
    reply(message.id, null);
  };

  return {
    postMessage(message) {
      if (!isRequest(message)) return;
      if (message.method !== "region") console.debug("[mock host]", message.method, message.params);
      else console.debug("[mock host] region", JSON.stringify(message.params.rects));
      request(message);
    },
    addEventListener(_type, cb) {
      listener = cb;
    },
  };
}
