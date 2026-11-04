// The top-bar modules the flyouts own (docs/bars.md, "Modules"): `tray` (the pinned icons, and a
// chevron to the overflow grid), `language` (the keyboard layout in front) and `notifications`
// (a bell with the count). Their flyouts open from here.

import { button, glyph, h, rectOf } from "../dom";
import { defineFlyout } from "../flyout";
import { fire, request, type Scope } from "../host";
import type { BarsConfig, TrayItem } from "../protocol";
import { glyphs } from "./glyphs";
import { splitTray } from "./model";
import type { Languages } from "./protocol";
import { show } from "./surface";
import { appIcon, header, row, settingsLink } from "./ui";

export interface ModuleCtx {
  config: BarsConfig;
  scope: Scope;
}

// --- Tray -------------------------------------------------------------------------------------

type Click = "left" | "right" | "middle" | "double";

/** Forwards a click on an icon, with where the icon is (apps put their menus beside it). */
function forward(id: string, click: Click, el: HTMLElement): void {
  fire("tray", { id, button: click, rect: rectOf(el) });
}

/** One notification-area icon as a button: left, right, middle and double clicks go to its app. */
function trayButton(item: TrayItem, className: string, onActivate?: () => void): HTMLButtonElement {
  const el = button(className, appIcon(item.icon, item.name, "tray-icon"));
  el.title = item.name;
  el.dataset.name = item.name;
  el.addEventListener("click", () => {
    forward(item.id, "left", el);
    onActivate?.();
  });
  el.addEventListener("dblclick", () => forward(item.id, "double", el));
  el.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    forward(item.id, "right", el);
  });
  el.addEventListener("auxclick", (event) => {
    if (event.button === 1) forward(item.id, "middle", el);
  });
  return el;
}

/** The icons that aren't pinned, in a grid (YASB-style). */
const overflow = defineFlyout<{ items: TrayItem[] }>("tray", ({ items }, ctx) => {
  const grid = h("div", "tray-grid");
  const fill = (list: TrayItem[]) => {
    grid.replaceChildren(...list.map((item) => trayButton(item, "tray-cell")));
    if (list.length === 0) grid.append(h("div", "fly-notice", "No other icons"));
    ctx.reposition();
  };
  fill(items);
  ctx.scope.on("tray", (all) => fill(splitTray(all).overflow));
  return h("div", "tray-overflow", grid);
});

export function trayModule({ scope }: ModuleCtx): HTMLElement {
  const el = h("div", "tray");
  const more = button("mod tray-item tray-more", glyph(glyphs.chevronUp));
  more.title = "Show hidden icons";
  let hidden: TrayItem[] = [];
  let native: TrayItem | undefined;
  more.addEventListener("click", () => {
    // The UI Automation fallback: the native overflow is all there is.
    if (native && hidden.length === 0) return fire("tray", { id: native.id, button: "left" });
    void show(overflow, { items: hidden }, { anchor: more, align: "end" });
  });
  scope.on("tray", (items) => {
    const split = splitTray(items);
    hidden = split.overflow;
    native = split.native;
    const shown = split.bar.map((item) => trayButton(item, "mod tray-item"));
    el.replaceChildren(...shown, ...(hidden.length > 0 || native ? [more] : []));
  });
  return el;
}

// --- Language ------------------------------------------------------------------------------------

const language = defineFlyout<{ state: Languages }>("language", ({ state }, ctx) => {
  const list = h("div", "fly-list");
  const fill = (s: Languages) => {
    list.replaceChildren(
      ...s.layouts.map((layout) => {
        const badge = h("span", "lang-badge", layout.short);
        const current = layout.id === s.current;
        const item = row(badge, layout.language, layout.layout, ...(current ? [glyph(glyphs.check, "fly-check")] : []));
        item.classList.toggle("is-current", current);
        item.addEventListener("click", () => {
          request("setLayout", { id: layout.id })
            .then(() => ctx.close())
            .catch((error: unknown) => console.warn("[bars] setLayout failed", error));
        });
        return item;
      }),
    );
  };
  fill(state);
  ctx.scope.on("language", fill);
  return h(
    "div",
    "fly-panel lang-panel",
    header("Keyboard"),
    list,
    h("div", "fly-footer", settingsLink("Language settings", "ms-settings:regionlanguage"), settingsLink("Keyboard", "ms-settings:keyboard")),
  );
});

export function languageModule({ scope }: ModuleCtx): HTMLElement {
  const short = h("span", "lang-short");
  const region = h("span", "lang-region");
  const el = button("mod lang", h("span", "lang-stack", short, region));
  let state: Languages | undefined;
  el.addEventListener("click", () => {
    if (state) void show(language, { state }, { anchor: el, align: "end" });
  });
  scope.on("language", (next) => {
    state = next;
    const current = next.layouts.find((l) => l.id === next.current) ?? next.layouts[0];
    el.hidden = !current;
    short.textContent = current?.short ?? "";
    region.textContent = current?.region ?? "";
    el.title = current ? `${current.language} — ${current.layout}` : "";
  });
  return el;
}

// --- Notifications ----------------------------------------------------------------------------------

export function bellModule({ scope }: ModuleCtx, open: (anchor: HTMLElement) => void): HTMLElement {
  const badge = h("span", "notif-badge");
  const el = button("mod icon-only notif", glyph(glyphs.bell), badge);
  el.title = "Notifications";
  el.addEventListener("click", () => open(el));
  scope.on("notifications", ({ count }) => {
    badge.hidden = !count;
    badge.textContent = count && count > 99 ? "99+" : String(count ?? "");
    el.title = count ? `${count} notification${count === 1 ? "" : "s"}` : "Notifications";
    el.dataset.count = String(count ?? 0);
  });
  return el;
}
