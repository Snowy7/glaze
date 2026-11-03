// The mock-host harness page: loads a widget folder in an iframe and plays the shell + host.
// Query params (handy for bookmarks and headless screenshots):
//   widget=<id> cols=<n> rows=<n> cell=<px> gap=<px> dpi=<96|120|144|192>
//   bg=dark|light|photo|checker  ui=0 (hide the panel)  hidden=1  edit=1
//   settings=<json>  storage=<json, seeds the instance storage>  media=playing|paused|none
//   theme=<name>  plugin=mock|real  battery=none|discharging|charging  net=wifi|ethernet|none
//   surface=none|glass|solid  text=light|dark  scale=<0.5..2>  appearance=<json>  (see look.ts)

import { MypaperError } from "../src";
import type { AppearanceState, PluginManifest, Settings, SizeInfo, WidgetManifest } from "../src/types";
import { FakeBackend, THEMES } from "./fake";
import { BridgeHost, type HostOutput } from "./host";
import { appearanceForm, lookFromParams } from "./look";
import { mockPlugin, type FolderMocks } from "./mock-plugins";
import { settingsForm, withDefaults } from "./settings-form";

interface WidgetInfo {
  id: string;
  manifest: WidgetManifest;
}
interface PluginInfo {
  id: string;
  manifest: PluginManifest;
  runnable: boolean;
}

const q = new URLSearchParams(location.search);
const num = (key: string, fallback: number) => {
  const n = Number(q.get(key));
  return q.has(key) && Number.isFinite(n) && n > 0 ? n : fallback;
};

const BACKGROUNDS = ["dark", "light", "photo", "photo-dark", "photo-light", "checker"];
const DPIS = [96, 120, 144, 192];

// ---------------------------------------------------------------- state

const backend = new FakeBackend();
const state = {
  widgets: [] as WidgetInfo[],
  plugins: [] as PluginInfo[],
  id: q.get("widget") ?? "",
  cols: num("cols", 0),
  rows: num("rows", 0),
  cell: num("cell", 96),
  gap: num("gap", 8),
  dpi: num("dpi", 96),
  bg: q.get("bg") ?? "dark",
  visible: q.get("hidden") !== "1",
  editing: q.get("edit") === "1",
  pluginMode: (q.get("plugin") === "real" ? "real" : "mock") as "mock" | "real",
  settings: {} as Settings,
  look: lookFromParams(q) as AppearanceState,
};
{
  const battery = q.get("battery");
  backend.hasBattery = battery !== "none";
  backend.charging = battery === "charging";
  const net = q.get("net");
  if (net === "wifi" || net === "ethernet" || net === "none") backend.netKind = net;
}
let host: BridgeHost | undefined;
let iframe: HTMLIFrameElement | undefined;
let folderMocks: FolderMocks = {};
const logLines: string[] = [];

const panel = document.getElementById("panel") as HTMLElement;
const stage = document.getElementById("stage") as HTMLElement;
const frame = document.getElementById("frame") as HTMLElement;
const overlay = document.getElementById("edit-overlay") as HTMLElement;
const gridEl = document.getElementById("grid") as HTMLElement;
const toastEl = document.getElementById("toast") as HTMLElement;

if (q.get("ui") === "0") document.body.classList.add("no-ui");

const current = () => state.widgets.find((w) => w.id === state.id);

// ---------------------------------------------------------------- persistence (per-viewer convenience only)

const settingsKey = (id: string) => `mypaper-harness:settings:${id}`;
function loadSettings(id: string): Settings {
  try {
    const raw = localStorage.getItem(settingsKey(id));
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return typeof parsed === "object" && parsed !== null ? { ...parsed } : {};
  } catch {
    return {};
  }
}
function saveSettings(id: string, settings: Settings) {
  try {
    localStorage.setItem(settingsKey(id), JSON.stringify(settings));
  } catch {
    /* private mode: fine */
  }
}

// ---------------------------------------------------------------- helpers

function toast(text: string) {
  toastEl.textContent = text;
  toastEl.hidden = false;
  setTimeout(() => (toastEl.hidden = true), 2500);
}

function sizeInfo(): SizeInfo {
  const w = state.cols * state.cell + (state.cols - 1) * state.gap;
  const h = state.rows * state.cell + (state.rows - 1) * state.gap;
  return { cells: [state.cols, state.rows], px: [w, h], dpi: state.dpi };
}

