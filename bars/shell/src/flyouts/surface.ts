// How the flyouts open, on top of flyout.ts: where they go (under the top bar's strip, or off the
// dock toward the middle of the screen), the window room a big one needs first (the host grows
// the bar's window for it and shrinks it after), and the keyboard for the ones you type into.

import { closeFlyout, flyoutOpen, type Flyout } from "../flyout";
import { barAnchor } from "../anchor";
import { rectOf, viewport } from "../dom";
import { placePopover, type Align, type Point, type Size } from "../geometry";
import { fire, request } from "../host";
import { dockBand, topBand, type BarsConfig, type Hello } from "../protocol";

let hello: Hello | undefined;
let config: BarsConfig | undefined;
/** When the last pointer press on the page was (`performance.now()`). */
let lastPress = Number.NEGATIVE_INFINITY;
/** A flyout closing this soon after a press was closed by it. */
const PRESS_CLOSE_MS = 500;

/** main.ts hands over the bar's hello, and each config as it's built. */
export function initSurfaces(h: Hello): void {
  hello = h;
  config = h.config;
  document.addEventListener("pointerdown", () => (lastPress = performance.now()), { capture: true, passive: true });
}

export function setSurfaceConfig(next: BarsConfig): void {
  config = next;
}

export function barsConfig(): BarsConfig {
  if (!config) throw new Error("flyouts used before initSurfaces");
  return config;
}

const isTop = (): boolean => hello?.bar !== "dock";

/** The bar's band, across (DIPs). */
function band(): number {
  const c = barsConfig();
  return isTop() ? topBand(c.top) : dockBand(c.dock);
}

/** Along the axis flyouts grow: true for the top bar and a bottom dock. */
const vertical = (): boolean => isTop() || barsConfig().dock.position === "bottom";

/** How far past the band a flyout can reach now (the window), and at most (the monitor). */
export function reach(): { now: number; max: number } {
  const window_ = viewport();
  const monitor = hello?.monitor ?? { width: window_.w, height: window_.h };
  return vertical()
    ? { now: window_.h - band(), max: monitor.height - band() }
    : { now: window_.w - band(), max: monitor.width - band() };
}

/** Grows the window so a flyout `extent` DIPs past the band fits, before it opens. */
export async function ensureRoom(extent: number): Promise<void> {
  const want = Math.min(extent, reach().max);
  if (reach().now >= want) return;
  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      window.removeEventListener("resize", onResize);
      resolve();
    };
    const onResize = () => {
      if (reach().now >= want) finish();
    };
    window.addEventListener("resize", onResize);
    request("room", { extent })
      .catch((error: unknown) => console.warn("[bars] room failed", error))
      .finally(() => setTimeout(finish, 250));
  });
}

let releaseTimer = 0;
/** Gives the room back once no flyout is open (after any close animation). */
function releaseRoom(): void {
  clearTimeout(releaseTimer);
  releaseTimer = window.setTimeout(() => {
    if (!flyoutOpen()) fire("room", { extent: 0 });
  }, 450);
}

/** Where a flyout opened from `anchor` goes, and the side it grows from. */
export function placement(anchor: HTMLElement, align: Align): { place(size: Size): Point; origin: string } {
  const [x, y, w, h] = rectOf(anchor);
  if (isTop()) {
    const strip = barAnchor(anchor).rect;
    const origin = align === "start" ? "0 0" : align === "end" ? "100% 0" : "50% 0";
    return { place: (size) => {
      // A dropdown can grow after opening (for example, the mixer's device list).
      void ensureRoom(size.h + 20);
      const bounds = viewport();
      bounds.h = Math.max(bounds.h, strip[1] + strip[3] + 12 + size.h + 8);
      return placePopover(strip, size, bounds, "below", align, 12);
    }, origin };
  }
  const position = barsConfig().dock.position;
  const side = position === "bottom" ? "above" : position === "left" ? "right" : "left";
  const origin = position === "bottom" ? "50% 100%" : position === "left" ? "0 50%" : "100% 50%";
  return { place: (size) => placePopover([x, y, w, h], size, viewport(), side, align, 12), origin };
}

export interface ShowOptions {
  /** What opened it: its module, or the bar for the Windows key. */
  anchor: HTMLElement;
  align?: Align;
  /** How far past the band it may reach (DIPs); more than the default room grows the window. */
  extent?: number;
  /** It's typed into: the bar takes the keyboard while it's open. */
  keyboard?: boolean;
}

let shown: { id: string; anchor: HTMLElement } | undefined;

/** Opens `flyout`, or closes it when it's already open from the same anchor. */
export async function show<P>(flyout: Flyout<P>, props: P, options: ShowOptions): Promise<void> {
  if (flyoutOpen(flyout.id) && shown?.anchor === options.anchor) {
    closeFlyout();
    return;
  }
  if (options.extent !== undefined) await ensureRoom(options.extent);
  const { place, origin } = placement(options.anchor, options.align ?? "center");
  flyout.toggle(props, {
    anchor: options.anchor,
    place,
    origin,
    onClose: () => {
      if (shown?.id === flyout.id) shown = undefined;
      // Closed by a press (another module, a tile): the press decides what's in front. Closed by
      // a key: the app the user came from gets the foreground back.
      if (options.keyboard) fire("keyboard", { on: false, handBack: performance.now() - lastPress > PRESS_CLOSE_MS });
      releaseRoom();
    },
  });
  if (!flyoutOpen(flyout.id)) {
    // The click that dismissed it (flyout.ts closes on the anchor's press).
    releaseRoom();
    return;
  }
  shown = { id: flyout.id, anchor: options.anchor };
  if (options.keyboard) fire("keyboard", { on: true });
}

/** The largest size that fits past the band, minus a margin. */
export function fit(width: number, height: number): { w: number; h: number } {
  const v = viewport();
  const r = reach();
  return vertical()
    ? { w: Math.min(width, v.w - 16), h: Math.min(height, r.max - 16) }
    : { w: Math.min(width, r.max - 16), h: Math.min(height, v.h - 16) };
}
