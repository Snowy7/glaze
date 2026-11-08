/** @module Interface glaze:plugin/native@0.1.0 **/
export function call(module: string, method: string, bytes: Uint8Array): bigint;
export type Error = import('./glaze-plugin-types.js').Error;
export interface NativeReply {
  id: bigint,
  outcome: Result<Uint8Array, Error>,
}
export interface NativeMessage {
  module: string,
  name: string,
  bytes: Uint8Array,
}
export type Result<T, E> = { tag: 'ok', val: T } | { tag: 'err', val: E };
