// An in-page menu (power menu, dock context menu). One at a time. Closes on Escape, on a press
// outside, and when the window loses activation; arrows move, Enter activates.

import { button, h } from "./dom";
import type { Point, Size } from "./geometry";
import { mountLayer, type Layer } from "./overlay";

export type MenuEntry =
  | {
      kind: "item";
      label: string;
      /** Dim text on the right. */
      detail?: string;
      /** A leading dot (the active window). */
      marker?: "dot";
      /** Inverted: the one affirmative choice in a confirm step. */
      primary?: boolean;
      /** Runs on click. Return entries to swap the menu's content (a confirm step); otherwise it closes. */
      run(): void | MenuEntry[];
    }
  | { kind: "separator" }
  | { kind: "header"; label: string };

export interface MenuOptions {
  /** The element that opened it: pressing it again closes instead of reopening. */
  anchor: HTMLElement;
  entries: MenuEntry[];
  place(size: Size): Point;
  /** CSS transform-origin for the pop-in, toward the anchor. */
  origin?: string;
  onClose?(): void;
}

interface Open {
  el: HTMLElement;
  layer: Layer;
  anchor: HTMLElement;
  close(): void;
}

let current: Open | undefined;
let dismissedBy: { anchor: HTMLElement; at: number } | undefined;

export function closeMenu(): void {
  current?.close();
}

export function openMenu(options: MenuOptions): void {
  if (dismissedBy?.anchor === options.anchor && performance.now() - dismissedBy.at < 300) {
    dismissedBy = undefined;
    return;
  }
  closeMenu();

  const el = h("div", "menu");
  el.tabIndex = -1;
  el.setAttribute("role", "menu");
  if (options.origin) el.style.transformOrigin = options.origin;

  const fill = (entries: MenuEntry[]) => {
    el.replaceChildren(
      ...entries.map((entry) => {
        if (entry.kind === "separator") return h("div", "menu-sep");
        if (entry.kind === "header") return h("div", "menu-header", entry.label);
        const item = button(entry.primary ? "menu-item primary" : "menu-item");
        item.setAttribute("role", "menuitem");
        item.append(
          h("span", "menu-mark", entry.marker ? "●" : ""),
          h("span", "menu-label", entry.label),
          ...(entry.detail ? [h("span", "menu-detail", entry.detail)] : []),
        );
        item.addEventListener("click", () => {
          const next = entry.run();
          if (next) {
            fill(next);
            layer.reposition();
            el.focus({ preventScroll: true });
          } else {
            close();
          }
        });
        return item;
      }),
    );
  };

  const items = () => [...el.querySelectorAll<HTMLButtonElement>("button.menu-item")];
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    const all = items();
    if (all.length === 0) return;
    const at = all.findIndex((item) => item === document.activeElement);
    const target =
      event.key === "ArrowDown" ? (at + 1) % all.length
      : event.key === "ArrowUp" ? (at <= 0 ? all.length - 1 : at - 1)
      : event.key === "Home" ? 0
      : event.key === "End" ? all.length - 1
      : -1;
    if (target < 0) return;
    event.preventDefault();
    all[target]?.focus();
  };
  const onDown = (event: PointerEvent) => {
    if (!(event.target instanceof Node) || el.contains(event.target)) return;
    if (options.anchor.contains(event.target)) dismissedBy = { anchor: options.anchor, at: performance.now() };
    close();
  };

  fill(options.entries);
  const layer = mountLayer(el, options.place);

  function close() {
    if (current?.el !== el) return;
    current = undefined;
    document.removeEventListener("keydown", onKey, true);
    document.removeEventListener("pointerdown", onDown, true);
    window.removeEventListener("blur", close);
    layer.remove();
    options.onClose?.();
  }

  current = { el, layer, anchor: options.anchor, close };
  document.addEventListener("keydown", onKey, true);
  document.addEventListener("pointerdown", onDown, true);
  window.addEventListener("blur", close);
  el.focus({ preventScroll: true });
}
