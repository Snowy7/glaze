import { describe, expect, it } from "vitest";
import { denial } from "./permissions";

const none = {};
const full = {
  permissions: ["system.stats", "media", "launch", "theme", "audio", "user"] as const,
  plugins: ["weather"],
};

describe("denial", () => {
  it("allows requests that need nothing", () => {
    for (const method of ["hello", "storage.get", "storage.set", "unsubscribe", "pickFile", "something.new"]) {
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

  it("gates user.info on user, appIcon on launch and the audio topic on audio", () => {
    expect(denial(none, "user.info", undefined)).toMatch(/"user"/);
    expect(denial({ permissions: ["launch"] }, "user.info", undefined)).toMatch(/"user"/);
    expect(denial(full, "user.info", undefined)).toBeUndefined();

    expect(denial(none, "appIcon", { target: "calc.exe" })).toMatch(/"launch"/);
    expect(denial({ permissions: ["user"] }, "appIcon", { target: "calc.exe" })).toMatch(/"launch"/);
    expect(denial(full, "appIcon", { target: "calc.exe" })).toBeUndefined();

    const audio = { sub: "s1", topic: "audio", bins: 64 };
    expect(denial(none, "subscribe", audio)).toMatch(/"audio"/);
    expect(denial({ permissions: ["media"] }, "subscribe", audio)).toMatch(/"audio"/);
    expect(denial(full, "subscribe", audio)).toBeUndefined();
  });

  it("keeps instance storage open and gates a shared namespace on its own permission", () => {
    for (const method of ["storage.get", "storage.set"]) {
      expect(denial(none, method, { key: "a" })).toBeUndefined();
      expect(denial(none, method, { key: "a", shared: "activity" })).toMatch(/storage\.shared:activity/);
      const granted = { permissions: ["storage.shared:activity"] as const };
      expect(denial(granted, method, { key: "a", shared: "activity" })).toBeUndefined();
      expect(denial(granted, method, { key: "a", shared: "other" })).toMatch(/storage\.shared:other/);
      expect(denial(granted, method, { key: "a" })).toBeUndefined();
      expect(denial(full, method, { key: "a", shared: "activity" })).toBeDefined(); // other permissions don't count
      expect(denial(granted, method, { key: "a", shared: 3 })).toBeDefined();
    }
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
