/** @module Interface glaze:plugin/ui@0.1.0 **/
/**
 * Replaces what `instance` shows. Unchanged faces are dropped by the host.
 */
export function setView(instance: string, face: Face): void;
export function clear(instance: string): void;
/**
 * Registers an image (BGRA8 premultiplied, `width * 4` stride) for canvas `image` draws;
 * the host moves it to the shell through shared memory. Returns its id.
 */
export function putImage(width: number, height: number, pixels: Uint8Array): number;
export type Face = import('./glaze-plugin-view.js').Face;
export type Error = import('./glaze-plugin-types.js').Error;
