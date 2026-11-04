// The shell + host side of the bridge, for the mock harness: answers a widget's wire messages
// (docs/widgets.md), enforces the manifest's permissions, and pushes events. DOM-free, so it's
// unit-tested. Data comes from a `Backend` (fake data in the harness).

import { MypaperError, WIRE_VERSION, withToneDefaults } from "../src";
import type {
  AppearanceState,
  MediaAction,
  MediaState,
  Permission,
  Settings,
  SizeInfo,
  SubscribeParams,
  SystemStats,
  ThemeInfo,
  WidgetManifest,
  WireEvent,
  WireMethods,
  WireRequest,
  WireResponse,
} from "../src/types";

/** Where the data comes from. The harness fakes all of it. */
export interface Backend {
  stats(): SystemStats;
  media(): MediaState | null;
  control(action: MediaAction): void;
  theme(): ThemeInfo;
  launch(target: string): void;
  plugin(id: string, method: string, params: unknown): Promise<unknown>;
}

export type HostOutput = WireResponse | WireEvent;

/** The host's floor for `system.subscribe`. */
const MIN_INTERVAL_MS = 1000;

type Handlers = { [M in keyof WireMethods]: (params: WireMethods[M][0]) => Promise<WireMethods[M][1]> | WireMethods[M][1] };

interface Subscription {
  params: SubscribeParams;
  timer?: ReturnType<typeof setInterval> | undefined;
}

export class BridgeHost {
  private subs = new Map<string, Subscription>();
  readonly storage = new Map<string, unknown>();

  visible = true;
  editing = false;
  /** Divides every `system` interval: a screenshot can then show a full sparkline history in seconds. */
  speed = 1;

  constructor(
    readonly instance: string,
    readonly manifest: WidgetManifest,
    private backend: Backend,
    private out: (msg: HostOutput) => void,
    private state: { settings: Settings; size: SizeInfo; look: AppearanceState },
    /** Called with each handled call, for the wire log. */
    private log: (entry: { dir: "in" | "out"; msg: unknown }) => void = () => {},
  ) {}

  // ---------------------------------------------------------- from the widget

  /** Handles one raw message from the widget iframe. Ignores anything that isn't a request. */
  async handle(data: unknown): Promise<void> {
    if (typeof data !== "object" || data === null) return;
    if (!("mypaper" in data) || data.mypaper !== WIRE_VERSION) return;
    if (!("id" in data) || !("method" in data)) return;
    this.log({ dir: "in", msg: data });
    const req = data as WireRequest;
    try {
      const handler = this.handlers[req.method] as ((params: unknown) => unknown) | undefined;
      if (!handler) throw new MypaperError("NotFound", `unknown method "${String(req.method)}"`);
      const result = await handler(req.params);
      this.send({ mypaper: WIRE_VERSION, id: req.id, result: result ?? null });
    } catch (err) {
      const e = err instanceof MypaperError ? err : new MypaperError("Internal", String(err));
      this.send({ mypaper: WIRE_VERSION, id: req.id, error: e.toInfo() });
    }
  }

  private handlers: Handlers = {
    hello: () => ({
      instance: this.instance,
      widget: this.manifest,
      settings: this.state.settings,
      size: this.state.size,
      visible: this.visible,
      editing: this.editing,
      // Like the real host, send final values: tone-aware defaults applied.
      appearance: withToneDefaults(this.state.look.appearance),
      themeCss: this.state.look.themeCss,
    }),
    "storage.get": ({ key }) => this.storage.get(key) ?? null,
    "storage.set": ({ key, value }) => {
      this.storage.set(key, value);
      return null;
    },
    "system.stats": () => (this.need("system.stats"), this.backend.stats()),
    "media.current": () => (this.need("media"), this.backend.media()),
    "media.control": ({ action }) => {
      this.need("media");
      this.backend.control(action);
      return null;
    },
    launch: ({ target }) => {
      this.need("launch");
      if (typeof target !== "string" || !target) throw new MypaperError("InvalidParams", "launch: empty target");
      this.backend.launch(target);
      return null;
    },
    "theme.current": () => (this.need("theme"), this.backend.theme()),
    plugin: ({ plugin, method, params }) => {
      this.needPlugin(plugin);
      return this.backend.plugin(plugin, method, params);
    },
    subscribe: (params) => {
      this.checkSubscribe(params);
      this.startSub(params);
      return null;
    },
    unsubscribe: ({ sub }) => {
      this.stopSub(sub);
      return null;
    },
  };

