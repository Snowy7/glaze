// Liquid glass (docs/widgets.md, "Liquid glass"): while a widget's look is liquid glass
// (`data-style="liquid"` with `data-surface="glass"`), the wallpaper draws the glass under its
// card and the page draws only the content. This tells the shell where the card is: its box on
// the page and its corner radius, again whenever they change (a resize, a transition, a new
// look), at most once a frame. The drop-in runs it; with the ES module, call it yourself.

import type { Mypaper, PlateShape } from "./types";

/** Whether `root`'s look is liquid glass. */
export const isLiquid = (root: HTMLElement): boolean =>
  root.dataset["style"] === "liquid" && root.dataset["surface"] === "glass";

/** `el`'s plate: its box in page px, and its top-left radius in the same px (capped at a pill's). */
export function plateOf(el: HTMLElement): PlateShape | null {
  const box = el.getBoundingClientRect();
  if (box.width < 1 || box.height < 1) return null;
  // The box is after transforms (the kit scales the body); the radius is in the element's own px.
  const scale = el.offsetWidth > 0 ? box.width / el.offsetWidth : 1;
  const raw = getComputedStyle(el).borderTopLeftRadius;
  const radius = raw.endsWith("%") ? (Number.parseFloat(raw) / 100) * el.offsetWidth : Number.parseFloat(raw);
  const round = (v: number) => Math.round(v * 100) / 100;
  return {
    rect: [round(box.left), round(box.top), round(box.width), round(box.height)],
    radius: round(Math.min((Number.isFinite(radius) ? radius : 0) * scale, box.width / 2, box.height / 2)),
  };
}

export interface PlateTracker {
  /** Measure again soon (the look changed). */
  refresh(): void;
  stop(): void;
}

/** Reports the `.card`'s plate through `mypaper.plate` while the look is liquid glass. */
export function trackPlate(mypaper: Pick<Mypaper, "plate">, doc: Document = document): PlateTracker {
  const root = doc.documentElement;
  const view = doc.defaultView ?? window;
  /** The last report, as JSON; `undefined` before the first, so a page that is never liquid says nothing. */
  let sent: string | undefined;
  let frame = 0;
  let card: HTMLElement | null = null;
  const resize = new ResizeObserver(() => refresh());

  function report() {
    frame = 0;
    const el = doc.querySelector<HTMLElement>(".card");
    if (el !== card) {
      if (card) resize.unobserve(card);
      card = el;
      if (el) resize.observe(el);
    }
    const shape = el && isLiquid(root) ? plateOf(el) : null;
    const key = JSON.stringify(shape);
    if (key === sent || (sent === undefined && shape === null)) return;
    sent = key;
    mypaper.plate(shape);
  }

  function refresh() {
    if (!frame) frame = view.requestAnimationFrame(report);
  }

  view.addEventListener("resize", refresh);
  refresh();
  return {
    refresh,
    stop() {
      view.cancelAnimationFrame(frame);
      view.removeEventListener("resize", refresh);
      resize.disconnect();
      if (sent !== undefined && sent !== "null") mypaper.plate(null);
    },
  };
}
