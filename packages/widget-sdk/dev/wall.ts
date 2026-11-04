// Every widget at once, at one size, in one style and surface, over one background: a single
// headless screenshot reviews the whole set.
//   style=minimal|depth|neon|paper|aero  surface=none|glass|solid  bg=photo-dark|photo-light
//   size=default|min|max|<cols>x<rows> (clamped per widget)  only=a,b,c  skip=a,b  cell=<px> gap=<px>
//   text=auto|light|dark  scale=<n>  settings=<json by widget id>  media=playing|paused|none
//   bgimg=<url> any image as the background (a theme's, or /@fs/C:/path/to/wallpaper.jpg)
//   plus the data and appearance parameters of the gallery

import { MypaperError } from "../src";
import type { AppearanceState, Settings, SizeInfo, WidgetManifest } from "../src/types";
import { FakeBackend } from "./fake";
import { BridgeHost, type HostOutput } from "./host";
import { type FolderMocks, mockPlugin, setMockLocate } from "./mock-plugins";
import { seedSharedFromParams } from "./seed";
import { lookFromParams } from "./look";
import { withDefaults } from "./settings-form";

const q = new URLSearchParams(location.search);
setMockLocate(q.get("locate"));
const num = (key: string, fallback: number) => {
  const n = Number(q.get(key));
  return q.has(key) && Number.isFinite(n) && n > 0 ? n : fallback;
};
const list = (key: string) => (q.get(key) ?? "").split(",").map((s) => s.trim()).filter(Boolean);
const settingsById = (): Record<string, Record<string, unknown>> => {
  try {
    const parsed: unknown = JSON.parse(q.get("settings") ?? "{}");
    return typeof parsed === "object" && parsed !== null ? { ...parsed } : {};
  } catch {
    return {};
  }
};

const CELL = num("cell", 96);
const GAP = num("gap", 8);

const backend = new FakeBackend();
seedSharedFromParams(backend, new URLSearchParams(location.search));
backend.hasBattery = q.get("battery") !== "none";
const media = q.get("media");
if (media === "none" || media === "paused" || media === "playing") backend.setMedia(media);
backend.onLaunch = () => {};

const sheet = document.getElementById("sheet") as HTMLElement;
const hosts = new Map<Window, BridgeHost>();
window.addEventListener("message", (e) => {
  if (e.source) void hosts.get(e.source as Window)?.handle(e.data);
});

function sizeFor(m: WidgetManifest): [number, number] {
  const want = q.get("size") ?? "default";
  if (want === "min" || want === "default" || want === "max") return [...m.size[want]] as [number, number];
  const [c = 1, r = 1] = want.split("x").map(Number);
  return [Math.min(m.size.max[0], Math.max(m.size.min[0], c)), Math.min(m.size.max[1], Math.max(m.size.min[1], r))];
}

async function main() {
  const all: { id: string; manifest: WidgetManifest }[] = await fetch("/__api/widgets").then((r) => r.json());
  const only = list("only");
  const skip = list("skip");
  const widgets = all.filter((w) => (only.length === 0 || only.includes(w.id)) && !skip.includes(w.id));
  // Every widget's canned plugin answers (mypaper.mock.json), merged: they are keyed by plugin id.
  const mocks: FolderMocks = {};
  for (const { id } of widgets) {
    const res = await fetch(`/__widget/${id}/mypaper.mock.json`).catch(() => null);
    if (res?.ok) Object.assign(mocks, (await res.json()) as FolderMocks);
  }
  backend.pluginCall = (pid, method, params) => mockPlugin(pid, method, params, mocks);
  const base = lookFromParams(q);
  const bg = q.get("bg") ?? "photo-dark";
  const text =
    q.get("text") === "light" || q.get("text") === "dark" ? base.appearance.text : bg.includes("light") ? "dark" : "light";
  const look: AppearanceState = { appearance: { ...base.appearance, text }, themeCss: base.themeCss };
  const perWidget = settingsById();

  const stage = document.createElement("div");
  stage.className = `stage bg-${bg}`;
  const image = q.get("bgimg");
  if (image) stage.style.background = `url(${JSON.stringify(image)}) center / cover`;
  const head = document.createElement("div");
  head.className = "head";
  head.textContent = `${look.appearance.style} / ${look.appearance.surface} / ${bg}`;
  const wall = document.createElement("div");
  wall.className = "wall";
  stage.append(head, wall);
  sheet.append(stage);

  const frames: [HTMLIFrameElement, BridgeHost, WidgetManifest][] = [];
  for (const { manifest: m } of widgets) {
    const [cols, rows] = sizeFor(m);
    const w = cols * CELL + (cols - 1) * GAP;
    const h = rows * CELL + (rows - 1) * GAP;
    const cell = document.createElement("div");
    cell.className = "cell";
    cell.style.width = `${w}px`;
    cell.style.height = `${h}px`;
    const tag = document.createElement("div");
    tag.className = "tag";
    tag.textContent = `${m.id} ${cols}x${rows}`;
    const frame = document.createElement("iframe");
    frame.title = m.name;
    frame.setAttribute("sandbox", "allow-scripts allow-same-origin");
    frame.style.width = `${w}px`;
    frame.style.height = `${h}px`;
    cell.append(tag, frame);
    wall.append(cell);
    const size: SizeInfo = { cells: [cols, rows], px: [w, h], dpi: 96 };
    const settings: Settings = withDefaults(m.settings ?? [], perWidget[m.id] ?? {});
    const host = new BridgeHost(
      `${m.id}-1`,
      m,
      backend,
      (msg: HostOutput) => frame.contentWindow?.postMessage(msg, "*"),
      { settings, size, look },
    );
    host.speed = num("speed", 1);
    frames.push([frame, host, m]);
  }
  for (const [frame, host, m] of frames) {
    if (frame.contentWindow) hosts.set(frame.contentWindow, host);
    frame.src = `/__widget/${m.id}/${m.entry}`;
  }
  document.title = "mypaper widget wall";
  window.setTimeout(() => document.body.setAttribute("data-ready", ""), 0);
}

void main().catch((err: unknown) => {
  sheet.textContent = err instanceof MypaperError ? err.message : String(err);
});
