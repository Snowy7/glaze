// One sandboxed iframe per widget instance, and the message relay between the iframes and the host.
// Reconciles by instance id: an iframe is only recreated when its widget id changes, and never
// reloaded while it stays (except on the host's `reload`).

import { h } from "./dom";
import { rectToPx, sizeToPx, type PxRect, type Rect } from "./grid";
import { denial } from "./permissions";
import type {
  HelloResult,
  HostToShell,
  InstanceState,
  ShellState,
  ShellToHost,
  SizeInfo,
  WidgetManifest,
  WireRequest,
} from "./protocol";

export interface Slot {
  instance: string;
  widget: string;
  manifest: WidgetManifest;
  el: HTMLDivElement; // positioned wrapper: the iframe plus any edit-mode chrome
  iframe: HTMLIFrameElement;
  origin: string; // https://<widget>.widget.mypaper
  sizeKey: string; // JSON of the last size the widget was told about
  settingsKey: string; // JSON of the last settings the widget was told about
}

export interface Frames {
  readonly slots: ReadonlyMap<string, Slot>;
  /** Cells to show instead of the host's (optimistic edits, drag previews). */
  readonly overrides: Map<string, Rect>;
  /** Exact px rects to show instead of any cell (the widget being dragged). */
  readonly live: Map<string, PxRect>;
  sync(state: ShellState): void;
  /** Applies positions from the host state, `overrides` and `live`. */
  render(): void;
  onHost(message: Exclude<HostToShell, { type: "state" }>): void;
}

interface Options {
  layer: HTMLElement;
  send: (message: ShellToHost) => void;
  onCreate: (slot: Slot) => void;
}

const originOf = (widget: string) => `https://${widget}.widget.mypaper`;

// A manifest's entry is a relative path; never let it leave the widget's origin.
const urlOf = (manifest: WidgetManifest) =>
  `${originOf(manifest.id)}/${(manifest.entry ?? "index.html").replace(/^[/\\]+/, "")}`;

const iframeName = (instance: string, manifest: WidgetManifest) => JSON.stringify({ instance, widget: manifest });

function isRequest(data: unknown): data is WireRequest {
  return (
    typeof data === "object" &&
    data !== null &&
    "mypaper" in data &&
    data.mypaper === 1 &&
    "id" in data &&
    (typeof data.id === "number" || typeof data.id === "string") &&
    "method" in data &&
    typeof data.method === "string"
  );
}

