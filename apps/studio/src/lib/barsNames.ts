// What Studio calls the bars' presets and designs, with one line each (crates/bars/presets).

import type { DockDesign, PresetId, TopDesign } from "./barsSchema";

type Named = { name: string; blurb: string };

/** Pairings: a top design, a bottom design, and which bars are on. */
export const PRESETS: Record<PresetId, Named> = {
  monochrome: { name: "Monochrome", blurb: "Black glass, white type, a floating dock" },
  macos: { name: "macOS", blurb: "A menu bar and a magnifying dock" },
  windows11: { name: "Windows 11", blurb: "The full-width taskbar, no top bar" },
  windows10: { name: "Windows 10", blurb: "A left-aligned taskbar, square buttons" },
  chromeos: { name: "ChromeOS", blurb: "A shelf with a launcher circle" },
  islands: { name: "Islands", blurb: "One pill per group of modules" },
  "neon-glass": { name: "Neon glass", blurb: "Dark glass with a neon edge" },
  floating: { name: "Floating", blurb: "Detached, rounded bars" },
};

export const TOP_DESIGNS: Record<TopDesign, Named> = {
  monochrome: { name: "Monochrome", blurb: "A black glass strip with white type" },
  macos: { name: "macOS", blurb: "The menu bar: the app in bold, status on the right" },
  fluent: { name: "Windows 11", blurb: "Mica-like, Fluent icons" },
  capsule: { name: "Capsule", blurb: "A floating glass pill" },
  minimal: { name: "Minimal", blurb: "No surface: the clock and a few icons" },
  islands: { name: "Islands", blurb: "A clear rail, one pill per group" },
  neon: { name: "Neon", blurb: "Dark glass, a neon edge" },
  floating: { name: "Floating", blurb: "A detached, rounded bar" },
};

export const DOCK_DESIGNS: Record<DockDesign, Named> = {
  monochrome: { name: "Monochrome", blurb: "A black glass pill with dots" },
  macos: { name: "macOS dock", blurb: "Glass, magnify, running dots" },
  windows11: { name: "Windows 11 taskbar", blurb: "Centered, pill indicators" },
  windows10: { name: "Windows 10 taskbar", blurb: "Left-aligned, underline indicators" },
  chromeos: { name: "ChromeOS shelf", blurb: "Centered apps, the status area as a pill" },
  fluent: { name: "Fluent dock", blurb: "The Windows 11 taskbar as a floating island" },
  islands: { name: "Islands", blurb: "A solid pill with bar indicators" },
  neon: { name: "Neon", blurb: "Dark glass, glowing indicators" },
  floating: { name: "Floating", blurb: "A detached, rounded pill" },
};
