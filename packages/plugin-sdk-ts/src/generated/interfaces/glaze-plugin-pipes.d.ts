/** @module Interface glaze:plugin/pipes@0.1.0 **/
export function open(name: string): bigint;
export function write(pipe: bigint, bytes: Uint8Array): void;
export function close(pipe: bigint): void;
export type Error = import('./glaze-plugin-types.js').Error;
export type PipeEvent = PipeEventData | PipeEventClosed;
export interface PipeEventData {
  tag: 'data',
  val: [bigint, Uint8Array],
}
export interface PipeEventClosed {
  tag: 'closed',
  val: bigint,
}