export function createFrames({ layer, send, onCreate }: Options): Frames {
  const slots = new Map<string, Slot>();
  const overrides = new Map<string, Rect>();
  const live = new Map<string, PxRect>();
  let state: ShellState | undefined;
  let byInstance = new Map<string, InstanceState>();

  function sizeInfo(inst: InstanceState): SizeInfo {
    if (!state) throw new Error("no state yet");
    return { cells: inst.size, px: sizeToPx(state.grid, inst.size), dpi: state.monitor.dpi };
  }

  function postToFrame(slot: Slot, message: object) {
    slot.iframe.contentWindow?.postMessage({ mypaper: 1, ...message }, slot.origin);
  }

  const postEvent = (slot: Slot, event: string, data: unknown) => postToFrame(slot, { event, data });

  function broadcast(event: string, data: unknown) {
    for (const slot of slots.values()) postEvent(slot, event, data);
  }

  function create(inst: InstanceState, manifest: WidgetManifest): Slot {
    const iframe = h("iframe");
    iframe.name = iframeName(inst.instance, manifest);
    iframe.setAttribute("sandbox", "allow-scripts allow-same-origin");
    if (manifest.permissions?.includes("clipboard.write")) iframe.allow = "clipboard-write";
    iframe.title = manifest.name;
    iframe.src = urlOf(manifest);

    const el = h("div", "slot fresh", iframe);
    el.dataset.instance = inst.instance;
    const slot: Slot = {
      instance: inst.instance,
      widget: inst.widget,
      manifest,
      el,
      iframe,
      origin: originOf(inst.widget),
      sizeKey: JSON.stringify(sizeInfo(inst)),
      settingsKey: JSON.stringify(inst.settings),
    };
    slots.set(inst.instance, slot);
    layer.append(el);
    onCreate(slot);
    return slot;
  }

  function remove(slot: Slot) {
    slot.el.remove();
    slots.delete(slot.instance);
    overrides.delete(slot.instance);
    live.delete(slot.instance);
  }

  function render() {
    if (!state) return;
    for (const slot of slots.values()) {
      const inst = byInstance.get(slot.instance);
      if (!inst) continue;
      const rect = live.get(slot.instance) ?? rectToPx(state.grid, overrides.get(slot.instance) ?? inst);
      slot.el.style.transform = `translate(${rect.x}px, ${rect.y}px)`;
      slot.el.style.width = `${rect.width}px`;
      slot.el.style.height = `${rect.height}px`;
    }
  }

  function sync(next: ShellState) {
    const prev = state;
    state = next;
    byInstance = new Map(next.instances.map((i) => [i.instance, i]));
    const manifests = new Map(next.widgets.map((w) => [w.id, w]));

    const seen = new Set<string>();
    const fresh: Slot[] = [];
    for (const inst of next.instances) {
      const manifest = manifests.get(inst.widget);
      if (!manifest) continue; // not installed: kept in widgets.json, not shown
      seen.add(inst.instance);
      let slot = slots.get(inst.instance);
      if (slot && slot.widget !== inst.widget) {
        remove(slot);
        slot = undefined;
      }
      if (slot) slot.manifest = manifest;
      else fresh.push(create(inst, manifest));
    }
    for (const slot of [...slots.values()]) if (!seen.has(slot.instance)) remove(slot);

    render();
    // Let new slots appear in place instead of gliding in from the corner.
    if (fresh.length > 0) {
      requestAnimationFrame(() => requestAnimationFrame(() => fresh.forEach((s) => s.el.classList.remove("fresh"))));
    }

    if (prev) {
      if (prev.editing !== next.editing) broadcast("editmode", next.editing);
      if (prev.visible !== next.visible) broadcast("visibility", next.visible);
    }
    for (const slot of slots.values()) {
      const inst = byInstance.get(slot.instance);
      if (!inst) continue;
      const info = sizeInfo(inst);
      const sizeKey = JSON.stringify(info);
      if (sizeKey !== slot.sizeKey) {
        slot.sizeKey = sizeKey;
        postEvent(slot, "size", info);
      }
      const settingsKey = JSON.stringify(inst.settings);
      if (settingsKey !== slot.settingsKey) {
        slot.settingsKey = settingsKey;
        postEvent(slot, "settings", inst.settings);
      }
    }
  }

  // iframe → shell → host
  window.addEventListener("message", (e: MessageEvent<unknown>) => {
    if (!state) return;
    const slot = [...slots.values()].find((s) => s.iframe.contentWindow === e.source && s.origin === e.origin);
    if (!slot || !isRequest(e.data)) return;
    const { id, method, params } = e.data;

    // The handshake is answered from the state; the host only learns that the frame (re)loaded.
    if (method === "hello") {
      const inst = byInstance.get(slot.instance);
      if (!inst) return;
      const result: HelloResult = {
        instance: slot.instance,
        widget: slot.manifest,
        settings: inst.settings,
        size: sizeInfo(inst),
        visible: state.visible,
        editing: state.editing,
      };
      postToFrame(slot, { id, result });
      send({ type: "hello", instance: slot.instance });
      return;
    }

    const message = denial(slot.manifest, method, params);
    if (message !== undefined) {
      postToFrame(slot, { id, error: { code: "PermissionDenied", message } });
      return;
    }

    // The instance id comes from the slot, never from the widget.
    send({ type: "call", instance: slot.instance, id, method, ...(params !== undefined && { params }) });
  });

  function onHost(message: Exclude<HostToShell, { type: "state" }>) {
    if (message.type === "reload") {
      for (const slot of slots.values()) {
        if (slot.widget !== message.widget) continue;
        const manifest = state?.widgets.find((w) => w.id === slot.widget);
        if (manifest) {
          slot.manifest = manifest;
          slot.iframe.name = iframeName(slot.instance, manifest);
          slot.iframe.src = urlOf(manifest);
        } else {
          slot.iframe.src = slot.iframe.src; // re-assigning src navigates again
        }
      }
      return;
    }

    const slot = slots.get(message.instance);
    if (!slot) return;
    if (message.type === "event") {
      postEvent(slot, message.event, message.data);
    } else if (message.error) {
      postToFrame(slot, { id: message.id, error: message.error });
    } else {
      postToFrame(slot, { id: message.id, result: message.result ?? null });
    }
  }

  return { slots, overrides, live, sync, render, onHost };
}
