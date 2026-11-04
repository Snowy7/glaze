import { expect, it } from "vitest";
import { byApp } from "./notification-center";
import type { NotificationItem } from "./protocol";

it("groups notification identities despite renamed apps, without merging equal display names", () => {
  const item = (id: number, app: string, aumid?: string): NotificationItem => ({ id, app, aumid, title: "", body: "", at: id });
  const groups = byApp([
    item(1, "T3 Code", "com.t3tools.t3code"),
    { ...item(3, "T3 Code (Nightly)", "com.t3tools.t3code"), icon: "https://icons.bars.mypaper/t3.png" },
    item(2, "T3 Code", "another.app"),
    item(4, "Legacy app"), item(5, "Legacy app"),
  ]);
  expect(groups.map((group) => group.items.map((toast) => toast.id))).toEqual([[5, 4], [3, 1], [2]]);
  expect(groups[1]?.icon).toBe("https://icons.bars.mypaper/t3.png");
  expect(new Set(groups.map((group) => group.id)).size).toBe(3);
});
