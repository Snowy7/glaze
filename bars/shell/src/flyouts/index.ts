// The flyouts (docs/bars.md, "Flyouts"): what the top-bar modules open. Each big one is its own
// chunk, loaded the first time it opens, so a bar that never opens the launcher never loads it;
// its DOM is built on open and dropped on close (flyout.ts), and the host starts its data
// provider on the first request and releases it when idle.

import "./flyouts.css";
import { closeFlyout, flyoutOpen } from "../flyout";
import { fire, on } from "../host";
import { bellModule, type ModuleCtx } from "./modules";
import type { FlyoutName } from "./protocol";

export { initSurfaces, setSurfaceConfig } from "./surface";
export { languageModule, trayModule } from "./modules";

/** A module loaded once, the first time it's asked for. */
function lazy<T>(load: () => Promise<T>): () => Promise<T> {
  let loading: Promise<T> | undefined;
  return () => (loading ??= load());
}

const launcher = lazy(() => import("./launcher/launcher"));
const quick = lazy(() => import("./quick"));
const network = lazy(() => import("./network"));
const mixer = lazy(() => import("./mixer"));
const calendar = lazy(() => import("./calendar"));
const notifications = lazy(() => import("./notifications"));

/**
 * Opens a flyout from `anchor`. If it can't (its chunk or its content failed), the host logs that
 * and opens the matching native Windows panel instead, so the click still does something.
 */
const opener =
  (name: FlyoutName, load: () => Promise<{ open(anchor: HTMLElement): Promise<void> }>) =>
  (anchor: HTMLElement): void => {
    load()
      .then((m) => m.open(anchor))
      .catch((error: unknown) => {
        console.error(`[bars] the ${name} flyout failed to open`, error);
        fire("flyoutFailed", { flyout: name, error: String(error).slice(0, 200) });
      });
  };

export const openLauncher = opener("launcher", launcher);
export const openQuick = opener("quick", quick);
export const openNetwork = opener("network", network);
export const openMixer = opener("mixer", mixer);
export const openCalendar = opener("calendar", calendar);
export const openNotifications = opener("notifications", notifications);

/** The `notifications` module: a bell with the count, opening the notifications flyout. */
export const notificationsModule = (ctx: ModuleCtx): HTMLElement => bellModule(ctx, openNotifications);

/**
 * Opens the launcher, or closes it when it's open: from the start module if this bar has one, else
 * from `from` (the module an action was bound on), else from the bar itself. The Windows key and
 * the `start` action both come here, so they open the same launcher in the same place.
 */
export function toggleLauncher(from?: HTMLElement): void {
  if (flyoutOpen("launcher")) return closeFlyout();
  openLauncher(
    document.querySelector<HTMLElement>('[data-module="start"]') ??
      from ??
      document.querySelector<HTMLElement>(".pill") ??
      document.body,
  );
}

// The Windows key: the host made this bar the foreground window.
on("open", ({ flyout }) => {
  if (flyout === "launcher") toggleLauncher();
});
