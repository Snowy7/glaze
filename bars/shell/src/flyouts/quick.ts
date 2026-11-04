// Quick settings (docs/bars.md, "Flyouts" → "Quick settings"): Wi-Fi and Bluetooth, power mode,
// battery saver, do not disturb and focus (shown; Settings changes them: there's no documented
// way to), night light (its settings page), brightness and volume.

import { button, glyph, h } from "../dom";
import { defineFlyout } from "../flyout";
import { fire, request } from "../host";
import { glyphs } from "./glyphs";
import type { PowerMode, QuickState } from "./protocol";
import { show } from "./surface";
import { slider, tile, type TileOptions } from "./ui";
import { openMixer, openNetwork } from ".";

const POWER: Record<PowerMode, { label: string; next: PowerMode; icon: string }> = {
  efficiency: { label: "Best efficiency", next: "balanced", icon: glyphs.leaf },
  balanced: { label: "Balanced", next: "performance", icon: glyphs.gauge },
  performance: { label: "Best performance", next: "efficiency", icon: glyphs.gauge },
};

export async function open(anchor: HTMLElement, keyboard = false): Promise<void> {
  await show(quick, null, { anchor, align: "end", extent: 460, keyboard });
}

const quick = defineFlyout<null>("quick", (_props, ctx) => {
  const tiles = h("div", "qs-tiles");
  const sliders = h("div", "qs-sliders");
  const battery = h("span", "qs-battery");
  const settings = button("fly-icon-button", glyph(glyphs.gear));
  settings.title = "All settings";
  settings.addEventListener("click", () => {
    fire("openSettings", { uri: "ms-settings:" });
    ctx.close();
  });
  const root = h("div", "fly-panel qs", tiles, sliders, h("div", "fly-footer", battery, h("span", "fly-gap"), settings));
  const anchor = () => document.querySelector<HTMLElement>('[data-module="quick"]') ?? root;

  let state: QuickState | undefined;
  let wifiName: string | undefined;

  const settingsTile = (options: Omit<TileOptions, "onClick"> & { uri: string }) =>
    tile({ ...options, onClick: () => fire("openSettings", { uri: options.uri }) });

  const wifi = tile({
    icon: glyphs.wifi,
    label: "Wi-Fi",
    on: false,
    onClick: () => toggleRadio("wifi"),
    onMore: () => openNetwork(anchor()),
  });
  const bluetooth = tile({ icon: glyphs.bluetooth, label: "Bluetooth", on: false, onClick: () => toggleRadio("bluetooth") });
  const power = tile({
    icon: glyphs.gauge,
    label: "Power mode",
    on: false,
    onClick: () => {
      const mode = state?.power;
      if (!mode) return fire("openSettings", { uri: "ms-settings:powersleep" });
      request("setPowerMode", { mode: POWER[mode].next })
        .then(apply)
        .catch((error: unknown) => console.warn("[bars] setPowerMode failed", error));
    },
  });
  const saver = settingsTile({ icon: glyphs.saver, label: "Battery saver", on: false, uri: "ms-settings:batterysaver" });
  const dnd = settingsTile({ icon: glyphs.bellOff, label: "Do not disturb", on: false, uri: "ms-settings:notifications" });
  const focus = settingsTile({ icon: glyphs.focus, label: "Focus", on: false, uri: "ms-settings:quiethours" });
  const night = settingsTile({ icon: glyphs.nightlight, label: "Night light", detail: "Settings", on: false, uri: "ms-settings:nightlight" });
  tiles.append(wifi.el, bluetooth.el, power.el, saver.el, dnd.el, focus.el, night.el);

  const brightness = slider({
    value: 0,
    label: "Brightness",
    icon: glyph(glyphs.sun, "qs-slider-glyph"),
    onInput: (level) => fire("setBrightness", { level: Math.round(level * 100) }),
  });
  brightness.el.hidden = true;
  const muteButton = button("fly-icon-button qs-mute", glyph(glyphs.speaker));
  muteButton.title = "Mute";
  let muted = false;
  muteButton.addEventListener("click", () => {
    request("volume", { mute: !muted })
      .then(applyVolume)
      .catch((error: unknown) => console.warn("[bars] volume failed", error));
  });
  const volume = slider({ value: 0, label: "Volume", onInput: (level) => fire("volume", { set: level }) });
  const mixer = button("fly-icon-button", glyph(glyphs.chevronRight));
  mixer.title = "Volume mixer";
  mixer.addEventListener("click", () => openMixer(anchor()));
  const volumeRow = h("div", "qs-volume", muteButton, volume.el, mixer);
  volumeRow.hidden = true;
  sliders.append(brightness.el, volumeRow);

  const applyVolume = (v: { level: number; muted: boolean } | null) => {
    volumeRow.hidden = !v;
    if (!v) return;
    muted = v.muted;
    volume.set(v.muted ? 0 : v.level);
    muteButton.replaceChildren(glyph(v.muted || v.level === 0 ? glyphs.speakerMute : glyphs.speaker));
    muteButton.title = v.muted ? "Unmute" : "Mute";
  };

  const apply = (next: QuickState) => {
    state = next;
    const radio = (kind: "wifi" | "bluetooth") => next.radios.find((r) => r.kind === kind);
    const w = radio("wifi");
    wifi.update({ on: w?.on ?? false, disabled: !w, detail: !w ? "Unavailable" : w.on ? (wifiName ?? "On") : "Off" });
    const b = radio("bluetooth");
    bluetooth.update({ on: b?.on ?? false, disabled: !b, detail: !b ? "Unavailable" : b.on ? "On" : "Off" });
    if (next.power) {
      const p = POWER[next.power];
      power.update({ on: next.power !== "balanced", icon: p.icon, detail: p.label });
    } else power.update({ on: false, detail: "Settings" });
    saver.el.hidden = next.saver === undefined;
    saver.update({ on: next.saver ?? false, detail: next.saver ? "On" : "Off" });
    dnd.el.hidden = next.dnd === undefined;
    dnd.update({ on: next.dnd !== undefined && next.dnd !== "off", detail: { off: "Off", priority: "Priority only", alarms: "Alarms only" }[next.dnd ?? "off"] });
    focus.el.hidden = next.focus === undefined;
    focus.update({ on: next.focus ?? false, detail: next.focus ? "In a session" : "Off" });
    brightness.el.hidden = next.brightness === undefined;
    if (next.brightness !== undefined) brightness.set(next.brightness / 100);
    ctx.reposition();
  };

  const toggleRadio = (kind: "wifi" | "bluetooth") => {
    const radio = state?.radios.find((r) => r.kind === kind);
    if (!radio) return fire("openSettings", { uri: kind === "wifi" ? "ms-settings:network-wifi" : "ms-settings:bluetooth" });
    request("setRadio", { kind, on: !radio.on })
      .then(apply)
      .catch((error: unknown) => {
        console.warn("[bars] setRadio failed", error);
        fire("openSettings", { uri: kind === "wifi" ? "ms-settings:network-wifi" : "ms-settings:bluetooth" });
      });
  };

  request("quick")
    .then(apply)
    .catch((error: unknown) => console.warn("[bars] quick failed", error));
  ctx.scope.on("volume", applyVolume);
  ctx.scope.on("network", (info) => {
    wifiName = info.kind === "wifi" ? info.name : undefined;
    if (state) apply(state);
  });
  ctx.scope.on("battery", (info) => {
    battery.hidden = !info;
    battery.textContent = info ? `${Math.round(info.percent)}%${info.charging ? " · Charging" : ""}` : "";
  });
  for (const topic of ["volume", "network", "battery"] as const) {
    fire("watch", { topic });
    ctx.scope.add(() => fire("unwatch", { topic }));
  }
  return root;
});