function clampSize(m: WidgetManifest) {
  const { min, max } = m.size;
  state.cols = Math.min(max[0], Math.max(min[0], state.cols || m.size.default[0]));
  state.rows = Math.min(max[1], Math.max(min[1], state.rows || m.size.default[1]));
  if (m.resizable === false) [state.cols, state.rows] = m.size.default;
}

function applyStageLayout() {
  const size = sizeInfo();
  stage.dataset["bg"] = state.bg;
  stage.classList.toggle("editing", state.editing);
  overlay.hidden = !state.editing;
  if (iframe) {
    iframe.style.width = `${size.px[0]}px`;
    iframe.style.height = `${size.px[1]}px`;
  }
  const pad = 24;
  // In headless/no-ui mode the page is exactly as big as the widget, so screenshots are tight.
  if (document.body.classList.contains("no-ui")) {
    stage.style.width = `${size.px[0] + pad * 2}px`;
    stage.style.height = `${size.px[1] + pad * 2}px`;
  }
  const step = state.cell + state.gap;
  gridEl.style.backgroundImage = "radial-gradient(circle, rgb(255 255 255 / 0.35) 1.5px, transparent 1.6px)";
  gridEl.style.backgroundSize = `${step}px ${step}px`;
  gridEl.style.backgroundPosition = `${pad + state.cell / 2}px ${pad + state.cell / 2}px`;
}

function renderLog() {
  const pre = document.getElementById("log");
  if (pre) pre.textContent = logLines.slice(-60).join("\n");
  const store = document.getElementById("storage");
  if (store && host) store.textContent = JSON.stringify(Object.fromEntries(host.storage), null, 2) || "{}";
}
let logQueued = false;
function logEntry(entry: { dir: "in" | "out"; msg: unknown }) {
  const text = JSON.stringify(entry.msg);
  logLines.push(`${entry.dir === "in" ? "<-" : "->"} ${text.length > 220 ? `${text.slice(0, 220)}...` : text}`);
  if (logLines.length > 400) logLines.splice(0, 200);
  if (logQueued) return;
  logQueued = true;
  requestAnimationFrame(() => {
    logQueued = false;
    renderLog();
  });
}

// ---------------------------------------------------------------- plugins

async function loadFolderMocks(id: string): Promise<FolderMocks> {
  try {
    const res = await fetch(`/__widget/${id}/mypaper.mock.json`);
    if (!res.ok) return {};
    const json: unknown = await res.json();
    return typeof json === "object" && json !== null ? { ...json } as FolderMocks : {};
  } catch {
    return {};
  }
}

async function realPlugin(id: string, method: string, params: unknown): Promise<unknown> {
  const res = await fetch(`/__api/plugin/${id}/call`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ method, params }),
  });
  const body: { result?: unknown; error?: { code?: string; message?: string } } = await res.json();
  if (body.error) throw MypaperError.fromWire(body.error);
  return body.result ?? null;
}

backend.pluginCall = (id, method, params) =>
  state.pluginMode === "real" ? realPlugin(id, method, params) : mockPlugin(id, method, params, folderMocks);
backend.onLaunch = (target) => {
  toast(`launch: ${target}`);
  logLines.push(`** launch(${JSON.stringify(target)})`);
};
backend.onMediaChange = () => host?.notifyMedia();

import.meta.hot?.on("mypaper:plugin-event", (e: { plugin: string; event: string; data: unknown }) => {
  if (state.pluginMode === "real") host?.notifyPlugin(e.plugin, e.event, e.data);
});
import.meta.hot?.on("mypaper:widget-changed", (e: { id: string }) => {
  if (e.id === state.id) void loadWidget();
});

function plugin(action: "suspend" | "resume", id: string) {
  void fetch(`/__api/plugin/${id}/lifecycle`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ action }),
  });
}

// ---------------------------------------------------------------- the widget

window.addEventListener("message", (e) => {
  if (iframe && e.source === iframe.contentWindow) void host?.handle(e.data);
});

async function loadWidget() {
  const info = current();
  host?.dispose();
  iframe?.remove();
  host = undefined;
  if (!info) return renderPanel();

  const m = info.manifest;
  clampSize(m);
  state.settings = withDefaults(m.settings ?? [], { ...loadSettings(m.id), ...settingsFromUrl() });
  folderMocks = await loadFolderMocks(m.id);

  const next = document.createElement("iframe");
  next.title = m.name;
  next.setAttribute("sandbox", "allow-scripts allow-same-origin");
  iframe = next;
  frame.prepend(next);

  const instance = `${m.id}-1`;
  host = new BridgeHost(
    instance,
    m,
    backend,
    (msg: HostOutput) => next.contentWindow?.postMessage(msg, "*"),
    { settings: state.settings, size: sizeInfo(), look: state.look },
    logEntry,
  );
  host.visible = state.visible;
  host.editing = state.editing;
  for (const [key, value] of Object.entries(jsonParam("storage"))) host.storage.set(key, value);
  applyStageLayout();
  next.src = `/__widget/${m.id}/${m.entry}`;
  renderPanel();
}

