// The notification center's view (docs/bars.md, "Flyouts" → "Notifications"); notifications.ts
// feeds it the list and owns what dismissing and clearing do.
//
// - Toasts are grouped by app, newest group first. A group of several is a stack (the newest card
//   on top, the rest peeking under it) until it's opened.
// - Every card carries its app's icon: the toast's own logo, else the Start menu's icon for an app
//   of that name (the launcher's index), else a tinted monogram.
// - Dismissing is a swipe sideways or the × that shows on hover. A group clears with its ×, and
//   everything with "Clear all".
// - The header shows Do Not Disturb (it opens its setting: there's no API to change it), and the
//   month sits under the list, as Windows 11 puts it (the clock opens this center).
// - With nothing to show, there's a real empty state.

import { button, glyph, glyphLetter, h } from "../dom";
import { fire, request } from "../host";
import { glyphs } from "./glyphs";
import { ago, firstDayOf, monthGrid } from "./model";
import type { NotificationItem, NotificationList } from "./protocol";
import { barsConfig } from "./surface";
import { iconButton, settingsLink } from "./ui";

export interface CenterActions {
  /** Dismiss these toasts (one, or a whole group). */
  dismiss(ids: readonly NotificationItem["id"][]): void;
  clearAll(): void;
  close(): void;
}

/** How far a card must be swiped to go, DIPs. */
const SWIPE = 90;

interface Group {
  app: string;
  icon?: string;
  items: NotificationItem[];
}

/** Toasts grouped by app, the apps in the order of their newest toast. */
export function byApp(list: readonly NotificationItem[]): Group[] {
  const groups = new Map<string, Group>();
  for (const item of [...list].sort((a, b) => b.at - a.at)) {
    const group = groups.get(item.app) ?? { app: item.app, items: [] };
    if (!group.icon && item.icon) group.icon = item.icon;
    group.items.push(item);
    groups.set(item.app, group);
  }
  return [...groups.values()];
}

/** A stable hue for an app's monogram, so each app keeps its color. */
export function hueOf(name: string): number {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return hash % 360;
}

// The launcher's index knows every Start menu app's icon, for toasts without a logo. Its icons
// are drawn after the index is built, so an answer without them is asked for again next time.
let startIcons: Promise<Map<string, string>> | undefined;
const startMenuIcons = (): Promise<Map<string, string>> => {
  startIcons ??= request("apps")
    .then(({ apps }) => new Map(apps.flatMap((app) => (app.icon ? [[app.name.toLowerCase(), app.icon] as const] : []))))
    .catch(() => new Map<string, string>())
    .then((icons) => {
      if (icons.size === 0) startIcons = undefined;
      return icons;
    });
  return startIcons;
};

/** The app's icon: its logo, else the Start menu's, else a monogram in the app's own hue. */
function appBadge(group: Group, className: string): HTMLElement {
  const monogram = h("span", `${className} notif-monogram`, glyphLetter(group.app || "?"));
  monogram.style.setProperty("--hue", String(hueOf(group.app)));
  const picture = (src: string) => {
    const img = h("img", className);
    img.alt = "";
    img.src = src;
    img.addEventListener("error", () => img.replaceWith(monogram), { once: true });
    return img;
  };
  if (group.icon) return picture(group.icon);
  const slot = h("span", "notif-badge-slot", monogram);
  void startMenuIcons().then((icons) => {
    const name = group.app.toLowerCase();
    const src = icons.get(name) ?? [...icons].find(([app]) => app.startsWith(name) || name.startsWith(app))?.[1];
    if (src) slot.replaceChildren(picture(src));
  });
  return slot;
}

/** Swipe sideways to dismiss: the card follows the pointer and goes past `SWIPE`. */
function swipeable(card: HTMLElement, dismiss: () => void): void {
  let start: { x: number; id: number } | undefined;
  card.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || (event.target instanceof Element && event.target.closest("button"))) return;
    start = { x: event.clientX, id: event.pointerId };
    card.setPointerCapture(event.pointerId);
    card.classList.add("is-dragging");
  });
  card.addEventListener("pointermove", (event) => {
    if (start?.id !== event.pointerId) return;
    const dx = Math.max(0, event.clientX - start.x);
    card.style.translate = `${dx}px 0`;
    card.style.opacity = String(Math.max(0.2, 1 - dx / (SWIPE * 2.2)));
  });
  const end = (event: PointerEvent) => {
    if (start?.id !== event.pointerId) return;
    const dx = event.clientX - start.x;
    start = undefined;
    card.classList.remove("is-dragging");
    card.style.translate = "";
    card.style.opacity = "";
    if (dx > SWIPE) dismiss();
  };
  card.addEventListener("pointerup", end);
  card.addEventListener("pointercancel", end);
}

/** The month as a small grid, today marked (Windows 11 shows it under the notifications). */
function month(now: Date): HTMLElement {
  const locale = navigator.language;
  const firstDay = firstDayOf(barsConfig().calendar.firstDay, locale);
  const names = Array.from({ length: 7 }, (_, i) =>
    new Date(2026, 1, 1 + ((firstDay + i) % 7)).toLocaleDateString(locale, { weekday: "narrow" }),
  ); // 1 February 2026 is a Sunday
  const grid = h(
    "div",
    "cal-grid notif-month-grid",
    ...names.map((n) => h("span", "cal-head", n)),
    ...monthGrid(now.getFullYear(), now.getMonth(), firstDay, now)
      .flat()
      .map((day) => {
        const el = h("span", "cal-day", String(day.date.getDate()));
        el.classList.toggle("is-out", !day.inMonth);
        el.classList.toggle("is-today", day.today);
        return el;
      }),
  );
  const title = h("button", "notif-month-title", now.toLocaleDateString(locale, { weekday: "long", day: "numeric", month: "long" }));
  title.addEventListener("click", () => fire("launch", { path: "outlookcal:" }));
  return h("section", "notif-month", title, grid);
}