  private need(permission: Permission): void {
    if (!this.manifest.permissions?.includes(permission)) {
      throw new MypaperError("PermissionDenied", `this widget's manifest doesn't declare "${permission}"`);
    }
  }

  private needPlugin(id: string): void {
    if (!this.manifest.plugins?.includes(id)) {
      throw new MypaperError("PermissionDenied", `this widget's manifest doesn't list plugin "${id}"`);
    }
  }

  private checkSubscribe(p: SubscribeParams): void {
    if (p.topic === "system") this.need("system.stats");
    else if (p.topic === "media") this.need("media");
    else if (p.topic === "theme") this.need("theme");
    else this.needPlugin(p.plugin);
  }

  // ---------------------------------------------------------- subscriptions

  private startSub(params: SubscribeParams): void {
    this.stopSub(params.sub);
    const sub: Subscription = { params };
    this.subs.set(params.sub, sub);
    if (this.visible) this.arm(sub);
  }

  private stopSub(id: string): void {
    const sub = this.subs.get(id);
    if (sub) clearInterval(sub.timer);
    this.subs.delete(id);
  }

  /** Only `system` is polled; the other topics are pushed by `notify*`. */
  private arm(sub: Subscription): void {
    const p = sub.params;
    if (p.topic !== "system") return;
    const push = () => this.pushSub(p.sub, this.backend.stats());
    push();
    sub.timer = setInterval(push, Math.max(MIN_INTERVAL_MS, p.intervalMs) / this.speed);
  }

  private pushSub(sub: string, value: unknown): void {
    this.send({ mypaper: WIRE_VERSION, event: "sub", data: { sub, value } });
  }

  private pushTopic(topic: "media" | "theme", value: unknown): void {
    if (!this.visible) return;
    for (const [id, sub] of this.subs) if (sub.params.topic === topic) this.pushSub(id, value);
  }

  // ---------------------------------------------------------- from the harness

  notifyMedia(): void {
    this.pushTopic("media", this.backend.media());
  }

  notifyTheme(): void {
    this.pushTopic("theme", this.backend.theme());
  }

  notifyPlugin(plugin: string, event: string, data: unknown): void {
    if (!this.visible) return;
    for (const [id, sub] of this.subs) {
      const p = sub.params;
      if (p.topic === "plugin" && p.plugin === plugin && p.event === event) this.pushSub(id, data);
    }
  }

  /** Like the real host: subscriptions pause while hidden and resume when shown. */
  setVisible(visible: boolean): void {
    if (visible === this.visible) return;
    this.visible = visible;
    for (const sub of this.subs.values()) {
      clearInterval(sub.timer);
      sub.timer = undefined;
      if (visible) this.arm(sub);
    }
    this.send({ mypaper: WIRE_VERSION, event: "visibility", data: visible });
  }

  setEditing(editing: boolean): void {
    this.editing = editing;
    this.send({ mypaper: WIRE_VERSION, event: "editmode", data: editing });
  }

  setSettings(settings: Settings): void {
    this.state.settings = settings;
    this.send({ mypaper: WIRE_VERSION, event: "settings", data: settings });
  }

  setAppearance(look: AppearanceState): void {
    this.state.look = look;
    const sent = { ...look, appearance: withToneDefaults(look.appearance) };
    this.send({ mypaper: WIRE_VERSION, event: "appearance", data: sent });
  }

  setSize(size: SizeInfo): void {
    this.state.size = size;
    this.send({ mypaper: WIRE_VERSION, event: "size", data: size });
  }

  /** Stops every timer (the iframe is going away). */
  dispose(): void {
    for (const id of [...this.subs.keys()]) this.stopSub(id);
  }

  private send(msg: HostOutput): void {
    this.log({ dir: "out", msg });
    this.out(msg);
  }
}
