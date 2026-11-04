// Calls the shell makes to the host on a widget's behalf (a settings form searching places with
// the widget's plugin). They go out like the widget's own `call`s, so the host checks the same
// permissions, but with "shell-" ids: `takeReply` claims those answers before the frames see them.

import type { HostToShell, ShellToHost } from "./protocol";

const PREFIX = "shell-";
const pending = new Map<string, { resolve(value: unknown): void; reject(error: Error): void }>();
let next = 0;

/** A wire `call` for `instance`, answered with its `result` (or rejected with its error). */
export function hostCall(send: (message: ShellToHost) => void, instance: string, method: string, params?: unknown): Promise<unknown> {
  const id = `${PREFIX}${++next}`;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    send({ type: "call", instance, id, method, ...(params !== undefined && { params }) });
  });
}

/** Settles a shell call from its reply; false for anything that isn't one (frames get it). */
export function takeReply(message: HostToShell): boolean {
  if (message.type !== "reply" || typeof message.id !== "string" || !message.id.startsWith(PREFIX)) return false;
  const call = pending.get(message.id);
  pending.delete(message.id);
  if (message.error) call?.reject(new Error(message.error.message));
  else call?.resolve(message.result ?? null);
  return true;
}
