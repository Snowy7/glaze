import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeHost, type Backend, type HostOutput } from "../dev/host";
import type { WidgetManifest } from "../src";

const manifest = (overrides: Partial<WidgetManifest> = {}): WidgetManifest => ({
  id: "t",
  name: "T",
  version: "1.0.0",
  entry: "index.html",
  size: { default: [2, 1], min: [1, 1], max: [4, 2] },
  ...overrides,
});

const backend: Backend = {
  stats: () => ({ cpu: 1, memory: { used: 1, total: 2 } }),
  media: () => null,
  control: vi.fn(),
  theme: () => ({ name: "mono" }),
  launch: vi.fn(),
  plugin: async (id, method) => ({ id, method }),
};

function setup(m: WidgetManifest) {
  const out: HostOutput[] = [];
  const host = new BridgeHost("t-1", m, backend, (msg) => out.push(msg), {
    settings: {},
    size: { cells: [2, 1], px: [200, 96], dpi: 96 },
  });
  const call = async (method: string, params?: unknown) => {
    const id = out.length + 1000;
    await host.handle({ mypaper: 1, id, method, params });
    return out.findLast((m) => "id" in m && m.id === id);
  };
  return { host, out, call };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("permissions come from the manifest", () => {
  it("denies capabilities the manifest doesn't declare, and allows declared ones", async () => {
    const { call } = setup(manifest({ permissions: ["launch"] }));
    expect(await call("system.stats")).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await call("media.current")).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await call("theme.current")).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await call("launch", { target: "notepad.exe" })).toMatchObject({ result: null });
  });

  it("only lets a widget call plugins it lists", async () => {
    const { call } = setup(manifest({ plugins: ["weather"] }));
    expect(await call("plugin", { plugin: "weather", method: "current" })).toMatchObject({
      result: { id: "weather", method: "current" },
    });
    expect(await call("plugin", { plugin: "other", method: "x" })).toMatchObject({
      error: { code: "PermissionDenied" },
    });
    expect(await call("subscribe", { sub: "s1", topic: "plugin", plugin: "other", event: "e" })).toMatchObject({
      error: { code: "PermissionDenied" },
    });
  });

  it("rejects unknown methods and ignores messages that aren't requests", async () => {
    const { call, host, out } = setup(manifest());
    expect(await call("nope")).toMatchObject({ error: { code: "NotFound" } });
    const before = out.length;
    await host.handle({ hello: 1 });
    await host.handle({ mypaper: 2, id: 1, method: "hello" });
    expect(out).toHaveLength(before);
  });
});

describe("storage", () => {
  it("round-trips values and reports a missing key as null", async () => {
    const { call } = setup(manifest());
    expect(await call("storage.get", { key: "a" })).toMatchObject({ result: null });
    await call("storage.set", { key: "a", value: { n: 1 } });
    expect(await call("storage.get", { key: "a" })).toMatchObject({ result: { n: 1 } });
  });
});

describe("system subscriptions", () => {
  const subs = (out: HostOutput[]) => out.filter((m) => "event" in m && m.event === "sub");

  it("pushes on the interval (never faster than 1 s), pauses while hidden, resumes, and stops on unsubscribe", async () => {
    const { host, out, call } = setup(manifest({ permissions: ["system.stats"] }));
    await call("subscribe", { sub: "s1", topic: "system", intervalMs: 100 });
    expect(subs(out)).toHaveLength(1); // immediate first sample

    vi.advanceTimersByTime(3000);
    expect(subs(out)).toHaveLength(4); // 1 s floor, not 100 ms

    host.setVisible(false);
    vi.advanceTimersByTime(5000);
    expect(subs(out)).toHaveLength(4);
    expect(out.at(-1)).toMatchObject({ event: "visibility", data: false });

    host.setVisible(true);
    expect(subs(out)).toHaveLength(5);

    await call("unsubscribe", { sub: "s1" });
    vi.advanceTimersByTime(5000);
    expect(subs(out)).toHaveLength(5);
  });
});
