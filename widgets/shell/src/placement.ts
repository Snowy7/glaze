// Which widgets the user may add or duplicate (docs/widgets.md: manifest `instances` and `preset`),
// and which window the shell page lives in. Pure. The host enforces the same rule; this only
// greys out what it would refuse.

import type { ShellState, WidgetManifest } from "./protocol";

export type AddCheck = { ok: true } | { ok: false; reason: string };

/** Popup widgets open from a bar module; they are not placed from the palette. */
export const inPalette = (manifest: Pick<WidgetManifest, "preset">): boolean => manifest.preset !== "popup";

/**
 * Whether one more instance of `manifest` may be added: `single` allows one in total (on any
 * monitor, per `counts`), `per-monitor` one among this shell's own instances.
 */
export function canAdd(
  manifest: Pick<WidgetManifest, "id" | "instances">,
  state: { instances: readonly { widget: string }[]; counts?: ShellState["counts"] },
): AddCheck {
  const here = state.instances.filter((i) => i.widget === manifest.id).length;
  switch (manifest.instances) {
    case "single":
      return (state.counts?.[manifest.id] ?? here) >= 1
        ? { ok: false, reason: "Only one of this widget is allowed, and it is already added" }
        : { ok: true };
    case "per-monitor":
      return here >= 1 ? { ok: false, reason: "Only one per monitor, and this monitor already has one" } : { ok: true };
    default:
      return { ok: true };
  }
}

/** Which window this page lives in; hosts that predate the field only have the desktop. */
export const layerOf = (state: Pick<ShellState, "layer">): "desktop" | "overlay" => state.layer ?? "desktop";
