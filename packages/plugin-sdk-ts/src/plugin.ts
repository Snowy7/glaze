import type * as Lifecycle from './generated/interfaces/mypaper-plugin-lifecycle.js';
import { fromWit } from './compat.ts';
export type { Event, Action, FaceOpen, InitInfo } from './generated/interfaces/mypaper-plugin-lifecycle.js';

/** One plugin object per component instance. Handlers finish synchronously; I/O replies are
 * events. Returning undefined also prevents async handlers from silently discarding promises. */
export interface Plugin {
  init?(info: Lifecycle.InitInfo, state: Uint8Array | undefined): undefined;
  onEvent(event: Lifecycle.Event): undefined;
  save?(): Uint8Array;
}

/** Export the result as `lifecycle`. Throw a string from init to report a WIT init error. */
export function definePlugin(plugin: Plugin): typeof Lifecycle {
  return {
    init(info) {
      const normalized = fromWit(info);
      try { plugin.init?.(normalized, normalized.state); }
      catch (error) { throw error instanceof Error ? error.message : String(error); }
    },
    onEvent(event) { plugin.onEvent(fromWit(event)); },
    save() { return plugin.save?.() ?? new Uint8Array(); },
  };
}
