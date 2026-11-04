// Window previews (docs/bars.md, "Previews"): hovering a running item for `dock.preview.delay`
// opens the `preview` flyout, a card per window (title, close button, and a box where the host
// draws the window's live DWM thumbnail into our window). Moving to another item switches it;
// leaving both for a moment closes it.

import { button, h, glyphLetter, viewport } from "../dom";
import { closeFlyout, defineFlyout, flyoutOpen } from "../flyout";
import type { Point, Rect, Side, Size } from "../geometry";
import { placePopover } from "../geometry";
import { fire, request } from "../host";
import type { DockItem } from "../protocol";
import { leafDuration } from "../style/motion";

/** The largest thumbnail, DIPs. */
export const THUMB_MAX: Size = { w: 220, h: 132 };

/** Each window's thumbnail size: its aspect, fitted into `max`. Pure, so it can be tested. */
export function thumbSizes(sources: readonly { width: number; height: number }[], max = THUMB_MAX): Size[] {
  return sources.map(({ width, height }) => {
    const scale = Math.min(max.w / Math.max(1, width), max.h / Math.max(1, height));
    return { w: Math.max(48, Math.round(width * scale)), h: Math.max(32, Math.round(height * scale)) };
  });
}

interface PreviewProps {
  item: DockItem;
  sizes: Map<number, Size>;
  onEnter(): void;
  onLeave(): void;
}

const flyout = defineFlyout<PreviewProps>("preview", ({ item, sizes, onEnter, onLeave }, ctx) => {
  const el = h("div", "preview");
  el.addEventListener("pointerenter", onEnter);
  el.addEventListener("pointerleave", onLeave);
  const thumbs: { hwnd: number; box: HTMLElement }[] = [];
  for (const w of item.windows) {
    const size = sizes.get(w.hwnd);
    if (!size) continue;
    const icon = item.icon ? Object.assign(h("img", "pv-icon"), { src: item.icon, alt: "" }) : h("span", "pv-icon ph-mini", glyphLetter(item.name));
    const close = button("pv-close", "×");
    close.title = "Close window";
    close.addEventListener("click", (e) => {
      e.stopPropagation();
      fire("close", { hwnd: w.hwnd });
      ctx.close();
    });
    const box = h("div", "pv-thumb");
    box.style.width = `${size.w}px`;
    box.style.height = `${size.h}px`;
    const card = button("pv-card", h("div", "pv-head", icon, h("span", "pv-title", w.title || item.name), close), box);
    card.dataset.active = String(w.active);
    card.addEventListener("click", () => {
      fire("activate", { hwnd: w.hwnd });
      ctx.close();
    });
    thumbs.push({ hwnd: w.hwnd, box });
    el.append(card);
  }
  // The host draws each thumbnail over its box once the card has finished opening.
  const place = () => {
    const windows = thumbs.map(({ hwnd, box }) => {
      const r = box.getBoundingClientRect();
      const rect: [number, number, number, number] = [r.left, r.top, r.width, r.height];
      return { hwnd, rect };
    });
    fire("previewPlace", { windows });
  };
  const timer = window.setTimeout(place, leafDuration("open") + 20);
  ctx.scope.add(() => {
    clearTimeout(timer);
    fire("previewClose");
  });
  return el;
});

export interface PreviewController {
  /** The pointer is over `item`'s slot. */
  hover(item: DockItem, slot: HTMLElement): void;
  /** The pointer left a slot (or the dock). */
  leave(): void;
  close(): void;
  readonly open: boolean;
}

/**
 * Previews for one dock. `anchor` gives the rect to open next to (the slot along the dock, the
 * pill across it) and the side toward the screen's middle.
 */
export function createPreviews(
  delay: number,
  anchor: (slot: HTMLElement) => { rect: Rect; side: Side; origin: string; gap: number },
): PreviewController {
  let showTimer = 0;
  let hideTimer = 0;
  let current: string | undefined;
  let generation = 0;

  const cancelHide = () => clearTimeout(hideTimer);
  const scheduleHide = () => {
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => controller.close(), 280);
  };

  const show = async (item: DockItem, slot: HTMLElement) => {
    const asked = ++generation;
    const hwnds = item.windows.map((w) => w.hwnd);
    const sources = await request("previewOpen", { hwnds }).catch(() => []);
    if (asked !== generation || sources.length === 0) return;
    const fitted = thumbSizes(sources);
    const sizes = new Map(sources.map((s, i) => [s.hwnd, fitted[i] ?? THUMB_MAX]));
    const { rect, side, origin, gap } = anchor(slot);
    current = item.key;
    flyout.open(
      { item, sizes, onEnter: cancelHide, onLeave: scheduleHide },
      {
        anchor: slot,
        origin,
        place: (size): Point => placePopover(rect, size, viewport(), side, "center", gap),
        onClose: () => {
          if (current === item.key) current = undefined;
        },
      },
    );
  };

  const controller: PreviewController = {
    hover(item, slot) {
      cancelHide();
      if (item.windows.length === 0) return controller.close();
      if (current === item.key && flyoutOpen("preview")) return;
      clearTimeout(showTimer);
      // Already showing one: switch at once, like the taskbar.
      const wait = flyoutOpen("preview") ? 0 : delay;
      showTimer = window.setTimeout(() => void show(item, slot), wait);
    },
    leave() {
      clearTimeout(showTimer);
      scheduleHide();
    },
    close() {
      clearTimeout(showTimer);
      cancelHide();
      generation++;
      current = undefined;
      if (flyoutOpen("preview")) closeFlyout();
    },
    get open() {
      return flyoutOpen("preview");
    },
  };
  return controller;
}
