import { describe, expect, it } from "vitest";
import { denial } from "./permissions";

const none = {};
const full = {
  permissions: ["system.stats", "media", "launch", "theme"] as const,
  plugins: ["weather"],
};

describe("denial", () => {
  it("allows requests that need nothing", () => {
    for (const method of ["hello", "storage.get", "storage.set", "unsubscribe", "something.new"]) {
      expect(denial(none, method, undefined)).toBeUndefined();
    }
  });

  it("denies each capability method without its permission", () => {
    expect(denial(none, "system.stats", undefined)).toMatch(/system\.stats/);
    expect(denial(none, "media.current", undefined)).toMatch(/media/);
    expect(denial(none, "media.control", { action: "play" })).toMatch(/media/);
    expect(denial(none, "launch", { target: "calc.exe" })).toMatch(/launch/);
    expect(denial(none, "theme.current", undefined)).toMatch(/theme/);
  });

  it("allows them with the permission", () => {
    for (const method of ["system.stats", "media.current", "media.control", "launch", "theme.current"]) {
      expect(denial(full, method, {})).toBeUndefined();
    }
  });

  it("only accepts the permission it names", () => {
    expect(denial({ permissions: ["media"] }, "launch", {})).toBeDefined();
    expect(denial({ permissions: ["network:https://example.com"] }, "system.stats", undefined)).toBeDefined();
  });

  it("checks the plugin id for plugin calls", () => {
    expect(denial(full, "plugin", { plugin: "weather", method: "current" })).toBeUndefined();
    expect(denial(full, "plugin", { plugin: "other", method: "x" })).toMatch(/other/);
    expect(denial(none, "plugin", { plugin: "weather", method: "x" })).toBeDefined();
    expect(denial(full, "plugin", undefined)).toBeDefined();
    expect(denial(full, "plugin", { plugin: 3 })).toBeDefined();
  });

  it("checks subscriptions by topic", () => {
    expect(denial(none, "subscribe", { sub: "s1", topic: "system", intervalMs: 1000 })).toMatch(/system\.stats/);
    expect(denial(none, "subscribe", { sub: "s1", topic: "media" })).toMatch(/media/);
    expect(denial(none, "subscribe", { sub: "s1", topic: "theme" })).toMatch(/theme/);
    expect(denial(full, "subscribe", { sub: "s1", topic: "system", intervalMs: 1000 })).toBeUndefined();
    expect(denial(full, "subscribe", { sub: "s1", topic: "media" })).toBeUndefined();
    expect(denial(full, "subscribe", { sub: "s1", topic: "theme" })).toBeUndefined();
  });

  it("checks the plugin of a plugin subscription", () => {
    const ok = { sub: "s1", topic: "plugin", plugin: "weather", event: "updated" };
    expect(denial(full, "subscribe", ok)).toBeUndefined();
    expect(denial(full, "subscribe", { ...ok, plugin: "other" })).toBeDefined();
    expect(denial(none, "subscribe", ok)).toBeDefined();
    expect(denial(full, "subscribe", { sub: "s1", topic: "plugin" })).toBeDefined();
  });

  it("leaves malformed or unknown subscriptions to the host", () => {
    expect(denial(none, "subscribe", undefined)).toBeUndefined();
    expect(denial(none, "subscribe", { sub: "s1", topic: "nope" })).toBeUndefined();
  });
});
