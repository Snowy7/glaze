// Studio as a widget shell, for the live preview: the real widget pages run in sandboxed iframes
// and talk to Studio over the SDK's wire protocol (docs/widgets.md, "Wire protocol"). Studio
// answers with the instance's settings and look and with sample data. Nothing a widget asks for
// leaves the preview: `launch` does nothing, there are no files to pick and no plugins.

import type { Appearance, WidgetManifest, WidgetSetting } from "./api";
import type { Json } from "./barsSchema";

/** What a widget is told about itself: the handshake's state, and what later events update. */
export type FrameState = {
  instance: string;
  manifest: WidgetManifest;
  settings: Record<string, Json>;
  size: { cells: [number, number]; px: [number, number]; dpi: number };
  /** The merged look (theme, then the look of all widgets, then the widget's own). */
  appearance: Appearance;
  /** The active theme's name, for `theme.current`. */
  theme: string;
};

type Request = { mypaper: 1; id: number; method: string; params?: unknown };

const isRequest = (data: unknown): data is Request =>
  typeof data === "object" && data !== null && "mypaper" in data && data.mypaper === 1 && "id" in data && typeof data.id === "number" && "method" in data && typeof data.method === "string";

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

/** The manifest's defaults under the instance's own settings. */
export function settingsWithDefaults(settings: readonly WidgetSetting[] | undefined, own: Record<string, Json>): Record<string, Json> {
  const defaults = Object.fromEntries((settings ?? []).filter((s) => s.default !== undefined).map((s) => [s.key, s.default as Json]));
  return { ...defaults, ...own };
}

// --- Sample data: plausible and gently moving, the same for every widget on the stage ---

let cpu = 14;
let memory = 9.2e9;
const started = Date.now();

function stats() {
  cpu = Math.min(64, Math.max(4, cpu + (Math.random() - 0.5) * 9));
  memory = Math.min(12.5e9, Math.max(7.5e9, memory + (Math.random() - 0.5) * 2.4e8));
  return {
    cpu,
    memory: { used: memory, total: 16e9 },
    gpu: Math.max(2, cpu * 0.6),
    net: { up: 42_000 + Math.random() * 20_000, down: 380_000 + Math.random() * 240_000 },
    disks: [
      { name: "C:", used: 318e9, total: 512e9 },
      { name: "D:", used: 1.2e12, total: 2e12 },
    ],
    battery: { percent: 84, charging: true },
    uptime: 4 * 3600 + 22 * 60 + (Date.now() - started) / 1000,
    network: { kind: "wifi", name: "Home", signal: 78 },
  };
}

const ARTWORK = `data:image/svg+xml;base64,${btoa(
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x2="1" y2="1"><stop offset="0" stop-color="#6b7a99"/><stop offset="1" stop-color="#2a2f3d"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><circle cx="40" cy="26" r="10" fill="#d7dbe6" opacity=".7"/></svg>',
)}`;

const media = () => ({ title: "Weightless", artist: "Marconi Union", album: "Weightless", app: "Spotify", playing: true, position: 74 + ((Date.now() - started) / 1000) % 300, duration: 480, artwork: ARTWORK });

/** Smoothly moving spectrum bins in 0..1, low frequencies louder. */
function spectrum(bins: number, t: number): number[] {
  return Array.from({ length: bins }, (_, i) => {
    const x = i / bins;
    const wave = 0.5 + 0.5 * Math.sin(t * 3.1 + i * 0.37) * Math.cos(t * 1.7 - i * 0.11);
    return Math.max(0, Math.min(1, (1 - x) ** 1.4 * (0.35 + 0.65 * wave)));
  });
}

// --- The bridge ---

type Bridge = {
  frame: Window;
  state: () => FrameState;
  storage: Map<string, unknown>;
  timers: Map<string, number>;
};

const bridges = new Map<Window, Bridge>();
const sharedStorage = new Map<string, Map<string, unknown>>();

