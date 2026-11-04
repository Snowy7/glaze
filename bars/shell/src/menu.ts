// Menus (the power menu, the dock's context menu): the `menu` flyout, a list of entries. Arrows
// move, Home/End jump, Enter activates; an entry can swap the content for a confirm step. Opening,
// dismissal and the window region are flyout.ts's.

import { button, h } from "./dom";
import { closeFlyout, defineFlyout, type OpenOptions } from "./flyout";

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

export interface MenuOptions extends OpenOptions {
  entries: MenuEntry[];
}

const menu = defineFlyout<{ entries: MenuEntry[] }>("menu", ({ entries }, ctx) => {
  const el = h("div", "menu");
  el.setAttribute("role", "menu");

  const fill = (list: MenuEntry[]) => {
    el.replaceChildren(
      ...list.map((entry) => {
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
          if (!next) return ctx.close();
          fill(next);
          ctx.reposition();
          el.querySelector<HTMLButtonElement>("button.menu-item")?.focus();
        });
        return item;
      }),
    );
  };

  const onKey = (event: KeyboardEvent) => {
    const all = [...el.querySelectorAll<HTMLButtonElement>("button.menu-item")];
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
  document.addEventListener("keydown", onKey, true);
  ctx.scope.add(() => document.removeEventListener("keydown", onKey, true));

  fill(entries);
  return el;
});

/** Opens a menu, or closes it when it's already open from the same anchor. */
export function openMenu({ entries, ...options }: MenuOptions): void {
  menu.toggle({ entries }, options);
}

export function closeMenu(): void {
  closeFlyout();
}
