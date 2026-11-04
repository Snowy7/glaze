// Pure helpers over the dock's item list: grouping, the divider, indicators, optimistic updates.

import type { DockItem } from "../protocol";
import type { DropAction } from "./drag";

/** Pinned items first, then running ones (stable). The host already sends this order. */
export function orderItems(items: readonly DockItem[]): DockItem[] {
  return [...items.filter((i) => i.pinned), ...items.filter((i) => !i.pinned)];
}

/** The index the divider sits before, or null unless both groups are present. */
export function dividerIndex(items: readonly DockItem[]): number | null {
  const pinned = items.filter((i) => i.pinned).length;
  return pinned > 0 && pinned < items.length ? pinned : null;
}

export type Indicator = "none" | "running" | "active";

export function indicator(item: DockItem): Indicator {
  if (item.windows.length === 0) return "none";
  return item.windows.some((w) => w.active) ? "active" : "running";
}

/** Whether any of the item's windows is flashing for attention. */
export const wantsAttention = (item: DockItem): boolean => item.windows.some((w) => w.attention);

/** A dock item as drawn: with `split`, a window of an app that has several gets its own. */
export interface Shown extends DockItem {
  /** For a split-off window: the app item it came from (what pins, unpins and launches). */
  of?: DockItem;
}

/**
 * One item per window (`dock.split`): an app's first window keeps the app's item, each other
 * window follows it as an item of its own, keyed and titled by the window. Split-off items carry
 * a path no pin has, so dragging them reorders nothing.
 */
export function splitItems(items: readonly DockItem[]): Shown[] {
  return items.flatMap((item): Shown[] => {
    if (item.windows.length <= 1) return [item];
    const [first, ...rest] = item.windows;
    return [
      { ...item, windows: first ? [first] : [] },
      ...rest.map((w) => ({
        ...item,
        key: `${item.key}#${w.hwnd}`,
        path: `${item.path}#${w.hwnd}`,
        name: w.title || item.name,
        windows: [w],
        of: item,
      })),
    ];
  });
}

/** What a task button's title shows: its window's title, else the app's name. */
export function itemTitle(item: DockItem): string {
  return item.windows.length === 1 ? item.windows[0]?.title || item.name : item.name;
}

/** The list as it will look once the host applies `action`, so the dock doesn't flash old state. */
export function applyDrop(items: readonly DockItem[], action: DropAction): DockItem[] {
  const ordered = orderItems(items);
  const pinned = ordered.filter((i) => i.pinned);
  const running = ordered.filter((i) => !i.pinned);
  switch (action.type) {
    case "none":
      return ordered;
    case "reorder":
      return [
        ...action.paths.flatMap((p) => pinned.filter((i) => i.path === p)),
        ...pinned.filter((i) => !action.paths.includes(i.path)),
        ...running,
      ];
    case "pin": {
      const item = running.find((i) => i.path === action.path);
      if (!item) return ordered;
      return [
        ...pinned.slice(0, action.index),
        { ...item, pinned: true },
        ...pinned.slice(action.index),
        ...running.filter((i) => i !== item),
      ];
    }
    case "unpin": {
      const item = pinned.find((i) => i.path === action.path);
      if (!item) return ordered;
      const rest = pinned.filter((i) => i !== item);
      return item.windows.length > 0 ? [...rest, { ...item, pinned: false }, ...running] : [...rest, ...running];
    }
  }
}
