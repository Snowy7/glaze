// Dock layouts, by `config.dock.style`. A new style is a factory here (the host sizes its band
// from size, padding and margin); typed by `DockStyle`, so adding one to protocol.ts fails the
// build until it has a layout. Both built-in styles share dock.ts: `taskbar` is the floating
// pill stretched along its edge, with modules and task buttons.

import type { Bar, BarContext } from "../bar";
import type { BarsConfig, DockStyle } from "../protocol";
import { createDock } from "./dock";

export type DockFactory = (root: HTMLElement, config: BarsConfig, ctx: BarContext) => Bar;

export const dockStyles: Record<DockStyle, DockFactory> = {
  floating: createDock,
  taskbar: createDock,
};
