// The flyouts (docs/bars.md, "Flyouts"): what the top-bar modules open. Each big one is its own
// chunk, loaded the first time it opens, so a bar that never opens the launcher never loads it;
// its DOM is built on open and dropped on close (flyout.ts), and the host starts its data
// provider on the first request and releases it when idle.

import "./flyouts.css";
import { closeFlyout, flyoutOpen } from "../flyout";
import { on } from "../host";
import { bellModule, type ModuleCtx } from "./modules";

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

const opener =
  (load: () => Promise<{ open(anchor: HTMLElement): Promise<void> }>) =>
  (anchor: HTMLElement): void => {
    load()
      .then((m) => m.open(anchor))
      .catch((error: unknown) => console.error("[bars] a flyout failed to open", error));
  };

export const openLauncher = opener(launcher);
export const openQuick = opener(quick);
export const openNetwork = opener(network);
export const openMixer = opener(mixer);
export const openCalendar = opener(calendar);
export const openNotifications = opener(notifications);

/** The `notifications` module: a bell with the count, opening the notifications flyout. */
export const notificationsModule = (ctx: ModuleCtx): HTMLElement => bellModule(ctx, openNotifications);

// The Windows key: the host made this bar the foreground window; open (or close) the launcher
// from the start module, else from the bar itself.
on("open", ({ flyout }) => {
  if (flyout !== "launcher") return;
  if (flyoutOpen("launcher")) return closeFlyout();
  const anchor =
    document.querySelector<HTMLElement>('[data-module="start"]') ??
    document.querySelector<HTMLElement>(".pill") ??
    document.body;
  openLauncher(anchor);
});
