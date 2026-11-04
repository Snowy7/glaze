// Notifications (docs/bars.md, "Flyouts" → "Notifications"): every toast in the notification
// center, grouped by app, newest first, to dismiss one by one or all at once. The host reads them
// through UserNotificationListener (no change events without package identity, so the list is
// re-read every few seconds while this is open).

import { defineFlyout } from "../flyout";
import { fire, request } from "../host";
import { notificationCenter } from "./notification-center";
import type { NotificationList } from "./protocol";
import { show } from "./surface";

export async function open(anchor: HTMLElement, keyboard = false): Promise<void> {
  await show(notifications, null, { anchor, align: "end", extent: 860, keyboard });
}

const notifications = defineFlyout<null>("notifications", (_props, ctx) => {
  // The view (grouping, stacks, icons, swipe, empty states) is notification-center.ts.
  const center = notificationCenter({
    dismiss: (ids) => {
      // One at a time: the listener has no batch removal.
      ids
        .reduce<Promise<unknown>>((done, id) => done.then(() => request("dismissNotification", { id }).then(apply)), Promise.resolve())
        .catch((error: unknown) => console.warn("[bars] dismiss failed", error));
    },
    clearAll: () => {
      request("clearNotifications")
        .then(apply)
        .catch((error: unknown) => console.warn("[bars] clear failed", error));
    },
    close: () => ctx.close(),
  });

  function apply(state: NotificationList) {
    center.update(state);
    ctx.reposition();
  }

  request("notifications")
    .then(apply)
    .catch((error: unknown) => console.warn("[bars] notifications failed", error));
  ctx.scope.on("notificationList", apply);
  fire("watch", { topic: "notificationList" });
  ctx.scope.add(() => fire("unwatch", { topic: "notificationList" }));
  return center.el;
});