export function notificationCenter(actions: CenterActions): { el: HTMLElement; update(state: NotificationList): void } {
  const list = h("div", "notif-list");
  const clear = button("notif-clear", "Clear all");
  clear.addEventListener("click", () => actions.clearAll());
  const dnd = button("notif-dnd", glyph(glyphs.bellOff), h("span", "notif-dnd-label", "Do not disturb"));
  dnd.title = "Do not disturb (opens its setting)";
  dnd.addEventListener("click", () => {
    fire("openSettings", { uri: "ms-settings:notifications" });
    actions.close();
  });
  void request("quick")
    .then((quick) => {
      const on = !!quick.dnd && quick.dnd !== "off";
      dnd.dataset.on = String(on);
      dnd.querySelector(".notif-dnd-label")!.textContent = on ? "Do not disturb is on" : "Do not disturb";
    })
    .catch(() => undefined);
  const monthSlot = h("div", "notif-month-slot");
  const el = h(
    "div",
    "fly-panel notif-panel",
    h("div", "fly-header notif-header", h("span", "fly-title", "Notifications"), h("span", "fly-actions", dnd, clear)),
    list,
    monthSlot,
    h(
      "div",
      "fly-footer",
      settingsLink("Notification settings", "ms-settings:notifications"),
      h("span", "fly-gap"),
      iconButton(glyphs.bell, "Windows' notification center", () => {
        fire("actionCenter", { panel: "notifications" });
        actions.close();
      }),
    ),
  );
  /** Apps the user opened: they stay open across the list's refreshes. */
  const opened = new Set<string>();

  const empty = (title: string, detail: string, action?: HTMLElement) =>
    h("div", "notif-empty", h("span", "notif-empty-glyph", glyph(glyphs.bell)), h("span", "notif-empty-title", title), h("span", "notif-empty-detail", detail), ...(action ? [action] : []));

  const card = (item: NotificationItem, group: Group, now: number) => {
    const leave = () => {
      el.classList.add("has-leaving");
      article.classList.add("is-leaving");
      actions.dismiss([item.id]);
    };
    const dismiss = iconButton(glyphs.close, "Dismiss", leave);
    dismiss.classList.add("notif-dismiss");
    const time = h("time", "notif-time", ago(item.at, now));
    time.title = new Date(item.at).toLocaleString();
    const article = h(
      "article",
      "notif-card",
      appBadge(group, "notif-icon"),
      h(
        "div",
        "notif-text",
        h("div", "notif-line", h("span", "notif-title", item.title || group.app), time),
        ...(item.body ? [h("div", "notif-body", item.body)] : []),
      ),
      dismiss,
    );
    swipeable(article, leave);
    return article;
  };

  const groupView = (group: Group, now: number) => {
    const many = group.items.length > 1;
    const open = !many || opened.has(group.app);
    const toggle = button("notif-group-toggle", h("span", "notif-count", many ? (open ? "Show less" : `${group.items.length}`) : ""));
    toggle.hidden = !many;
    toggle.title = open ? "Show less" : `Show all ${group.items.length}`;
    const clearGroup = iconButton(glyphs.close, `Clear ${group.app}`, () => {
      section.classList.add("is-leaving");
      actions.dismiss(group.items.map((i) => i.id));
    });
    clearGroup.classList.add("notif-group-clear");
    const stack = h("div", "notif-stack", ...(open ? group.items : group.items.slice(0, 1)).map((item) => card(item, group, now)));
    if (!open) {
      stack.dataset.more = String(Math.min(2, group.items.length - 1));
      stack.title = `Show all ${group.items.length}`;
      stack.addEventListener("click", (event) => {
        if (event.target instanceof Element && event.target.closest("button")) return;
        opened.add(group.app);
        render();
      });
    }
    toggle.addEventListener("click", () => {
      if (opened.has(group.app)) opened.delete(group.app);
      else opened.add(group.app);
      render();
    });
    const section = h(
      "section",
      "notif-group",
      h("div", "notif-group-head", appBadge(group, "notif-group-icon"), h("span", "notif-app", group.app || "App"), h("span", "fly-gap"), toggle, clearGroup),
      stack,
    );
    section.dataset.open = String(open);
    return section;
  };

  let state: NotificationList | undefined;
  const render = () => {
    if (!state) return;
    el.classList.remove("has-leaving");
    clear.hidden = state.notifications.length === 0;
    if (state.error === "denied") {
      list.replaceChildren(empty("Notifications are off for apps", "Allow access to show them here.", settingsLink("Turn them on", "ms-settings:privacy-notifications")));
    } else if (state.error) {
      list.replaceChildren(empty("Notifications can't be read here", "Windows' own notification center still has them.", settingsLink("Notification settings", "ms-settings:notifications")));
    } else if (state.notifications.length === 0) {
      list.replaceChildren(empty("No new notifications", "You're all caught up."));
    } else {
      const now = Date.now();
      list.replaceChildren(...byApp(state.notifications).map((group) => groupView(group, now)));
    }
    monthSlot.replaceChildren(month(new Date()));
  };

  list.append(empty("Loading…", ""));
  clear.hidden = true;
  monthSlot.append(month(new Date()));
  return {
    el,
    update(next) {
      state = next;
      render();
    },
  };
}
