// The top-bar modules (docs/bars.md, "Modules"). Each factory builds its element once and keeps it
// current through host events; clicks become host commands.

import { barAnchor } from "../anchor";
import { clockText } from "../clock-format";
import { button, glyph, h, viewport } from "../dom";
import {
  languageModule,
  notificationsModule,
  openNotifications,
  openLauncher,
  openMixer,
  openNetwork,
  openQuick,
  trayModule,
} from "../flyouts";
import { placePopover, type Rect } from "../geometry";
import { fire, request, type Scope } from "../host";
import { icons } from "../icons";
import { openMenu, type MenuEntry } from "../menu";
import { mountLayer, type Layer } from "../overlay";
import type { BarKind, BarsConfig, Module } from "../protocol";
import { mediaLabel, signalLevel, volumeDelta, volumeGlyph, workspaceSteps } from "./logic";

export interface TopCtx {
  config: BarsConfig;
  scope: Scope;
  /** The bar the module sits in. */
  kind: BarKind;
}
type Factory = (ctx: TopCtx) => HTMLElement;

/**
 * Where a popover from `el` starts: across the whole band (strip, island or taskbar), not just
 * the module. Open it on `barAnchor(el).side`: below a top bar, above a bottom taskbar.
 */
export const underStrip = (el: Element, _config?: BarsConfig): Rect => barAnchor(el).rect;

/**
 * Windows 11 opens quick settings from the network, volume and battery group: the `fluent` family
 * (the bar's look.family, on <html>) does too. A `modules.<name>.click` binding still wins.
 */
const groupedQuick = (): boolean => document.documentElement.dataset.family === "fluent";

const start: Factory = () => {
  const el = button("mod icon-only", glyph(icons.mark));
  el.title = "Start";
  el.addEventListener("click", () => openLauncher(el));
  return el;
};

/** A search box: Windows Search (Win+S). Narrow bars show only the glyph (style.css). */
const search: Factory = () => {
  const el = button("mod search", glyph(icons.search), h("span", "search-label", "Search"));
  el.title = "Search";
  el.addEventListener("click", () => fire("search"));
  return el;
};

const workspaces: Factory = ({ scope }) => {
  const el = h("div", "mod ws");
  let current = 0;
  const dots: HTMLButtonElement[] = [];
  scope.on("workspaces", (state) => {
    current = state.current;
    const count = Math.max(1, Math.min(state.count, 16));
    while (dots.length < count) {
      const index = dots.length;
      const dot = button("ws-dot");
      dot.addEventListener("click", () => {
        const step = workspaceSteps(current, index);
        if (step) for (let i = 0; i < step.count; i++) fire("workspace", { direction: step.direction });
      });
      dots.push(dot);
    }
    dots.length = count;
    el.replaceChildren(...dots);
    dots.forEach((dot, i) => {
      dot.classList.toggle("is-current", i === current);
      dot.setAttribute("aria-label", `Desktop ${i + 1}`);
      dot.setAttribute("aria-current", String(i === current));
    });
  });
  return el;
};

const app: Factory = ({ scope }) => {
  const el = h("div", "mod app");
  const icon = h("img", "app-icon");
  icon.alt = "";
  icon.hidden = true;
  const name = h("span", "app-name", "Desktop");
  el.append(icon, name);
  scope.on("foreground", (fg) => {
    name.textContent = fg?.name ?? "Desktop";
    icon.hidden = !fg?.icon;
    if (fg?.icon) icon.src = fg.icon;
    else icon.removeAttribute("src");
  });
  return el;
};

const clock: Factory = ({ config, scope, kind }) => {
  const el = button("mod clock");
  const options = kind === "dock" ? config.dock.clock : config.top.clock;
  // As on Windows 11 and macOS, the clock opens the notification center (with the month under it).
  el.addEventListener("click", () => openNotifications(el));
  scope.on("clock", ({ now, system }) => {
    const date = new Date(now);
    el.textContent = clockText(options, date, system);
    el.title = date.toLocaleDateString(undefined, { dateStyle: "full" });
  });
  return el;
};

