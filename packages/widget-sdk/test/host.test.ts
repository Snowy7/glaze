import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeHost, type Backend, type HostOutput } from "../dev/host";
import { DEFAULT_APPEARANCE, type WidgetManifest } from "../src";

const manifest = (overrides: Partial<WidgetManifest> = {}): WidgetManifest => ({
  id: "t",
  name: "T",
  version: "1.0.0",
  entry: "index.html",
  size: { default: [2, 1], min: [1, 1], max: [4, 2] },
  ...overrides,
});

const backend: Backend = {
  stats: () => ({ cpu: 1, memory: { used: 1, total: 2 }, uptime: 60 }),
  media: () => null,
  control: vi.fn(),
  theme: () => ({ name: "mono" }),
  launch: vi.fn(),
  plugin: async (id, method) => ({ id, method }),
  spectrum: (bins) => Array.from({ length: bins }, () => 0.5),
  user: () => ({ name: "Ada Lovelace", account: "ada" }),
  appIcon: (target, size) => `icon:${target}:${size}`,
  pickFile: async () => ({ url: "blob:x", name: "x.png" }),
  shared: (namespace) => {
    let store = sharedStores.get(namespace);
    if (!store) sharedStores.set(namespace, (store = new Map()));
    return store;
  },
};
const sharedStores = new Map<string, Map<string, unknown>>();

function setup(m: WidgetManifest) {
  const out: HostOutput[] = [];
  const host = new BridgeHost("t-1", m, backend, (msg) => out.push(msg), {
    settings: {},
    size: { cells: [2, 1], px: [200, 96], dpi: 96 },
    look: { appearance: DEFAULT_APPEARANCE, themeCss: null },
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

describe("shared storage", () => {
  it("is common to every instance, and needs storage.shared:<namespace>", async () => {
    const a = setup(manifest({ permissions: ["storage.shared:activity"] }));
    const b = setup(manifest({ id: "u", permissions: ["storage.shared:activity"] }));
    const nothing = setup(manifest());

    await a.call("storage.set", { key: "todo", value: 3, shared: "activity" });
    expect(await b.call("storage.get", { key: "todo", shared: "activity" })).toMatchObject({ result: 3 });
    expect(await b.call("storage.get", { key: "todo" })).toMatchObject({ result: null }); // instance storage is separate

    expect(await nothing.call("storage.get", { key: "todo", shared: "activity" })).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await a.call("storage.get", { key: "todo", shared: "other" })).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await a.call("storage.get", { key: "todo", shared: "Bad Name" })).toMatchObject({ error: { code: "InvalidParams" } });

    await a.call("storage.set", { key: "todo", shared: "activity" }); // no value: deletes
    expect(await b.call("storage.get", { key: "todo", shared: "activity" })).toMatchObject({ result: null });
  });
});

describe("user, icons and files", () => {
  it("gates user.info on user and appIcon on launch, and lets pickFile through", async () => {
    const { call } = setup(manifest());
    expect(await call("user.info")).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await call("appIcon", { target: "calc.exe" })).toMatchObject({ error: { code: "PermissionDenied" } });
    expect(await call("pickFile", { extensions: ["png"] })).toMatchObject({ result: { url: "blob:x", name: "x.png" } });

    const granted = setup(manifest({ permissions: ["user", "launch"] }));
    expect(await granted.call("user.info")).toMatchObject({ result: { name: "Ada Lovelace", account: "ada" } });
    expect(await granted.call("appIcon", { target: "calc.exe" })).toMatchObject({ result: "icon:calc.exe:64" });
    expect(await granted.call("appIcon", { target: "calc.exe", size: 32 })).toMatchObject({ result: "icon:calc.exe:32" });
    expect(await granted.call("appIcon", { target: "" })).toMatchObject({ error: { code: "InvalidParams" } });
  });
});

describe("audio subscriptions", () => {
  const frames = (out: HostOutput[]) => out.filter((m) => "event" in m && m.event === "sub");
  /** How many bins the first frame for `sub` has. */
  const sizeOf = (out: HostOutput[], sub: string): number | undefined => {
    for (const m of out) {
      if ("event" in m && m.event === "sub" && m.data.sub === sub && Array.isArray(m.data.value)) return m.data.value.length;
    }
    return undefined;
  };

  it("needs the audio permission", async () => {
    const { call } = setup(manifest());
    expect(await call("subscribe", { sub: "a", topic: "audio" })).toMatchObject({ error: { code: "PermissionDenied" } });
  });

  it("sends ~30 frames a second, only while visible, until unsubscribed", async () => {
    const { host, out, call } = setup(manifest({ permissions: ["audio"] }));
    await call("subscribe", { sub: "a", topic: "audio", bins: 96 });
    vi.advanceTimersByTime(1000);
    expect(frames(out).length).toBeGreaterThanOrEqual(29);
    expect(frames(out).length).toBeLessThanOrEqual(31);
    expect(sizeOf(out, "a")).toBe(96);

    host.setVisible(false);
    const before = frames(out).length;
    vi.advanceTimersByTime(1000);
    expect(frames(out)).toHaveLength(before);

    host.setVisible(true);
    vi.advanceTimersByTime(100);
    expect(frames(out).length).toBeGreaterThan(before);

    await call("unsubscribe", { sub: "a" });
    const after = frames(out).length;
    vi.advanceTimersByTime(1000);
    expect(frames(out)).toHaveLength(after);
  });

  it("clamps bins to 64..128 and defaults to 64", async () => {
    const { out, call } = setup(manifest({ permissions: ["audio"] }));
    await call("subscribe", { sub: "a", topic: "audio" });
    await call("subscribe", { sub: "b", topic: "audio", bins: 1000 });
    await call("subscribe", { sub: "c", topic: "audio", bins: 3 });
    vi.advanceTimersByTime(40);
    expect(["a", "b", "c"].map((sub) => sizeOf(out, sub))).toEqual([64, 128, 64]);
  });
});

describe("appearance", () => {
  it("hands the look over in the handshake and pushes changes", async () => {
    const { host, out, call } = setup(manifest());
    expect(await call("hello")).toMatchObject({ result: { appearance: DEFAULT_APPEARANCE, themeCss: null } });
    const next = { appearance: { ...DEFAULT_APPEARANCE, surface: "glass" as const }, themeCss: "a{}" };
    host.setAppearance(next);
    expect(out.at(-1)).toEqual({ mypaper: 1, event: "appearance", data: next });
    expect(await call("hello")).toMatchObject({ result: { appearance: next.appearance, themeCss: "a{}" } });
  });
});