function post(bridge: Bridge, message: object) {
  bridge.frame.postMessage({ mypaper: 1, ...message }, "*");
}

function answer(bridge: Bridge, request: Request): unknown {
  const params = isObject(request.params) ? request.params : {};
  const state = bridge.state();
  switch (request.method) {
    case "hello":
      return { instance: state.instance, widget: state.manifest, settings: state.settings, size: state.size, visible: true, editing: false, appearance: state.appearance, themeCss: null };
    case "storage.get": {
      const store = typeof params.shared === "string" ? (sharedStorage.get(params.shared) ?? new Map()) : bridge.storage;
      return store.get(String(params.key)) ?? null;
    }
    case "storage.set": {
      const name = typeof params.shared === "string" ? params.shared : null;
      const store = name ? (sharedStorage.get(name) ?? new Map<string, unknown>()) : bridge.storage;
      if (name) sharedStorage.set(name, store);
      if (params.value === undefined) store.delete(String(params.key));
      else store.set(String(params.key), params.value);
      return null;
    }
    case "system.stats":
      return stats();
    case "media.current":
      return media();
    case "theme.current":
      return { name: state.theme, palette: { dominant: "#2b2f38", accent: "#c8ccd6" } };
    case "user.info":
      return { name: "there", account: "you" };
    case "media.control":
    case "launch":
      return null;
    case "pickFile":
    case "appIcon":
      return null;
    case "subscribe":
      subscribe(bridge, params);
      return null;
    case "unsubscribe": {
      const sub = String(params.sub);
      window.clearInterval(bridge.timers.get(sub));
      bridge.timers.delete(sub);
      return null;
    }
    case "plugin":
      throw Object.assign(new Error("Plugins don't run in Studio's preview"), { code: "PluginUnavailable" });
    default:
      throw Object.assign(new Error(`${request.method} isn't available in the preview`), { code: "NotFound" });
  }
}

function subscribe(bridge: Bridge, params: Record<string, unknown>) {
  const sub = String(params.sub);
  const send = (value: unknown) => post(bridge, { event: "sub", data: { sub, value } });
  if (params.topic === "system") {
    send(stats());
    const every = Math.max(1000, Number(params.intervalMs) || 1000);
    bridge.timers.set(sub, window.setInterval(() => !document.hidden && send(stats()), every));
  } else if (params.topic === "audio") {
    const bins = Math.min(128, Math.max(64, Number(params.bins) || 64));
    bridge.timers.set(sub, window.setInterval(() => !document.hidden && send(spectrum(bins, performance.now() / 1000)), 66));
  }
}

function onMessage(event: MessageEvent) {
  const bridge = event.source ? bridges.get(event.source as Window) : undefined;
  if (!bridge || !isRequest(event.data)) return;
  const request = event.data;
  try {
    post(bridge, { id: request.id, result: answer(bridge, request) });
  } catch (error) {
    const code = isObject(error) && typeof error.code === "string" ? error.code : "Internal";
    post(bridge, { id: request.id, error: { code, message: error instanceof Error ? error.message : String(error) } });
  }
}

let listening = false;

/**
 * Serves one widget iframe. `state` is read on every request, so it may change; call `push` after
 * a change the widget should hear about at once (settings, size, look).
 */
export function attachFrame(frame: HTMLIFrameElement, state: () => FrameState) {
  if (!listening) {
    window.addEventListener("message", onMessage);
    listening = true;
  }
  const target = frame.contentWindow;
  if (!target) throw new Error("the widget frame isn't in the document");
  const bridge: Bridge = { frame: target, state, storage: new Map(), timers: new Map() };
  bridges.set(target, bridge);
  return {
    push(event: "settings" | "size" | "appearance") {
      const current = state();
      const data = event === "settings" ? current.settings : event === "size" ? current.size : { appearance: current.appearance, themeCss: null };
      post(bridge, { event, data });
    },
    dispose() {
      for (const timer of bridge.timers.values()) window.clearInterval(timer);
      bridges.delete(target);
    },
  };
}
