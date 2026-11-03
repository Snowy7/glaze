// Dock layouts, by `config.dock.style`. A new style is a factory here plus a band size in the host
// (`DockStyle::band` in crates/bars); typed by `DockStyle`, so adding one to protocol.ts fails the
// build until it has a layout.

import type { Bar, BarContext } from "../bar";
import type { BarsConfig, DockStyle } from "../protocol";
import { createDock } from "./dock";

export type DockFactory = (root: HTMLElement, config: BarsConfig, ctx: BarContext) => Bar;

export const dockStyles: Record<DockStyle, DockFactory> = {
  floating: createDock,
};
