// The WebView2 message channel to the Rust host (docs/bars.md, "Bridge"; shapes in protocol.ts).
// Requests are promises; events go to listeners, and the latest value of each is replayed to late
// subscribers, so a module built after the host pushed its topic still starts with current data.

import type { EventMap, EventName, Hello, Method, MethodMap } from "./protocol";

export interface Transport {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: Transport };
  }
}

export type Unsubscribe = () => void;

/** Requests with no params take none: `request("hello")`, `request("volume", { delta: 0.02 })`. */
type ParamsArgs<M extends Method> = MethodMap[M]["params"] extends undefined ? [] : [params: MethodMap[M]["params"]];

let transport: Transport | undefined;
let nextId = 1;
const pending = new Map<number, { resolve(value: unknown): void; reject(error: Error): void }>();
const listeners = new Map<string, Set<(data: unknown) => void>>();
const latest = new Map<string, unknown>();
// Events that mean "now", not "the current state": never replayed.
const TRANSIENT: ReadonlySet<string> = new Set<EventName>(["reveal"]);

function handleMessage(data: unknown): void {
  if (typeof data !== "object" || data === null) return;
  if ("event" in data && typeof data.event === "string" && "data" in data) {
    if (!TRANSIENT.has(data.event)) latest.set(data.event, data.data);
    for (const cb of listeners.get(data.event) ?? []) cb(data.data);
    return;
  }
  if (!("id" in data) || typeof data.id !== "number") return;
  const call = pending.get(data.id);
  if (!call) return;
  pending.delete(data.id);
  if ("error" in data && typeof data.error === "object" && data.error !== null) {
    const { code, message } = data.error as { code?: unknown; message?: unknown };
    call.reject(new Error(`${String(code)}: ${String(message)}`));
  } else if ("result" in data) {
    call.resolve(data.result);
  }
}

/** Picks the transport (the real WebView2, or the dev mock in a plain browser) and says hello. */
export async function connect(): Promise<Hello> {
  transport = window.chrome?.webview ?? (await import("./mock")).createMockTransport();
  transport.addEventListener("message", (event) => handleMessage(event.data));
  return request("hello");
}

export function request<M extends Method>(method: M, ...args: ParamsArgs<M>): Promise<MethodMap[M]["result"]> {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    // The host answers with the result type protocol.ts declares for the method.
    pending.set(id, { resolve: (value) => resolve(value as MethodMap[M]["result"]), reject });
    transport?.postMessage({ id, method, params: (args as unknown[])[0] });
  });
}

/** A request whose result nobody needs: errors are logged, not thrown. */
export function fire<M extends Method>(method: M, ...args: ParamsArgs<M>): void {
  request(method, ...args).catch((error: unknown) => console.warn(`[bars] ${method} failed`, error));
}

/** Subscribes to a host event; the latest value, if any, is delivered immediately. */
export function on<E extends EventName>(event: E, cb: (data: EventMap[E]) => void): Unsubscribe {
  // Values come from the host in the shape protocol.ts declares for the event.
  const listener = cb as (data: unknown) => void;
  const set = listeners.get(event) ?? new Set();
  listeners.set(event, set);
  set.add(listener);
  if (latest.has(event)) listener(latest.get(event));
  return () => set.delete(listener);
}

/** Collects subscriptions and cleanups so a bar can be torn down and rebuilt on a config change. */
export interface Scope {
  on<E extends EventName>(event: E, cb: (data: EventMap[E]) => void): void;
  add(cleanup: Unsubscribe): void;
  dispose(): void;
}

export function createScope(): Scope {
  const cleanups: Unsubscribe[] = [];
  return {
    on: (event, cb) => void cleanups.push(on(event, cb)),
    add: (cleanup) => void cleanups.push(cleanup),
    dispose: () => {
      for (const cleanup of cleanups.splice(0)) cleanup();
    },
  };
}
