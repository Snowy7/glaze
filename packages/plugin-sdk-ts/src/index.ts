import { witUi } from './host.js';
import { flatten, type Face } from './view.js';
export * from './view.js';
export * from './plugin.js';
export { capabilityError } from './compat.js';
export type * from './generated/plugin.js';
export type * as types from './generated/interfaces/mypaper-plugin-types.js';
export type * as view from './generated/interfaces/mypaper-plugin-view.js';
export { events, http, launch, log, native, notify, pipes, secrets, storage, style, system, timers, websocket } from './host.js';

export const ui = {
  setView(instance: string, face: Face): void { witUi.setView(instance, flatten(face)); },
  clear: witUi.clear,
  putImage: witUi.putImage,
};
