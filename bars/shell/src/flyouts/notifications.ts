// Notifications (docs/bars.md, "Flyouts" → "Notifications"): every toast in the notification
// center, grouped by app, newest first, to dismiss one by one or all at once. The host reads them
// through UserNotificationListener (no change events without package identity, so the list is
// re-read every few seconds while this is open).

import { button, h } from "../dom";
import { defineFlyout } from "../flyout";
import { fire, request } from "../host";
import { glyphs } from "./glyphs";
import { ago } from "./model";
import type { NotificationItem, NotificationList } from "./protocol";
import { show } from "./surface";
import { appIcon, header, iconButton, notice, settingsLink } from "./ui";

export async function open(anchor: HTMLElement, keyboard = false): Promise<void> {
  await show(notifications, null, { anchor, align: "end", extent: 600, keyboard });
}

/** Toasts grouped by app, the apps in the order of their newest toast. */
function byApp(list: readonly NotificationItem[]): { app: string; icon?: string; items: NotificationItem[] }[] {
  const groups = new Map<string, { app: string; icon?: string; items: NotificationItem[] }>();
  for (const item of list) {
    const group = groups.get(item.app) ?? { app: item.app, ...(item.icon ? { icon: item.icon } : {}), items: [] };
    group.items.push(item);
    groups.set(item.app, group);
  }
  return [...groups.values()];
}

const notifications = defineFlyout<null>("notifications", (_props, ctx) => {
  const list = h("div", "notif-list");
  const clear = button("fly-link", "Clear all");
  clear.hidden = true;
  const root = h(
    "div",
    "fly-panel notif-panel",
    header("Notifications", clear),
    list,
    h(
      "div",
      "fly-footer",
      settingsLink("Notification settings", "ms-settings:notifications"),
      h("span", "fly-gap"),
      iconButton(glyphs.bell, "Open the notification center", () => {
        fire("actionCenter", { panel: "notifications" });
        ctx.close();
      }),
    ),
  );

  const apply = (state: NotificationList) => {
    clear.hidden = state.notifications.length === 0;
    if (state.error === "denied") {
      list.replaceChildren(notice("Notification access is off for apps.", settingsLink("Turn it on", "ms-settings:privacy-notifications")));
    } else if (state.error) {
      list.replaceChildren(notice("Notifications can't be read here.", settingsLink("Notification settings", "ms-settings:notifications")));
    } else if (state.notifications.length === 0) {
      list.replaceChildren(notice("No new notifications"));
    } else {
      const now = Date.now();
      list.replaceChildren(
        ...byApp(state.notifications).map((group) =>
          h(
            "section",
            "notif-group",
            h("div", "notif-app", appIcon(group.icon, group.app, "fly-icon notif-app-icon"), h("span", "notif-app-name", group.app || "App")),
            ...group.items.map((item) => {
              const dismiss = iconButton(glyphs.close, "Dismiss", () => {
                el.classList.add("is-leaving");
                request("dismissNotification", { id: item.id })
                  .then(apply)
                  .catch((error: unknown) => console.warn("[bars] dismiss failed", error));
              });
              const el = h(
                "article",
                "notif-item",
                h("div", "notif-head", h("span", "notif-title", item.title || group.app), h("span", "notif-time", ago(item.at, now)), dismiss),
                ...(item.body ? [h("div", "notif-body", item.body)] : []),
              );
              return el;
            }),
          ),
        ),
      );
    }
    ctx.reposition();
  };

  clear.addEventListener("click", () => {
    request("clearNotifications")
      .then(apply)
      .catch((error: unknown) => console.warn("[bars] clear failed", error));
  });
  request("notifications")
    .then(apply)
    .catch((error: unknown) => console.warn("[bars] notifications failed", error));
  ctx.scope.on("notificationList", apply);
  fire("watch", { topic: "notificationList" });
  ctx.scope.add(() => fire("unwatch", { topic: "notificationList" }));
  list.append(notice("Loading…"));
  return root;
});
