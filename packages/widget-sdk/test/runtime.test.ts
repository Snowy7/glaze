import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_APPEARANCE,
  MypaperError,
  createMypaper,
  isMypaperError,
  type AppearanceState,
  type HelloResult,
  type Transport,
  type WireRequest,
} from "../src";

const hello: HelloResult = {
  instance: "clock-1",
  widget: {
    id: "clock",
    name: "Clock",
    version: "1.0.0",
    entry: "index.html",
    size: { default: [2, 1], min: [1, 1], max: [4, 2] },
  },
  settings: { format24: true },
  size: { cells: [2, 1], px: [200, 96], dpi: 96 },
  visible: true,
  editing: false,
};

/** A fake shell: records what the widget sends and lets the test answer or push events. */
function setup(options: { answerHello?: boolean; helloTimeoutMs?: number } = {}) {
  const { answerHello = true, helloTimeoutMs } = options;
  const sent: WireRequest[] = [];
  let handler: (data: unknown) => void = () => {};
  const transport: Transport = {
    send(message) {
      sent.push(message);
    },
    listen(h) {
      handler = h;
      return () => {};
    },
  };
  const mypaper = createMypaper(transport, helloTimeoutMs === undefined ? {} : { helloTimeoutMs });
  const shell = {
    sent,
    last: (method: string) => sent.findLast((m) => m.method === method),
    result: (id: number, result: unknown) => handler({ mypaper: 1, id, result }),
    error: (id: number, code: string, message = "nope") =>
      handler({ mypaper: 1, id, error: { code, message } }),
    event: (event: string, data: unknown) => handler({ mypaper: 1, event, data }),
    raw: (data: unknown) => handler(data),
  };
  if (answerHello) shell.result(sent[0]?.id ?? 1, hello);
  return { mypaper, shell };
}

afterEach(() => vi.useRealTimers());

describe("handshake", () => {
  it("exposes instance and manifest after ready, and throws before", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    expect(() => mypaper.instance).toThrow(/ready/);
    shell.result(1, hello);
    await mypaper.ready;
    expect(mypaper.instance).toBe("clock-1");
    expect(mypaper.widget.id).toBe("clock");
    expect(await mypaper.settings.get()).toEqual({ format24: true });
    expect((await mypaper.size.get()).px).toEqual([200, 96]);
  });

  it("holds calls until the handshake is answered", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    const stats = mypaper.system.stats();
    expect(shell.sent.map((m) => m.method)).toEqual(["hello"]);
    shell.result(1, hello);
    await mypaper.ready;
    await Promise.resolve();
    expect(shell.sent.map((m) => m.method)).toEqual(["hello", "system.stats"]);
    shell.result(2, { cpu: 10, memory: { used: 1, total: 2 } });
    await expect(stats).resolves.toMatchObject({ cpu: 10 });
  });

  it("rejects ready when the host never answers", async () => {
    vi.useFakeTimers();
    const { mypaper } = setup({ answerHello: false, helloTimeoutMs: 500 });
    const failed = expect(mypaper.ready).rejects.toThrow(/no answer/);
    await vi.advanceTimersByTimeAsync(500);
    await failed;
  });
});

describe("calls", () => {
  it("correlates out-of-order responses by id", async () => {
    const { mypaper, shell } = setup();
    const a = mypaper.storage.get<string>("a");
    const b = mypaper.storage.get<string>("b");
    await mypaper.ready;
    await Promise.resolve();
    const [reqA, reqB] = shell.sent.filter((m) => m.method === "storage.get");
    expect(reqA?.params).toEqual({ key: "a" });
    expect(reqB?.params).toEqual({ key: "b" });
    shell.result(reqB?.id ?? -1, "B");
    shell.result(reqA?.id ?? -1, "A");
    expect(await a).toBe("A");
    expect(await b).toBe("B");
  });

  it("maps a null storage result to undefined", async () => {
    const { mypaper, shell } = setup();
    const value = mypaper.storage.get("missing");
    await mypaper.ready;
    await Promise.resolve();
    shell.result(shell.last("storage.get")?.id ?? -1, null);
    expect(await value).toBeUndefined();
  });

  it("rejects with a typed MypaperError carrying the wire code", async () => {
    const { mypaper, shell } = setup();
    const call = mypaper.launch("C:\\x");
    await mypaper.ready;
    await Promise.resolve();
    shell.error(shell.last("launch")?.id ?? -1, "PermissionDenied", "launch not declared");
    const err = await call.catch((e: unknown) => e);
    expect(isMypaperError(err, "PermissionDenied")).toBe(true);
    expect(isMypaperError(err, "NotFound")).toBe(false);
    expect(err).toMatchObject({ message: "launch not declared" });
  });

  it("maps an unknown error code to Internal", () => {
    expect(MypaperError.fromWire({ code: "Boom", message: "x" }).code).toBe("Internal");
  });

  it("forwards plugin id, method and params", async () => {
    const { mypaper, shell } = setup();
    const call = mypaper.plugin<{ t: number }>("weather", "current", { city: "Oslo" });
    await mypaper.ready;
    await Promise.resolve();
    const req = shell.last("plugin");
    expect(req?.params).toEqual({ plugin: "weather", method: "current", params: { city: "Oslo" } });
    shell.result(req?.id ?? -1, { t: 3 });
    expect(await call).toEqual({ t: 3 });
  });

  it("ignores messages that aren't ours, including echoes of our own requests", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    shell.raw({ id: 1, result: hello });
    shell.raw({ mypaper: 2, id: 1, result: hello });
    shell.raw(shell.sent[0]);
    shell.raw("hello");
    shell.raw(null);
    expect(() => mypaper.instance).toThrow();
    shell.result(1, hello);
    await mypaper.ready;
    expect(mypaper.instance).toBe("clock-1");
  });
});

