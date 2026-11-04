// Dev-only host: lets the page run in a plain browser (and for screenshots) with realistic data.
// Loaded by `connect()` only when `window.chrome.webview` is absent.
//
//   ?bar=top|dock   ?preset=monochrome|macos|windows11|islands|neon-glass|floating
//   ?pos=bottom|top|left|right   ?size=48   ?h=28   ?autohide=1   ?scheme=light|dark
//   ?text=light|dark   ?surface=none|glass|solid   ?wall=dark|light|vivid   ?reduce=1   ?perf=1

import type { Transport } from "./host";
import { mockConfig, PRESETS } from "./mock-config";
import type {
  Appearance,
  BarKind,
  DockItem,
  DockPosition,
  EventMap,
  EventName,
  Hello,
  IconPack,
  LookPayload,
  Preset,
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

const win = (hwnd: number, title: string, active = false, minimized = false, attention = false) => ({
  hwnd,
  title,
  active,
  minimized,
  attention,
});

const initialApps = (): DockItem[] => [
  { key: "files", name: "Files", path: "explorer.exe", icon: ICONS.files, pinned: true, windows: [win(101, "Documents")] },
  { key: "browser", name: "Browser", path: "browser.exe", icon: ICONS.browser, pinned: true, windows: [win(201, "mypaper — bars", true), win(202, "Docs: Windows 11 shell")] },
  { key: "terminal", name: "Terminal", path: "wt.exe", icon: ICONS.terminal, pinned: true, windows: [win(301, "pwsh", false, false, true)] },
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

/** A stand-in wallpaper, as CSS and as the snapshot the engine would publish (an SVG). */
function wallpaper(kind: string, w: number, h: number): { css: string; snapshot: string } {
  const blobs =
    kind === "light"
      ? [["18%", "30%", "#ffffff"], ["85%", "80%", "#d8dde6"], ["50%", "100%", "#c9d4ea"]]
      : kind === "vivid"
        ? [["15%", "20%", "#ff5fa2"], ["80%", "25%", "#6b5cff"], ["40%", "95%", "#00c2c7"], ["95%", "95%", "#ffb347"]]
        : [["18%", "30%", "#4a5160"], ["85%", "80%", "#3b3346"], ["45%", "100%", "#2b3a4a"]];
  const base = kind === "light" ? "#eceef2" : kind === "vivid" ? "#1b1530" : "#17181c";
  const gradients = blobs
    .map(([x, y, c], i) => `<radialGradient id="g${i}" cx="${x}" cy="${y}" r="45%"><stop offset="0" stop-color="${c}"/><stop offset="1" stop-color="${c}" stop-opacity="0"/></radialGradient>`)
    .join("");
  const rects = blobs.map((_, i) => `<rect width="100%" height="100%" fill="url(#g${i})"/>`).join("");
  // Some detail, so glass visibly blurs what's behind it.
  const stripes = Array.from({ length: 24 }, (_, i) => `<rect x="${(i * w) / 24}" y="0" width="${w / 60}" height="${h}" fill="#fff" opacity=".07"/>`).join("");
  const picture = (blur: number) =>
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs>${gradients}<filter id="b" x="0" y="0" width="1" height="1"><feGaussianBlur stdDeviation="${blur}" edgeMode="duplicate"/></filter></defs><g filter="${blur ? "url(#b)" : "none"}"><rect width="100%" height="100%" fill="${base}"/>${rects}${stripes}</g></svg>`;
  const url = (svg: string) => `data:image/svg+xml,${encodeURIComponent(svg)}`;
  const css = `html{background:url("${url(picture(0))}") 0 0 / 100% 100% no-repeat,${base}}`;
  return { css, snapshot: url(picture(28)) };
}

const isPreset = (name: string | null): name is Preset => !!name && name in PRESETS;

export function createMockTransport(): Transport {
  const q = new URLSearchParams(location.search);
  const bar: BarKind = q.get("bar") === "dock" ? "dock" : "top";
  const pos = q.get("pos");
  const position: DockPosition | undefined = pos === "left" || pos === "right" || pos === "top" || pos === "bottom" ? pos : undefined;
  const dark = q.get("text") === "dark";
  const surfaceParam = q.get("surface");
  const surface: Appearance["surface"] = surfaceParam === "none" || surfaceParam === "solid" ? surfaceParam : "glass";
  const wall = q.get("wall") ?? (dark ? "light" : "dark");
  const autohide = q.has("autohide");
  const presetName = q.get("preset");
  const preset: Preset = isPreset(presetName) ? presetName : "monochrome";

  const appearance: Appearance = {
    surface,
    tint: dark ? "#f2f2f2" : "#0e0e0e",
    opacity: surface === "solid" ? 0.96 : 0.6,
    blur: 30,
    radius: bar === "dock" ? 16 : 0,
    border: true,
    text: dark ? "dark" : "light",
    accent: "#ffffff",
    font: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
    scale: 1,
    shadow: false,
  };
  const scheme = q.get("scheme");
  const config = mockConfig(preset, {
    dock: {
      enabled: true,
      ...(position ? { position } : {}),
      ...(q.has("size") ? { size: Number(q.get("size")) } : {}),
      ...(autohide ? { autohide } : {}),
    },
    ...(q.has("h") ? { top: { height: Number(q.get("h")) } } : {}),
    ...(scheme === "light" || scheme === "dark" ? { look: { scheme } } : {}),
  });
  const look: LookPayload = {
    appearance,
    themeCss: null,
    system: { accent: ["#0078d4", "#0091f8", "#4cc2ff", "#99ebff", "#0067c0", "#003e92", "#001a68"], scheme: dark ? "light" : "dark" },
    vars: {},
    iconPack: null,
    assets: {},
  };

  // A stand-in wallpaper, so glass and shadows read the way they will on the desktop.
  const art = wallpaper(wall, innerWidth, innerHeight);
  const style = document.createElement("style");
  style.textContent = art.css;
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
      { id: "sync", name: "Dropbox" },
      { id: "chat", name: "Signal", icon: ICONS.mail },
      { id: "overflow", name: "Show hidden icons" },
    ]);
    emit("workspaces", workspaces);
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

  // ?poke=<selector>: the pointer arrives over that element (magnify, labels, previews), for
  // screenshots without a mouse.
  const poke = q.get("poke");
  if (poke) {
    setTimeout(() => {
      const target = document.querySelector(poke);
      if (!(target instanceof HTMLElement)) return;
      const r = target.getBoundingClientRect();
      const at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, bubbles: false };
      for (const el of [target.closest(".pill"), target.closest(".items"), target]) {
        el?.dispatchEvent(new PointerEvent("pointerenter", at));
      }
      target.closest(".items")?.dispatchEvent(new PointerEvent("pointermove", { ...at, bubbles: true }));
    }, 400);
  }

  // ?dump=<selector>: after 2 s, the matching elements' boxes go into body[data-dump] (read it
  // with a headless browser's --dump-dom).
  const dump = q.get("dump");
  if (dump) {
    setTimeout(() => {
      const boxes = [...document.querySelectorAll(dump)].map((el) => {
        const r = el.getBoundingClientRect();
        return [el.className, Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)];
      });
      document.body.dataset.dump = JSON.stringify(boxes);
    }, 2000);
  }

  // The pointer touching the screen edge reveals an auto-hide dock (the real host does this).
  if (bar === "dock" && autohide) {
    let atEdge = false;
    const edge = config.dock.position;
    window.addEventListener("pointermove", (e) => {
      const hit =
        edge === "bottom"
          ? e.clientY >= innerHeight - 2
          : edge === "top"
            ? e.clientY <= 1
            : edge === "left"
              ? e.clientX <= 1
              : e.clientX >= innerWidth - 2;
      if (hit && !atEdge) emit("reveal", null);
      atEdge = hit;
    });
  }

  const request = (message: Request) => {
    switch (message.method) {
      case "hello": {
        // The bundled packs ship with the page (public/icons), as the host serves them.
        if (config.icons && !look.iconPack) {
          const base = `icons/${config.icons}/`;
          fetch(`${base}icons.json`)
            .then((r) => r.json() as Promise<Omit<IconPack, "base">>)
            .then((pack) => {
              look.iconPack = { ...pack, base };
              request(message);
            })
            .catch(() => console.warn("[mock host] no icon pack", config.icons));
          return;
        }
        const result: Hello = {
          bar,
          monitor: { device: "\\\\.\\DISPLAY1", primary: true, width: innerWidth, height: innerHeight },
          config,
          look,
          mode: { reduceMotion: q.has("reduce"), performance: q.has("perf") },
          // The mock's window is the whole monitor.
          backdrop: { url: art.snapshot, width: innerWidth, height: innerHeight, x: 0, y: 0 },
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
      case "previewOpen": {
        reply(
          message.id,
          message.params.hwnds.map((hwnd, i) => ({ hwnd, width: i % 2 ? 1280 : 1920, height: i % 2 ? 1024 : 1080 })),
        );
        return;
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
