/** @module Interface glaze:plugin/timers@0.1.0 **/
export function after(id: string, ms: number): void;
/**
 * Refused under 1000 ms.
 */
export function every(id: string, ms: number): void;
export function cancel(id: string): void;
export type Error = import('./glaze-plugin-types.js').Error;