describe("events", () => {
  it("delivers visibility and editmode, tracks the current value, and unsubscribes", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const seen: boolean[] = [];
    const off = mypaper.on("visibility", (v) => seen.push(v));
    expect(mypaper.visible).toBe(true);

    shell.event("visibility", false);
    expect(mypaper.visible).toBe(false);
    off();
    shell.event("visibility", true);
    expect(seen).toEqual([false]);

    shell.event("editmode", true);
    expect(mypaper.editing).toBe(true);
  });

  it("pushes settings and size changes and keeps get() current", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const sizes: number[] = [];
    mypaper.size.onChange((s) => sizes.push(s.px[0]));
    const settings: unknown[] = [];
    mypaper.settings.onChange((s) => settings.push(s));

    shell.event("settings", { format24: false });
    shell.event("size", { cells: [4, 2], px: [416, 200], dpi: 96 });

    expect(settings).toEqual([{ format24: false }]);
    expect(sizes).toEqual([416]);
    expect(await mypaper.settings.get()).toEqual({ format24: false });
    expect((await mypaper.size.get()).cells).toEqual([4, 2]);
  });

  it("one throwing listener doesn't stop the others", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const ok = vi.fn();
    mypaper.on("editmode", () => {
      throw new Error("boom");
    });
    mypaper.on("editmode", ok);
    shell.event("editmode", true);
    expect(ok).toHaveBeenCalledWith(true);
    spy.mockRestore();
  });
});

describe("subscriptions", () => {
  it("subscribes, routes by sub id, and stops after unsubscribe", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const fast = vi.fn();
    const slow = vi.fn();
    const offFast = mypaper.system.subscribe(500, fast); // clamped to 1000
    mypaper.system.subscribe(5000, slow);
    await Promise.resolve();

    const subs = shell.sent.filter((m) => m.method === "subscribe");
    expect(subs.map((m) => m.params)).toEqual([
      { topic: "system", intervalMs: 1000, sub: "s1" },
      { topic: "system", intervalMs: 5000, sub: "s2" },
    ]);

    const stats = { cpu: 5, memory: { used: 1, total: 2 } };
    shell.event("sub", { sub: "s1", value: stats });
    expect(fast).toHaveBeenCalledWith(stats);
    expect(slow).not.toHaveBeenCalled();

    offFast();
    offFast(); // idempotent
    await Promise.resolve();
    shell.event("sub", { sub: "s1", value: stats });
    expect(fast).toHaveBeenCalledTimes(1);
    expect(shell.sent.filter((m) => m.method === "unsubscribe")).toHaveLength(1);
    expect(shell.last("unsubscribe")?.params).toEqual({ sub: "s1" });
  });

  it("sends subscribe before unsubscribe even when unsubscribed immediately", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    const off = mypaper.media.onChange(() => {});
    off();
    shell.result(1, hello);
    await mypaper.ready;
    await Promise.resolve();
    await Promise.resolve();
    expect(shell.sent.map((m) => m.method)).toEqual(["hello", "subscribe", "unsubscribe"]);
  });

  it("drops the callback and logs when the host denies the subscription", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const cb = vi.fn();
    mypaper.theme.onChange(cb);
    await Promise.resolve();
    shell.error(shell.last("subscribe")?.id ?? -1, "PermissionDenied");
    await vi.waitFor(() => expect(spy).toHaveBeenCalled());
    shell.event("sub", { sub: "s1", value: { name: "x" } });
    expect(cb).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it("rejects a non-numeric interval synchronously", async () => {
    const { mypaper } = setup();
    expect(() => mypaper.system.subscribe(Number.NaN, () => {})).toThrow(MypaperError);
  });
});

describe("appearance", () => {
  const look = { ...DEFAULT_APPEARANCE, surface: "glass" as const, text: "dark" as const };

  it("starts with the defaults and no theme CSS when the host sends none", async () => {
    const { mypaper } = setup();
    expect(await mypaper.appearance.get()).toEqual({ appearance: DEFAULT_APPEARANCE, themeCss: null });
  });

  it("reads appearance and themeCss from the handshake", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    shell.result(1, { ...hello, appearance: look, themeCss: "html { --x: 1 }", plate: true });
    expect(await mypaper.appearance.get()).toEqual({ appearance: look, themeCss: "html { --x: 1 }", plate: true });
  });

  it("pushes the appearance event to listeners and keeps get() current", async () => {
    const { mypaper, shell } = setup();
    await mypaper.ready;
    const seen: AppearanceState[] = [];
    const off = mypaper.appearance.onChange((s) => seen.push(s));
    const next = { appearance: look, themeCss: null };
    shell.event("appearance", next);
    expect(seen).toEqual([next]);
    expect(await mypaper.appearance.get()).toEqual(next);
    off();
    shell.event("appearance", { appearance: DEFAULT_APPEARANCE, themeCss: "x" });
    expect(seen).toHaveLength(1);
  });

  it("prefers an appearance event that raced ahead of the handshake answer", async () => {
    const { mypaper, shell } = setup({ answerHello: false });
    const newer = { appearance: look, themeCss: "newer" };
    shell.event("appearance", newer);
    shell.result(1, { ...hello, appearance: DEFAULT_APPEARANCE, themeCss: "older" });
    expect(await mypaper.appearance.get()).toEqual(newer);
  });
});
