// dist/bridge.js: defines the global `mypaper` inside a widget iframe. The host injects it into every
// frame at document creation, so this must stay a self-contained classic script (no imports at
// runtime; `import type` is erased). Wire format: docs/widgets.md, "Widget JS API".

import type { Mypaper, WidgetManifest } from "./protocol";

(() => {
  const SHELL_ORIGIN = "https://shell.mypaper";

  // WebRTC traffic bypasses the host's network filter, so it goes in every frame (including frames
  // nested in a widget, which don't get the API).
  for (const name of ["RTCPeerConnection", "webkitRTCPeerConnection", "RTCDataChannel", "RTCSessionDescription", "RTCIceCandidate"]) {
    try {
      Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false });
    } catch {
      // Already locked down: nothing to do.
    }
  }

  const isWidgetFrame =
    location.hostname.endsWith(".widget.mypaper") && window.parent !== window && window.parent === window.top;
  if (!isWidgetFrame) return;

  let identity: { instance: string; widget: WidgetManifest };
  try {
    identity = JSON.parse(window.name);
    if (typeof identity.instance !== "string" || typeof identity.widget !== "object") throw new Error();
  } catch {
    console.error("mypaper: the frame has no instance info in its name");
    return;
  }

  // ---- transport ----

  type Listener = (data: unknown) => void;
  const pending = new Map<number | string, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  const listeners = new Map<string, Set<Listener>>();
  const lastValue = new Map<string, unknown>(); // last editmode / visibility
  let nextId = 1;

  const post = (message: Record<string, unknown>) =>
    window.parent.postMessage({ mypaper: 1, ...message }, SHELL_ORIGIN);

  function call<T = unknown>(method: string, params?: Record<string, unknown>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve: (v) => resolve(v as T), reject });
      try {
        post(params ? { id, method, params } : { id, method });
      } catch (e) {
        pending.delete(id);
        throw e;
      }
    });
  }

  /** Fire-and-forget (subscriptions): a refusal is logged, not thrown into the widget. */
  function send(method: string, params?: Record<string, unknown>) {
    call(method, params).catch((e: Error) => console.warn(`mypaper: ${method} failed: ${e.message}`));
  }

  function wireError(code: string, message: string): Error {
    return Object.assign(new Error(message), { name: code, code });
  }

  function emit(key: string, data: unknown) {
    for (const cb of [...(listeners.get(key) ?? [])]) {
      try {
        cb(data);
      } catch (e) {
        console.error(e);
      }
    }
  }

  window.addEventListener("message", (e: MessageEvent<unknown>) => {
    if (e.source !== window.parent || e.origin !== SHELL_ORIGIN) return;
    const m = e.data;
    if (typeof m !== "object" || m === null || !("mypaper" in m) || m.mypaper !== 1) return;

    if ("event" in m && typeof m.event === "string") {
      const data = "data" in m ? m.data : undefined;
      if (m.event === "plugin" && typeof data === "object" && data !== null && "plugin" in data && "event" in data) {
        emit(`plugin:${String(data.plugin)}:${String(data.event)}`, "data" in data ? data.data : undefined);
      } else if (m.event === "system.stats" && typeof data === "object" && data !== null && "sub" in data) {
        emit(`system:${String(data.sub)}`, "stats" in data ? data.stats : undefined);
      } else {
        if (m.event === "editmode" || m.event === "visibility") lastValue.set(m.event, data);
        emit(m.event, data);
      }
      return;
    }

    if ("id" in m && (typeof m.id === "number" || typeof m.id === "string")) {
      const request = pending.get(m.id);
      if (!request) return;
      pending.delete(m.id);
      const error = "error" in m ? m.error : undefined;
      if (typeof error === "object" && error !== null && "code" in error) {
        request.reject(wireError(String(error.code), "message" in error ? String(error.message) : String(error.code)));
      } else {
        request.resolve("result" in m ? m.result : null);
      }
    }
  });

  // ---- subscriptions ----

  /** Adds a local listener; `release` runs once when it is removed. Returns the remover. */
  function listen<T>(key: string, cb: (data: T) => void, acquire?: () => void, release?: () => void): () => void {
    const listener: Listener = (data) => cb(data as T);
    let set = listeners.get(key);
    if (!set) listeners.set(key, (set = new Set()));
    set.add(listener);
    if (set.size === 1) acquire?.();
    if (lastValue.has(key)) {
      queueMicrotask(() => {
        if (listeners.get(key)?.has(listener)) listener(lastValue.get(key));
      });
    }
    return () => {
      const current = listeners.get(key);
      if (!current?.delete(listener)) return;
      if (current.size === 0) {
        listeners.delete(key);
        release?.();
      }
    };
  }

  const subscribeWire = (topic: string, params?: Record<string, unknown>) => () => send(`${topic}.subscribe`, params);
  const unsubscribeWire = (topic: string, params?: Record<string, unknown>) => () => send(`${topic}.unsubscribe`, params);

  // ---- the API ----

  const settings: Mypaper["settings"] = {
    get: () => call("settings.get"),
    onChange: (cb) => listen("settings", cb),
  };

  const size: Mypaper["size"] = {
    get: () => call("size.get"),
    onChange: (cb) => listen("size", cb),
  };

  const storage: Mypaper["storage"] = {
    async get<T>(key: string) {
      return ((await call("storage.get", { key })) ?? undefined) as T | undefined;
    },
    async set(key, value) {
      await call("storage.set", value === undefined ? { key } : { key, value });
    },
  };

  const system: Mypaper["system"] = {
    stats: () => call("system.stats"),
    subscribe(intervalMs, cb) {
      let sub: number | string | undefined;
      let cancelled = false;
      let removeListener: (() => void) | undefined;
      call<{ sub: number | string }>("system.subscribe", { intervalMs }).then(
        (result) => {
          if (cancelled) return send("system.unsubscribe", { sub: result.sub });
          sub = result.sub;
          removeListener = listen(`system:${sub}`, cb);
        },
        (e: Error) => console.warn(`mypaper: system.subscribe failed: ${e.message}`),
      );
      return () => {
        if (cancelled) return;
        cancelled = true;
        removeListener?.();
        if (sub !== undefined) send("system.unsubscribe", { sub });
      };
    },
  };

  const media: Mypaper["media"] = {
    current: () => call("media.current"),
    onChange: (cb) => listen("media", cb, subscribeWire("media"), unsubscribeWire("media")),
    control: async (action) => void (await call("media.control", { action })),
  };

  const theme: Mypaper["theme"] = {
    current: () => call("theme.current"),
    onChange: (cb) => listen("theme", cb, subscribeWire("theme"), unsubscribeWire("theme")),
  };

  const on: Mypaper["on"] = (event: "visibility" | "editmode", cb: (value: boolean) => void) => listen(event, cb);

  const api: Mypaper = {
    instance: identity.instance,
    widget: identity.widget,
    settings: Object.freeze(settings),
    size: Object.freeze(size),
    on,
    storage: Object.freeze(storage),
    system: Object.freeze(system),
    media: Object.freeze(media),
    launch: async (target) => void (await call("launch", { target })),
    theme: Object.freeze(theme),
    plugin: <T = unknown>(id: string, method: string, params?: unknown) =>
      call<T>("plugin.call", params === undefined ? { plugin: id, method } : { plugin: id, method, params }),
    onPlugin(id, event, cb) {
      const params = { plugin: id, event };
      return listen(`plugin:${id}:${event}`, cb, subscribeWire("plugin", params), unsubscribeWire("plugin", params));
    },
  };

  Object.defineProperty(window, "mypaper", { value: Object.freeze(api), enumerable: true });
  send("hello");
})();
