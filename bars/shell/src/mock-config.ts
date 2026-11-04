// Dev only (the mock host): `bars.json` the way the real host resolves it, from the same bundles
// (crates/bars/presets: the pairings, and each bar's designs under top/ and dock/), so designs can
// be previewed in a browser. The host's own rules live in crates/bars/src/presets.rs (`layered`)
// and config.rs; this mirrors only what the mock needs.

import chromeos from "../../../crates/bars/presets/chromeos.json";
import floating from "../../../crates/bars/presets/floating.json";
import islands from "../../../crates/bars/presets/islands.json";
import macos from "../../../crates/bars/presets/macos.json";
import monochrome from "../../../crates/bars/presets/monochrome.json";
import neonGlass from "../../../crates/bars/presets/neon-glass.json";
import windows10 from "../../../crates/bars/presets/windows10.json";
import windows11 from "../../../crates/bars/presets/windows11.json";
import type { BarsConfig, DockDesign, Look, Preset, TopDesign } from "./protocol";

export const PRESETS: Record<Preset, unknown> = {
  monochrome,
  macos,
  windows11,
  windows10,
  chromeos,
  islands,
  "neon-glass": neonGlass,
  floating,
};

const bundles = (files: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(Object.entries(files).map(([path, json]) => [path.replace(/^.*\/(.+)\.json$/, "$1"), json]));
// Vite and vitest read every design bundle at build time.
export const TOP_BUNDLES = bundles(import.meta.glob("../../../crates/bars/presets/top/*.json", { eager: true, import: "default" })) as Record<TopDesign, unknown>;
export const DOCK_BUNDLES = bundles(import.meta.glob("../../../crates/bars/presets/dock/*.json", { eager: true, import: "default" })) as Record<DockDesign, unknown>;

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * Objects merge key by key; anything else (arrays included) is replaced. An object whose `type`
 * differs from the base's replaces it whole (a background switching kinds), as in the host.
 */
export function deepMerge(base: unknown, over: unknown): unknown {
  if (over === undefined) return base;
  if (!isObject(base) || !isObject(over)) return over;
  if ("type" in over && base.type !== over.type) return over;
  const out: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(over)) out[key] = deepMerge(base[key], value);
  return out;
}

const emptyLook = (): Look => ({ font: {}, glass: {}, flyouts: {}, border: {}, glow: {}, shadow: {}, items: {}, indicator: {} });

function defaults(): BarsConfig {
  return {
    version: 1,
    preset: "monochrome",
    taskbar: { mode: "keep" },
    top: {
      design: "monochrome",
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
      look: emptyLook(),
      animation: {},
    },
    dock: {
      design: "monochrome",
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
      clock: { seconds: false, format: "ddd d MMM  HH:mm" },
      pinned: [],
      look: emptyLook(),
      animation: {},
    },
    modules: {},
    performance: "auto",
    launcher: { winKey: true, layout: "win11", files: true },
    tray: { host: false },
    calendar: { zones: [] },
    audio: { switchOutput: false },
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

/** The file's shared `look` and `animation` under each bar's own, as `presets.rs` folds them. */
function foldShared(raw: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = { ...raw };
  for (const field of ["look", "animation"] as const) {
    if (raw[field] === undefined) continue;
    for (const bar of ["top", "dock"] as const) {
      const own = isObject(raw[bar]) ? raw[bar] : {};
      out[bar] = { ...(isObject(out[bar]) ? out[bar] : {}), [field]: deepMerge(raw[field], own[field]) };
    }
  }
  delete out.look;
  delete out.animation;
  return out;
}

/** The config the host would send for `preset` with `over` (a partial bars.json) on top. */
export function mockConfig(preset: Preset, over: Json = {}): BarsConfig {
  const raw = isObject(over) ? (normalize(structuredClone(over)) as Record<string, unknown>) : {};
  const pairing = normalize(structuredClone(PRESETS[preset])) as Record<string, { design?: string }>;
  const named = (bar: "top" | "dock") => {
    const own = raw[bar];
    return (isObject(own) && typeof own.design === "string" ? own.design : undefined) ?? pairing[bar]?.design ?? "monochrome";
  };
  const top = named("top") as TopDesign;
  const dock = named("dock") as DockDesign;
  const designs = { top: structuredClone(TOP_BUNDLES[top]), dock: normalize({ dock: structuredClone(DOCK_BUNDLES[dock]) }) };
  let merged = deepMerge(defaults(), { top: designs.top, dock: (designs.dock as { dock: unknown }).dock });
  merged = deepMerge(merged, pairing);
  merged = deepMerge(merged, foldShared(raw));
  // Built from typed defaults and the bundles, which the host's tests check.
  const config = merged as BarsConfig;
  config.preset = preset;
  config.top.design = top;
  config.dock.design = dock;
  const pick = (value: number, fallback: number) => (Number.isNaN(value) ? fallback : value);
  const { top: t, dock: d } = config;
  t.margin = pick(t.margin, t.layout === "strip" ? 0 : t.layout === "floating" ? 8 : 6);
  t.padding = pick(t.padding, t.layout === "strip" ? 5 : t.layout === "floating" ? 6 : 4);
  t.spacing = pick(t.spacing, t.layout === "strip" ? 1 : 2);
  const floatingDock = d.style === "floating";
  d.size = pick(d.size, floatingDock ? 48 : 24);
  d.margin = pick(d.margin, floatingDock ? 6 : 0);
  d.padding = pick(d.padding, floatingDock ? 5 : 12);
  d.spacing = pick(d.spacing, floatingDock ? 6 : 4);
  return config;
}
