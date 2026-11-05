/** @module Interface mypaper:plugin/storage@0.1.0 **/
export function get(key: string): Uint8Array | undefined;
export function set(key: string, value: Uint8Array): void;
export function remove(key: string): void;
export type Error = import('./mypaper-plugin-types.js').Error;