/** A JSON object from a query param, or {}. */
function jsonParam(name: string): Record<string, unknown> {
  try {
    const raw = q.get(name);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return typeof parsed === "object" && parsed !== null ? { ...parsed } : {};
  } catch {
    return {};
  }
}

const settingsFromUrl = () => jsonParam("settings");

function resize() {
  applyStageLayout();
  host?.setSize(sizeInfo());
  renderLog();
}

// ---------------------------------------------------------------- panel

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function row(label: string, control: HTMLElement, check = false): HTMLElement {
  const r = el("div", check ? "row check" : "row");
  const l = el("label", undefined, label);
  r.append(...(check ? [control, l] : [l, control]));
  return r;
}

function select(options: readonly string[], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const s = el("select");
  for (const o of options) s.append(new Option(o, o, false, o === value));
  s.addEventListener("change", () => onChange(s.value));
  return s;
}

function numberInput(value: number, min: number, max: number, onChange: (v: number) => void): HTMLInputElement {
  const i = el("input");
  i.type = "number";
  i.min = String(min);
  i.max = String(max);
  i.value = String(value);
  i.addEventListener("input", () => i.value !== "" && onChange(Number(i.value)));
  return i;
}

function checkbox(value: boolean, onChange: (v: boolean) => void): HTMLInputElement {
  const i = el("input");
  i.type = "checkbox";
  i.checked = value;
  i.addEventListener("change", () => onChange(i.checked));
  return i;
}