const media: Factory = ({ scope }) => {
  const state = glyph("", "small");
  const label = h("span", "media-label");
  const el = button("mod media", state, label);
  let lastWheel = 0;
  el.addEventListener("click", () => fire("media", { action: "toggle" }));
  el.addEventListener(
    "wheel",
    (event) => {
      const now = performance.now();
      if (now - lastWheel < 300 || event.deltaY === 0) return;
      lastWheel = now;
      fire("media", { action: event.deltaY > 0 ? "next" : "previous" });
    },
    { passive: true },
  );
  scope.on("media", (info) => {
    el.hidden = !info;
    if (!info) return;
    state.innerHTML = info.playing ? icons.pause : icons.play;
    label.textContent = mediaLabel(info);
    el.title = `${mediaLabel(info)}${info.app ? ` (${info.app})` : ""}`;
  });
  return el;
};

const network: Factory = ({ scope }) => {
  const g = glyph();
  const el = button("mod icon-only", g);
  el.addEventListener("click", () => (groupedQuick() ? openQuick(el) : openNetwork(el)));
  scope.on("network", (info) => {
    g.innerHTML =
      info.kind === "wifi" ? icons.wifi(signalLevel(info.signal)) : info.kind === "ethernet" ? icons.ethernet : icons.offline;
    el.title =
      info.kind === "none" ? "Not connected" : (info.name ?? (info.kind === "wifi" ? "Wi-Fi" : "Ethernet"));
  });
  return el;
};

const volume: Factory = ({ scope }) => {
  const g = glyph();
  const el = button("mod icon-only", g);
  let state: { level: number; muted: boolean } | null = null;
  let pending = 0;
  let inflight = false;
  let hud: { layer: Layer; fill: HTMLElement; text: HTMLElement; timer: number } | undefined;

  const render = () => {
    el.hidden = !state;
    if (!state) return;
    g.innerHTML = icons.volume(volumeGlyph(state));
    el.title = state.muted ? "Muted" : `Volume ${Math.round(state.level * 100)}%`;
  };

  const hideHud = () => {
    if (!hud) return;
    clearTimeout(hud.timer);
    hud.layer.remove();
    hud = undefined;
  };
  // A thin level bar under the module while scrolling.
  const showHud = () => {
    if (!state) return;
    if (!hud) {
      const fill = h("div", "hud-fill");
      const text = h("span", "hud-num");
      const el2 = h("div", "hud", h("div", "hud-track", fill), text);
      const anchor = barAnchor(el);
      const layer = mountLayer(el2, (size) => placePopover(anchor.rect, size, viewport(), anchor.side, "center", 4));
      hud = { layer, fill, text, timer: 0 };
    }
    const shown = state.muted ? 0 : state.level;
    hud.fill.style.width = `${Math.round(shown * 100)}%`;
    hud.text.textContent = `${Math.round(shown * 100)}%`;
    clearTimeout(hud.timer);
    hud.timer = window.setTimeout(hideHud, 1200);
  };

  // Wheel steps accumulate while a request is in flight, then go out as one.
  const flush = () => {
    if (inflight || pending === 0) return;
    const delta = pending;
    pending = 0;
    inflight = true;
    request("volume", { delta })
      .then((result) => {
        state = result;
        render();
        showHud();
      })
      .catch((error: unknown) => console.warn("[bars] volume failed", error))
      .finally(() => {
        inflight = false;
        flush();
      });
  };

  // Click: the mixer; middle click: mute.
  el.addEventListener("click", () => (groupedQuick() ? openQuick(el) : openMixer(el)));
  el.addEventListener("auxclick", (event) => {
    if (event.button !== 1 || !state) return;
    request("volume", { mute: !state.muted })
      .then((result) => {
        state = result;
        render();
      })
      .catch((error: unknown) => console.warn("[bars] volume failed", error));
  });
  el.addEventListener(
    "wheel",
    (event) => {
      pending += volumeDelta(event.deltaY);
      flush();
    },
    { passive: true },
  );
  scope.on("volume", (v) => {
    state = v;
    render();
  });
  scope.add(hideHud);
  return el;
};

