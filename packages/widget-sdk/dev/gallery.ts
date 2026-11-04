// A contact sheet of one widget: every surface (rows) at several sizes (columns), over one or more
// backgrounds, each cell a live iframe with its own mock host. One headless screenshot of this page
// shows everything a reviewer needs.
//   widget=<id>  sizes=min,default,max | 2x1,3x2,...  surfaces=none,glass,solid
//   styles=minimal,depth,...|all  rows are style x surface (default: the style param, or minimal)
//   bgs=photo-dark,photo-light  text=auto|light|dark  scale=<n>  cell=<px> gap=<px>
//   settings=<json>  storage=<json>  media=playing|paused|none  battery=none|discharging|charging
//   net=wifi|ethernet|none  appearance=<json>  themecss=<css>
//   speed=<n>  makes system.subscribe tick n times faster, so sparklines fill up within a screenshot

import { MypaperError, WIDGET_STYLES, normalizeAppearance } from "../src";
import type { AppearanceState, Settings, SizeInfo, WidgetManifest } from "../src/types";
import { FakeBackend } from "./fake";
import { BridgeHost, type HostOutput } from "./host";
import { lookFromParams } from "./look";
import { mockPlugin, type FolderMocks } from "./mock-plugins";
import { withDefaults } from "./settings-form";

const q = new URLSearchParams(location.search);
const num = (key: string, fallback: number) => {
  const n = Number(q.get(key));
  return q.has(key) && Number.isFinite(n) && n > 0 ? n : fallback;
};
const list = (key: string, fallback: string) => (q.get(key) ?? fallback).split(",").map((s) => s.trim()).filter(Boolean);
const json = (key: string): Record<string, unknown> => {
  try {
    const parsed: unknown = JSON.parse(q.get(key) ?? "{}");
    return typeof parsed === "object" && parsed !== null ? { ...parsed } : {};
  } catch {
    return {};
  }
};

const CELL = num("cell", 96);
const GAP = num("gap", 8);

const backend = new FakeBackend();
const battery = q.get("battery");
backend.hasBattery = battery !== "none";
backend.charging = battery === "charging";
const net = q.get("net");
if (net === "wifi" || net === "ethernet" || net === "none") backend.netKind = net;
const media = q.get("media");
if (media === "none" || media === "paused" || media === "playing") backend.setMedia(media);

const sheet = document.getElementById("sheet") as HTMLElement;
const hosts = new Map<Window, BridgeHost>();

window.addEventListener("message", (e) => {
  if (e.source) void hosts.get(e.source as Window)?.handle(e.data);
});

async function main() {
  const widgets: { id: string; manifest: WidgetManifest }[] = await fetch("/__api/widgets").then((r) => r.json());
  const id = q.get("widget") ?? widgets[0]?.id ?? "";
  const info = widgets.find((w) => w.id === id);
  if (!info) {
    sheet.textContent = `No widget "${id}"`;
    return;
  }
  const m = info.manifest;
  let mocks: FolderMocks = {};
  try {
    const res = await fetch(`/__widget/${id}/mypaper.mock.json`);
    if (res.ok) mocks = (await res.json()) as FolderMocks;
  } catch {
    /* none */
  }
  backend.pluginCall = (pid, method, params) => mockPlugin(pid, method, params, mocks);
  backend.onLaunch = () => {};

  const sizes = list("sizes", "min,default,max").map((s): [number, number] => {
    if (s === "min" || s === "default" || s === "max") return [...m.size[s]] as [number, number];
    const [c = 1, r = 1] = s.split("x").map(Number);
    return [Math.min(m.size.max[0], Math.max(m.size.min[0], c)), Math.min(m.size.max[1], Math.max(m.size.min[1], r))];
  });
  const unique = sizes.filter((s, i) => sizes.findIndex((t) => t[0] === s[0] && t[1] === s[1]) === i);
  const surfaces = list("surfaces", "none,glass,solid");
  const base = lookFromParams(q);
  const styleParam = q.get("styles");
  const styles = styleParam === "all" ? [...WIDGET_STYLES] : styleParam ? list("styles", "") : [base.appearance.style];
  const settings: Settings = withDefaults(m.settings ?? [], json("settings"));

  for (const bg of list("bgs", "photo-dark,photo-light")) {
    const stage = document.createElement("div");
    stage.className = `stage bg-${bg}`;
    stage.dataset["bg"] = bg;
    const head = document.createElement("div");
    head.className = "head";
    head.textContent = `${m.id} / ${bg}`;
    stage.append(head);
    const textParam = q.get("text") ?? "auto";
    const text = textParam === "light" || textParam === "dark" ? textParam : bg.includes("light") ? "dark" : "light";

    for (const [style, surface] of styles.flatMap((st) => surfaces.map((su): [string, string] => [st, su]))) {
      const line = document.createElement("div");
      line.className = "rowline";
      for (const [cols, rows] of unique) {
        const w = cols * CELL + (cols - 1) * GAP;
        const h = rows * CELL + (rows - 1) * GAP;
        const cell = document.createElement("div");
        cell.className = "cell";
        cell.style.width = `${w}px`;
        cell.style.height = `${h}px`;
        const tag = document.createElement("div");
        tag.className = "tag";
        tag.textContent = `${style} ${surface} ${cols}x${rows} ${text}`;
        const frame = document.createElement("iframe");
        frame.title = m.name;
        frame.setAttribute("sandbox", "allow-scripts allow-same-origin");
        frame.style.width = `${w}px`;
        frame.style.height = `${h}px`;
        cell.append(tag, frame);
        line.append(cell);

        const size: SizeInfo = { cells: [cols, rows], px: [w, h], dpi: 96 };
        const look: AppearanceState = {
          appearance: {
            ...base.appearance,
            style: normalizeAppearance({ style }).style,
            text: q.has("text") && q.get("text") !== "auto" ? base.appearance.text : text,
            surface: surface === "glass" || surface === "solid" ? surface : "none",
          },
          themeCss: base.themeCss,
        };
        const host = new BridgeHost(
          `${m.id}-1`,
          m,
          backend,
          (msg: HostOutput) => frame.contentWindow?.postMessage(msg, "*"),
          { settings, size, look },
        );
        host.speed = num("speed", 1);
        for (const [key, value] of Object.entries(json("storage"))) host.storage.set(key, value);
        stageHosts.push([frame, host]);
      }
      stage.append(line);
    }
    sheet.append(stage);
  }
  // The frames are in the document now: route their messages before they can say hello.
  for (const [frame, host] of stageHosts) {
    if (frame.contentWindow) hosts.set(frame.contentWindow, host);
    frame.src = `/__widget/${m.id}/${m.entry}`;
  }
  document.title = `${m.id} gallery`;
  // Headless screenshots wait for this.
  window.setTimeout(() => document.body.setAttribute("data-ready", ""), 0);
}

const stageHosts: [HTMLIFrameElement, BridgeHost][] = [];
void main().catch((err: unknown) => {
  sheet.textContent = err instanceof MypaperError ? err.message : String(err);
});
