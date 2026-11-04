// Flyouts: in-page panels a bar opens over the screen (menus, the power menu; later a launcher,
// quick settings, a network list, a volume mixer, a calendar, the tray overflow…).
//
// Register one with `defineFlyout(id, render)`; open it from any module with `flyout.toggle(...)`.
// One flyout is open at a time. It closes on Escape, on a press outside it, and when the window
// loses activation (a click on another app). The open flyout is an overlay layer, so it's part of
// the window region (it shows and takes the pointer). It must fit in the window: the bar's band
// plus the room the host gives it (`ROOM` in protocol.ts).
//
// Styling: the element is `.flyout[data-flyout="<id>"]`, inside `.layer`; it enters from
// `--origin` (the side toward its anchor) with the `open` animation's style (`data-m-open`), and
// plays it backwards as `.leaving` when it closes.

import { h } from "./dom";
import type { Point, Size } from "./geometry";
import { createScope, type Scope } from "./host";
import { mountLayer, type Layer } from "./overlay";
import { leafDuration } from "./style/motion";

export interface FlyoutContext {
  /** Closes this flyout. */
  close(): void;
  /** Re-measures and re-places it (after its content changed size). */
  reposition(): void;
  /** Subscriptions and cleanups that end when it closes. */
  scope: Scope;
}

/** Builds a flyout's content each time it opens; `props` come from whoever opens it. */
export type FlyoutRender<P> = (props: P, ctx: FlyoutContext) => HTMLElement;

export interface OpenOptions {
  /** What opened it: pressing it again closes the flyout instead of reopening it. */
  anchor: HTMLElement;
  /** Where it goes, given its measured size (see `placePopover` in geometry.ts). */
  place(size: Size): Point;
  /** CSS transform-origin for the pop-in, toward the anchor. */
  origin?: string;
  onClose?(): void;
}

export interface Flyout<P> {
  readonly id: string;
  open(props: P, options: OpenOptions): void;
  /** Opens it, or closes it if it's open from the same anchor. */
  toggle(props: P, options: OpenOptions): void;
}

interface Open {
  id: string;
  anchor: HTMLElement;
  close(): void;
}

let current: Open | undefined;
/** A press on the anchor closes the flyout first; the click that follows must not reopen it. */
let dismissedBy: { anchor: HTMLElement; at: number } | undefined;
const ids = new Set<string>();

export function closeFlyout(): void {
  current?.close();
}

/** Whether a flyout (any, or `id`) is open. */
export function flyoutOpen(id?: string): boolean {
  return !!current && (id === undefined || current.id === id);
}

export function defineFlyout<P>(id: string, render: FlyoutRender<P>): Flyout<P> {
  if (ids.has(id)) throw new Error(`flyout ${id} is defined twice`);
  ids.add(id);

  const open = (props: P, options: OpenOptions) => {
    closeFlyout();
    const el = h("div", "flyout");
    el.dataset.flyout = id;
    el.tabIndex = -1;
    if (options.origin) el.style.setProperty("--origin", options.origin);
    const scope = createScope();
    // `reposition` may run while rendering (a subscription replaying its latest value): before the
    // layer exists there's nothing to place yet.
    let layer: Layer | undefined;
    const ctx: FlyoutContext = { close: () => close(), reposition: () => layer?.reposition(), scope };
    el.append(render(props, ctx));
    const mounted = mountLayer(el, options.place);
    layer = mounted;

    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close();
    };
    const onDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || el.contains(event.target)) return;
      if (options.anchor.contains(event.target)) dismissedBy = { anchor: options.anchor, at: performance.now() };
      close();
    };
    function close() {
      if (current?.close !== close) return;
      current = undefined;
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", close);
      scope.dispose();
      // It plays its exit (`.leaving`, the `open` animation's length), then goes.
      const exit = leafDuration("open");
      if (exit > 0) {
        el.classList.add("leaving");
        window.setTimeout(() => mounted.remove(), exit);
      } else {
        mounted.remove();
      }
      options.onClose?.();
    }

    current = { id, anchor: options.anchor, close };
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", close);
    el.focus({ preventScroll: true });
  };

  return {
    id,
    open,
    toggle(props, options) {
      const justClosed =
        dismissedBy?.anchor === options.anchor && performance.now() - dismissedBy.at < 300;
      dismissedBy = undefined;
      if (justClosed) return;
      if (current?.id === id && current.anchor === options.anchor) return closeFlyout();
      open(props, options);
    },
  };
}
