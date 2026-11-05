/** @module Interface mypaper:plugin/http@0.1.0 **/
export function fetch(request: Request): bigint;
export type Error = import('./mypaper-plugin-types.js').Error;
export interface Header {
  name: string,
  value: string,
}
/**
 * # Variants
 * 
 * ## `"get"`
 * 
 * ## `"post"`
 * 
 * ## `"put"`
 * 
 * ## `"patch"`
 * 
 * ## `"delete"`
 */
export type Method = "get" | "post" | "put" | "patch" | "delete";
export interface Request {
  method: Method,
  url: string,
  headers: Array<Header>,
  body?: Uint8Array,
  timeoutMs?: number,
}
export interface Response {
  status: number,
  headers: Array<Header>,
  body: Uint8Array,
}
export interface Reply {
  id: bigint,
  outcome: Result<Response, Error>,
}
export type Result<T, E> = { tag: 'ok', val: T } | { tag: 'err', val: E };
