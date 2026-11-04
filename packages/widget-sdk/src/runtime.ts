// The `mypaper` object, implemented over the postMessage wire format (docs/widgets.md).
// Pure logic over a `Transport`, so it runs (and is tested) without a DOM.

import { DEFAULT_APPEARANCE } from "./appearance";
import { MypaperError } from "./errors";
import {
  AUDIO_BINS,
  WIRE_VERSION,
  type AppearanceState,
  type HelloResult,
  type MediaAction,
  type MediaState,
  type Mypaper,
  type Settings,
  type SizeInfo,
  type SubscribeParams,
  type SystemStats,
  type ThemeInfo,
  type WidgetEvents,
  type WireEvent,
  type WireMethods,
  type WireRequest,
  type WireResponse,
} from "./types";

/** What the shell sends to a widget. */
type Incoming = WireEvent | WireResponse;

/** How the runtime talks to the shell. */
export interface Transport {
  send(message: WireRequest): void;
  /** Calls `handler` with the data of every incoming message. Returns a stop function. */
  listen(handler: (data: unknown) => void): () => void;
}

export interface RuntimeOptions {
  /** How long to wait for the shell's handshake answer. */
  helloTimeoutMs?: number;
}

type Cb<T> = (value: T) => void;

interface Emitter<T> {
  on(cb: Cb<T>): () => void;
  emit(value: T): void;
}

function emitter<T>(): Emitter<T> {
  const set = new Set<Cb<T>>();
  return {
    on(cb) {
      // Wrap so the same callback can be registered twice, each with its own unsubscribe.
      const entry: Cb<T> = (v) => cb(v);
      set.add(entry);
      return () => void set.delete(entry);
    },
    emit(value) {
      for (const cb of [...set]) {
        try {
          cb(value);
        } catch (err) {
          console.error("[mypaper] listener threw", err);
        }
      }
    },
  };
}

type OmitEach<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** The bin count a spectrum subscription asks for: a whole number within the host's range. */
export function audioBins(requested: number | undefined): number {
  if (requested === undefined || !Number.isFinite(requested)) return AUDIO_BINS.default;
  return Math.min(AUDIO_BINS.max, Math.max(AUDIO_BINS.min, Math.round(requested)));
}

/** Validates the envelope only. Payload shapes are the shell's contract. */
function parseWire(data: unknown): Incoming | undefined {
  if (typeof data !== "object" || data === null) return undefined;
  if (!("mypaper" in data) || data.mypaper !== WIRE_VERSION) return undefined;
  // Our own requests echo back when the page isn't framed; they have `method`.
  if ("method" in data) return undefined;
  if ("event" in data || "id" in data) return data as Incoming;
  return undefined;
}

function isEvent(msg: Incoming): msg is WireEvent {
  return "event" in msg;
}

