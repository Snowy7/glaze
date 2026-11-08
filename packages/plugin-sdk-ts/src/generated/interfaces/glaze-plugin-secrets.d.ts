/** @module Interface glaze:plugin/secrets@0.1.0 **/
export function get(key: string): string | undefined;
export function set(key: string, value: string): void;
export function remove(key: string): void;
export type Error = import('./glaze-plugin-types.js').Error;
