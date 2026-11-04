import { describe, expect, it } from "vitest";
import type { DockItem } from "../protocol";
import { applyDrop, dividerIndex, indicator, orderItems } from "./model";

const win = (hwnd: number, active = false) => ({ hwnd, title: `w${hwnd}`, active, minimized: false });
const item = (key: string, pinned: boolean, windows: DockItem["windows"] = []): DockItem => ({
  key,
  name: key,
  path: `${key}.exe`,
  pinned,
  windows,
});
const keys = (items: DockItem[]) => items.map((i) => i.key);

describe("grouping", () => {
  it("puts pinned items first and keeps the order within each group", () => {
    const items = [item("x", false), item("a", true), item("y", false), item("b", true)];
    expect(keys(orderItems(items))).toEqual(["a", "b", "x", "y"]);
  });

  it("places the divider between the groups only", () => {
    expect(dividerIndex([item("a", true), item("b", true), item("x", false)])).toBe(2);
    expect(dividerIndex([item("a", true)])).toBeNull();
    expect(dividerIndex([item("x", false)])).toBeNull();
    expect(dividerIndex([])).toBeNull();
  });

  it("tells running from active", () => {
    expect(indicator(item("a", true))).toBe("none");
    expect(indicator(item("a", true, [win(1)]))).toBe("running");
    expect(indicator(item("a", true, [win(1), win(2, true)]))).toBe("active");
  });
});

describe("applyDrop", () => {
  const list = [item("a", true), item("b", true, [win(1)]), item("c", true), item("x", false, [win(2)])];

  it("reorders the pinned group", () => {
    const next = applyDrop(list, { type: "reorder", paths: ["c.exe", "a.exe", "b.exe"] });
    expect(keys(next)).toEqual(["c", "a", "b", "x"]);
  });

  it("pins a running item at the index", () => {
    const next = applyDrop(list, { type: "pin", path: "x.exe", name: "x", index: 1 });
    expect(keys(next)).toEqual(["a", "x", "b", "c"]);
    expect(next[1]?.pinned).toBe(true);
  });

  it("unpins: a running app moves to the running group, a closed one disappears", () => {
    const running = applyDrop(list, { type: "unpin", path: "b.exe" });
    expect(keys(running)).toEqual(["a", "c", "b", "x"]);
    expect(running[2]?.pinned).toBe(false);
    expect(keys(applyDrop(list, { type: "unpin", path: "a.exe" }))).toEqual(["b", "c", "x"]);
  });
});