export function createMypaper(transport: Transport, options: RuntimeOptions = {}): Mypaper {
  const { helloTimeoutMs = 10_000 } = options;

  let nextId = 1;
  let nextSub = 1;
  const pending = new Map<number, Cb<WireResponse>>();
  const subs = new Map<string, Cb<unknown>>();

  const events = {
    visibility: emitter<boolean>(),
    editmode: emitter<boolean>(),
    settings: emitter<Settings>(),
    size: emitter<SizeInfo>(),
    appearance: emitter<AppearanceState>(),
  };

  let state: HelloResult | undefined;
  /** Replaced by the handshake's copy, then by every `appearance` event. */
  let look: AppearanceState = { appearance: DEFAULT_APPEARANCE, themeCss: null };
  let lookPushed = false;

  function requireState(what: string): HelloResult {
    if (!state) {
      throw new MypaperError("Internal", `mypaper.${what} is only available after \`await mypaper.ready\``);
    }
    return state;
  }

  // ------------------------------------------------------------ incoming

  transport.listen((data) => {
    const msg = parseWire(data);
    if (!msg) return;
    if (isEvent(msg)) return handleEvent(msg);
    const settle = pending.get(msg.id);
    if (settle) {
      pending.delete(msg.id);
      settle(msg);
    }
  });

  function handleEvent(msg: WireEvent): void {
    switch (msg.event) {
      case "visibility":
        if (state) state.visible = msg.data;
        return events.visibility.emit(msg.data);
      case "editmode":
        if (state) state.editing = msg.data;
        return events.editmode.emit(msg.data);
      case "settings":
        if (state) state.settings = msg.data;
        return events.settings.emit({ ...msg.data });
      case "size":
        if (state) state.size = msg.data;
        return events.size.emit(msg.data);
      case "appearance":
        look = msg.data;
        lookPushed = true;
        return events.appearance.emit(look);
      case "sub":
        return subs.get(msg.data.sub)?.(msg.data.value);
    }
  }

  // ------------------------------------------------------------ outgoing

  function rawCall<M extends keyof WireMethods>(
    method: M,
    params: WireMethods[M][0],
    timeoutMs?: number,
  ): Promise<WireMethods[M][1]> {
    return new Promise((resolve, reject) => {
      const id = nextId++;
      const timer =
        timeoutMs === undefined
          ? undefined
          : setTimeout(() => {
              pending.delete(id);
              reject(new MypaperError("Internal", `no answer from the mypaper host to "${method}"`));
            }, timeoutMs);
      pending.set(id, (res) => {
        clearTimeout(timer);
        if ("error" in res) reject(MypaperError.fromWire(res.error));
        // The shell owns the result shape for each method (WireMethods).
        else resolve(res.result as WireMethods[M][1]);
      });
      transport.send({ mypaper: WIRE_VERSION, id, method, params });
    });
  }

  const ready: Promise<void> = rawCall("hello", { sdk: WIRE_VERSION }, helloTimeoutMs).then((hello) => {
    state = hello;
    // An `appearance` event that raced ahead of the answer is newer than the handshake's copy.
    if (hello.appearance && !lookPushed) look = { appearance: hello.appearance, themeCss: hello.themeCss ?? null };
  });

  async function call<M extends keyof WireMethods>(
    method: M,
    params: WireMethods[M][0],
  ): Promise<WireMethods[M][1]> {
    await ready;
    return rawCall(method, params);
  }

  /** Subscribes after the handshake. Errors can't reject (the unsubscribe is sync), so they're logged. */
  function subscribe<T>(params: OmitEach<SubscribeParams, "sub">, cb: Cb<T>): () => void {
    const sub = `s${nextSub++}`;
    // The shell owns the payload type for each topic.
    subs.set(sub, (value) => cb(value as T));
    // Same promise chain for both calls keeps subscribe -> unsubscribe in order.
    void call("subscribe", { ...params, sub }).catch((err: unknown) => {
      subs.delete(sub);
      console.error(`[mypaper] subscribing to ${params.topic} failed`, err);
    });
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      subs.delete(sub);
      void call("unsubscribe", { sub }).catch(() => {});
    };
  }

  // ------------------------------------------------------------ api

  return {
    ready,
    get instance() {
      return requireState("instance").instance;
    },
    get widget() {
      return requireState("widget").widget;
    },
    get visible() {
      return state?.visible ?? true;
    },
    get editing() {
      return state?.editing ?? false;
    },

    settings: {
      async get() {
        await ready;
        return { ...requireState("settings").settings };
      },
      onChange: (cb) => events.settings.on(cb),
      async set(values) {
        await call("settings.set", { values });
      },
    },
    appearance: {
      async get() {
        await ready;
        return look;
      },
      onChange: (cb) => events.appearance.on(cb),
    },
    size: {
      async get() {
        await ready;
        return requireState("size").size;
      },
      onChange: (cb) => events.size.on(cb),
    },
    on: <E extends keyof WidgetEvents>(event: E, cb: Cb<WidgetEvents[E]>) => events[event].on(cb),

    storage: {
      async get<T>(key: string) {
        const value = await call("storage.get", { key });
        // `null` is how "missing" crosses JSON.
        return value === null ? undefined : (value as T);
      },
      async set(key, value) {
        await call("storage.set", { key, value });
      },
      shared: (shared: string) => ({
        async get<T>(key: string) {
          const value = await call("storage.get", { key, shared });
          return value === null ? undefined : (value as T);
        },
        async set(key: string, value: unknown) {
          await call("storage.set", { key, value, shared });
        },
      }),
    },

    system: {
      stats: () => call("system.stats", undefined),
      subscribe(intervalMs: number, cb: Cb<SystemStats>) {
        if (!Number.isFinite(intervalMs)) {
          throw new MypaperError("InvalidParams", "system.subscribe: intervalMs must be a number");
        }
        return subscribe({ topic: "system", intervalMs: Math.max(1000, Math.round(intervalMs)) }, cb);
      },
    },

    media: {
      current: () => call("media.current", undefined),
      onChange: (cb: Cb<MediaState | null>) => subscribe({ topic: "media" }, cb),
      async control(action: MediaAction) {
        await call("media.control", { action });
      },
    },

    async launch(target) {
      await call("launch", { target });
    },

    theme: {
      current: () => call("theme.current", undefined),
      onChange: (cb: Cb<ThemeInfo>) => subscribe({ topic: "theme" }, cb),
    },

    async plugin<T>(id: string, method: string, params?: unknown) {
      return (await call("plugin", { plugin: id, method, params })) as T;
    },
    onPlugin: (id, event, cb) => subscribe({ topic: "plugin", plugin: id, event }, cb),

    audio: {
      spectrum(cb: Cb<number[]>, opts: { bins?: number } = {}) {
        return subscribe({ topic: "audio", bins: audioBins(opts.bins) }, cb);
      },
    },
    user: { info: () => call("user.info", undefined) },
    pickFile: (opts = {}) => call("pickFile", opts),
    appIcon: (target, size) => call("appIcon", size === undefined ? { target } : { target, size }),
  };
}
