// The shell + host side of the bridge, for the mock harness: answers a widget's wire messages
// (docs/widgets.md), enforces the manifest's permissions, and pushes events. DOM-free, so it's
// unit-tested. Data comes from a `Backend` (fake data in the harness).

import { audioBins, MypaperError, WIRE_VERSION, withToneDefaults } from "../src";
import type {
  AppearanceState,
  MediaAction,
  MediaState,
  Permission,
  PickedFile,
  PickFileOptions,
  Settings,
  SizeInfo,
  SubscribeParams,
  SystemStats,
  ThemeInfo,
  UserInfo,
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
  /** `bins` floats in 0..1 for the audio topic. */
  spectrum(bins: number): number[];
  user(): UserInfo;
  appIcon(target: string, size: number): string | null;
  /** Shows the file dialog; `null` when cancelled. */
  pickFile(opts: PickFileOptions): Promise<PickedFile | null>;
  /** A shared storage namespace: common to every widget (every BridgeHost) on the page. */
  shared(namespace: string): Map<string, unknown>;
}

export type HostOutput = WireResponse | WireEvent;

/** The host's floor for `system.subscribe`. */
const MIN_INTERVAL_MS = 1000;

/** One audio frame every ~33 ms. */
const AUDIO_INTERVAL_MS = 1000 / 30;

const NAMESPACE = /^[a-z0-9-]+$/;

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
    "storage.get": ({ key, shared }) => this.store(shared).get(key) ?? null,
    "storage.set": ({ key, value, shared }) => {
      const store = this.store(shared);
      // Like the real host, a missing value deletes the key.
      if (value === undefined) store.delete(key);
      else store.set(key, value);
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
    "user.info": () => (this.need("user"), this.backend.user()),
    // Like the real host, minus the manifest check: the harness form is the place for that.
    "settings.set": ({ values }) => {
      if (typeof values === "object" && values !== null) this.setSettings({ ...this.state.settings, ...values });
      return null;
    },
    // The dialog is the consent: no permission.
    pickFile: (opts) => this.backend.pickFile(opts ?? {}),
    appIcon: ({ target, size }) => {
      this.need("launch");
      if (typeof target !== "string" || !target) throw new MypaperError("InvalidParams", "appIcon: empty target");
      return this.backend.appIcon(target, size ?? 64);
    },
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
    switch (p.topic) {
      case "system":
        return this.need("system.stats");
      case "media":
        return this.need("media");
      case "theme":
        return this.need("theme");
      case "audio":
        return this.need("audio");
      case "plugin":
        return this.needPlugin(p.plugin);
    }
  }

  /** This instance's storage, or a shared namespace (which needs `storage.shared:<namespace>`). */
  private store(shared: string | undefined): Map<string, unknown> {
    if (shared === undefined) return this.storage;
    if (typeof shared !== "string" || !NAMESPACE.test(shared)) {
      throw new MypaperError("InvalidParams", `storage: bad shared namespace ${JSON.stringify(shared)}`);
    }
    this.need(`storage.shared:${shared}`);
    return this.backend.shared(shared);
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

  /** `system` and `audio` are polled; the other topics are pushed by `notify*`. */
  private arm(sub: Subscription): void {
    const p = sub.params;
    if (p.topic === "system") {
      const push = () => this.pushSub(p.sub, this.backend.stats());
      push();
      sub.timer = setInterval(push, Math.max(MIN_INTERVAL_MS, p.intervalMs) / this.speed);
    } else if (p.topic === "audio") {
      const bins = audioBins(p.bins);
      sub.timer = setInterval(() => this.pushSub(p.sub, this.backend.spectrum(bins)), AUDIO_INTERVAL_MS);
    }
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
