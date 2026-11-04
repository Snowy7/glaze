// Dev only (the mock host): `bars.json` the way the real host resolves it, from the same preset
// bundles (crates/bars/presets), so presets can be previewed in a browser. The host's own rules
// live in crates/bars/src/config.rs; this mirrors only what the mock needs.

import floating from "../../../crates/bars/presets/floating.json";
import islands from "../../../crates/bars/presets/islands.json";
import macos from "../../../crates/bars/presets/macos.json";
import monochrome from "../../../crates/bars/presets/monochrome.json";
import neonGlass from "../../../crates/bars/presets/neon-glass.json";
import windows11 from "../../../crates/bars/presets/windows11.json";
import type { BarsConfig, Preset } from "./protocol";

export const PRESETS: Record<Preset, unknown> = {
  monochrome,
  macos,
  windows11,
  islands,
  "neon-glass": neonGlass,
  floating,
};

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Objects merge key by key; anything else (arrays included) is replaced. */
export function deepMerge(base: unknown, over: unknown): unknown {
  if (!isObject(base) || !isObject(over)) return over === undefined ? base : over;
  const out: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(over)) out[key] = deepMerge(base[key], value);
  return out;
}

function defaults(): BarsConfig {
  return {
    version: 1,
    preset: "monochrome",
    taskbar: { mode: "keep" },
    top: {
      enabled: true,
      monitors: "all",
      layout: "strip",
      position: "top",
      height: 32,
      margin: Number.NaN,
      padding: Number.NaN,
      spacing: Number.NaN,
      width: "full",
      left: ["start", "workspaces", "app"],
      center: ["clock"],
      right: ["media", "tray", "network", "volume", "battery", "quick", "power"],
      clock: { seconds: false, format: "ddd d MMM  HH:mm" },
    },
    dock: {
      enabled: false,
      monitors: "primary",
      style: "floating",
      position: "bottom",
      size: Number.NaN,
      margin: Number.NaN,
      padding: Number.NaN,
      spacing: Number.NaN,
      autohide: false,
      magnify: { enabled: true, scale: 1.5, range: 3 },
      running: true,
      split: false,
      counter: true,
      titles: false,
      indicator: "dot",
      preview: { enabled: true, delay: 450 },
      bounce: true,
      attention: "pulse",
      extras: [],
      align: "center",
      left: ["start", "search"],
      right: ["tray", "network", "volume", "battery", "clock"],
      pinned: [],
    },
    look: { border: {}, glow: {}, shadow: {}, items: {}, indicator: {} },
    animation: {},
    modules: {},
    performance: "auto",
  };
}

/** `magnify: true/false` is shorthand for the object, as in the host. */
function normalize(raw: unknown): unknown {
  if (!isObject(raw) || !isObject(raw.dock)) return raw;
  const dock = raw.dock;
  if (typeof dock.magnify === "boolean") dock.magnify = { enabled: dock.magnify, scale: 1.5, range: 3 };
  if (typeof dock.preview === "boolean") dock.preview = { enabled: dock.preview, delay: 450 };
  return raw;
}

/** The config the host would send for `preset` with `over` (a partial bars.json) on top. */
export function mockConfig(preset: Preset, over: Json = {}): BarsConfig {
  const merged = deepMerge(deepMerge(defaults(), normalize(structuredClone(PRESETS[preset]))), normalize(over));
  // The mock builds it from typed defaults and the preset files, which the host's tests check.
  const config = merged as BarsConfig;
  config.preset = preset;
  const pick = (value: number, fallback: number) => (Number.isNaN(value) ? fallback : value);
  const { top, dock } = config;
  top.margin = pick(top.margin, top.layout === "strip" ? 0 : top.layout === "floating" ? 8 : 6);
  top.padding = pick(top.padding, top.layout === "strip" ? 5 : top.layout === "floating" ? 6 : 4);
  top.spacing = pick(top.spacing, top.layout === "strip" ? 1 : 2);
  const floatingDock = dock.style === "floating";
  dock.size = pick(dock.size, floatingDock ? 48 : 24);
  dock.margin = pick(dock.margin, floatingDock ? 6 : 0);
  dock.padding = pick(dock.padding, floatingDock ? 5 : 12);
  dock.spacing = pick(dock.spacing, floatingDock ? 6 : 4);
  return config;
}