function renderPanel() {
  panel.replaceChildren();
  panel.append(el("h1", undefined, "mypaper widget harness"));

  const info = current();
  const ids = state.widgets.map((w) => w.id);
  const picker = select(ids, state.id, (id) => {
    state.id = id;
    state.cols = state.rows = 0;
    void loadWidget();
  });
  const reload = el("button", undefined, "Reload");
  reload.addEventListener("click", () => void loadWidget());
  const pick = el("div", "row");
  pick.append(picker, reload);
  panel.append(pick);

  if (!info) {
    panel.append(el("div", "err", state.widgets.length ? `No widget "${state.id}".` : "No widgets found. Set MYPAPER_WIDGETS to a folder of widgets."));
    return;
  }
  const m = info.manifest;

  const chips = el("div", "chips");
  const perms = [...(m.permissions ?? []), ...(m.plugins ?? []).map((p) => `plugin:${p}`)];
  if (perms.length === 0) chips.append(el("span", "chip none", "no permissions"));
  for (const p of perms) chips.append(el("span", "chip", p));
  panel.append(chips);

  // size
  panel.append(el("h2", undefined, "Size (cells)"));
  const cols = numberInput(state.cols, m.size.min[0], m.size.max[0], (v) => {
    state.cols = Math.min(m.size.max[0], Math.max(m.size.min[0], Math.round(v)));
    resize();
  });
  const rows = numberInput(state.rows, m.size.min[1], m.size.max[1], (v) => {
    state.rows = Math.min(m.size.max[1], Math.max(m.size.min[1], Math.round(v)));
    resize();
  });
  const sizeRow = el("div", "row");
  sizeRow.append(el("label", undefined, `${m.size.min.join("x")} to ${m.size.max.join("x")}`), cols, rows);
  panel.append(sizeRow);
  if (m.resizable === false) {
    cols.disabled = rows.disabled = true;
    panel.append(el("div", "note", "resizable: false, the size is locked."));
  }
  panel.append(
    row("Cell px", numberInput(state.cell, 40, 200, (v) => ((state.cell = v), resize()))),
    row("Gap px", numberInput(state.gap, 0, 40, (v) => ((state.gap = v), resize()))),
    row("DPI", select(DPIS.map(String), String(state.dpi), (v) => ((state.dpi = Number(v)), resize()))),
    row("Background", select(BACKGROUNDS, state.bg, (v) => ((state.bg = v), applyStageLayout()))),
  );

  // appearance
  panel.append(el("h2", undefined, "Appearance"));
  panel.append(appearanceForm(state.look, () => host?.setAppearance({ ...state.look })));

  // lifecycle
  panel.append(el("h2", undefined, "Host state"));
  panel.append(
    row(
      "Visible (host not held)",
      checkbox(state.visible, (v) => {
        state.visible = v;
        host?.setVisible(v);
        for (const p of m.plugins ?? []) if (state.pluginMode === "real") plugin(v ? "resume" : "suspend", p);
      }),
      true,
    ),
    row(
      "Edit mode",
      checkbox(state.editing, (v) => {
        state.editing = v;
        host?.setEditing(v);
        applyStageLayout();
      }),
      true,
    ),
  );

  // settings
  panel.append(el("h2", undefined, "Settings"));
  panel.append(
    settingsForm(m.settings ?? [], state.settings, (s) => {
      state.settings = s;
      saveSettings(m.id, s);
      host?.setSettings(s);
    }),
  );
  const reset = el("button", undefined, "Reset settings");
  reset.addEventListener("click", () => {
    saveSettings(m.id, {});
    state.settings = withDefaults(m.settings ?? [], {});
    host?.setSettings(state.settings);
    renderPanel();
  });
  panel.append(reset);

  // data sources
  if (m.permissions?.includes("system.stats")) {
    panel.append(el("h2", undefined, "System"));
    const batteryMode = !backend.hasBattery ? "none" : backend.charging ? "charging" : "discharging";
    panel.append(
      row("Battery", select(["none", "discharging", "charging"], batteryMode, (v) => {
        backend.hasBattery = v !== "none";
        backend.charging = v === "charging";
      })),
      row("Network", select(["wifi", "ethernet", "none"], backend.netKind, (v) => {
        backend.netKind = v === "ethernet" || v === "none" ? v : "wifi";
      })),
    );
  }
  if (m.permissions?.includes("media")) {
    panel.append(el("h2", undefined, "Media"));
    panel.append(row("State", select(["playing", "paused", "none"], q.get("media") ?? (backend.present ? (backend.playing ? "playing" : "paused") : "none"), (v) => backend.setMedia(v === "none" ? "none" : v === "paused" ? "paused" : "playing"))));
  }
  if (m.permissions?.includes("theme")) {
    panel.append(el("h2", undefined, "Theme"));
    const names = Object.keys(THEMES);
    panel.append(
      row("Palette", select(names, names.find((n) => THEMES[n]?.name === backend.theme_.name) ?? names[0] ?? "", (v) => {
        backend.theme_ = THEMES[v] ?? backend.theme_;
        host?.notifyTheme();
      })),
    );
  }
  if (m.plugins?.length) {
    panel.append(el("h2", undefined, "Plugins"));
    panel.append(
      row("Calls go to", select(["mock", "real"], state.pluginMode, (v) => {
        state.pluginMode = v === "real" ? "real" : "mock";
        void loadWidget();
      })),
    );
    for (const id of m.plugins) {
      const p = state.plugins.find((x) => x.id === id);
      const text = !p ? `plugin "${id}" not found` : p.runnable ? `${id}: real plugin found` : `${id}: found, but its executable isn't built (cargo build -p ...)`;
      panel.append(el("div", "note", text));
    }
    panel.append(el("div", "note", "Mock: built-in, or mypaper.mock.json in the widget folder."));
  }

  // storage + log
  panel.append(el("h2", undefined, "Storage (this instance, in memory)"));
  const store = el("pre");
  store.id = "storage";
  const clear = el("button", undefined, "Clear storage");
  clear.addEventListener("click", () => {
    host?.storage.clear();
    renderLog();
  });
  panel.append(store, clear);
  panel.append(el("h2", undefined, "Wire log"));
  const log = el("pre");
  log.id = "log";
  panel.append(log);
  renderLog();
}

// ---------------------------------------------------------------- start

async function start() {
  const [widgets, plugins]: [WidgetInfo[], PluginInfo[]] = await Promise.all([
    fetch("/__api/widgets").then((r) => r.json()),
    fetch("/__api/plugins").then((r) => r.json()),
  ]);
  state.widgets = widgets;
  state.plugins = plugins;
  if (!state.id || !current()) state.id = widgets[0]?.id ?? "";
  const mode = q.get("media");
  if (mode === "none" || mode === "paused" || mode === "playing") backend.setMedia(mode);
  const theme = q.get("theme");
  if (theme && THEMES[theme]) backend.theme_ = THEMES[theme];
  await loadWidget();
}

void start();