const battery: Factory = ({ scope }) => {
  const bolt = glyph(icons.bolt, "bolt");
  const g = glyph();
  const text = h("span", "battery-pct");
  const el = button("mod battery", bolt, g, text);
  el.addEventListener("click", () => (groupedQuick() ? openQuick(el) : fire("openSettings", { uri: "ms-settings:batterysaver" })));
  scope.on("battery", (info) => {
    el.hidden = !info;
    if (!info) return;
    bolt.hidden = !info.charging;
    g.innerHTML = icons.battery(info.percent / 100);
    text.textContent = `${Math.round(info.percent)}%`;
    el.title = info.charging ? "Charging" : "On battery";
  });
  return el;
};

const quick: Factory = () => {
  const el = button("mod icon-only", glyph(icons.quick));
  el.title = "Quick settings";
  el.addEventListener("click", () => openQuick(el));
  return el;
};

const confirmPower = (action: "restart" | "shutdown"): MenuEntry[] => {
  const restart = action === "restart";
  return [
    { kind: "header", label: restart ? "Restart your PC?" : "Shut down your PC?" },
    { kind: "item", label: "Cancel", run: () => undefined },
    {
      kind: "item",
      label: restart ? "Restart now" : "Shut down now",
      primary: true,
      run: () => fire("power", { action }),
    },
  ];
};

const power: Factory = () => {
  const el = button("mod icon-only", glyph(icons.power));
  el.title = "Power";
  const entries: MenuEntry[] = [
    { kind: "item", label: "Lock", run: () => fire("power", { action: "lock" }) },
    { kind: "item", label: "Sleep", run: () => fire("power", { action: "sleep" }) },
    { kind: "item", label: "Sign out", run: () => fire("power", { action: "signout" }) },
    { kind: "separator" },
    { kind: "item", label: "Restart…", run: () => confirmPower("restart") },
    { kind: "item", label: "Shut down…", run: () => confirmPower("shutdown") },
  ];
  el.addEventListener("click", () => {
    const anchor = barAnchor(el);
    openMenu({
      anchor: el,
      entries,
      origin: anchor.origin,
      place: (size) => placePopover(anchor.rect, size, viewport(), anchor.side, "end", 12),
    });
  });
  return el;
};

/** A ring that fills clockwise; `--p` is the percent. */
const gauge = (): { el: HTMLElement; set(percent: number): void } => {
  const el = h("span", "gauge");
  el.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true"><circle class="gauge-track" cx="8" cy="8" r="5.5"/><circle class="gauge-arc" cx="8" cy="8" r="5.5" pathLength="100" transform="rotate(-90 8 8)"/></svg>`;
  return { el, set: (percent) => el.style.setProperty("--p", String(Math.max(0, Math.min(100, percent)))) };
};

const stats: Factory = ({ scope }) => {
  const cpu = gauge();
  const ram = gauge();
  const el = button("mod stats", cpu.el, ram.el);
  el.addEventListener("click", () => fire("launch", { path: "taskmgr.exe" }));
  scope.on("stats", ({ cpu: c, memory }) => {
    const mem = memory.total > 0 ? (memory.used / memory.total) * 100 : 0;
    cpu.set(c);
    ram.set(mem);
    el.title = `CPU ${Math.round(c)}%  ·  Memory ${Math.round(mem)}%`;
  });
  return el;
};

/** Typed by `Module`, so adding a module to protocol.ts fails the build until it has a factory. */
export const factories: Record<Module, Factory> = {
  start,
  search,
  workspaces,
  app,
  clock,
  media,
  tray: trayModule,
  network,
  volume,
  battery,
  quick,
  power,
  stats,
  language: languageModule,
  notifications: notificationsModule,
};
