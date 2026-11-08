/** @module Interface glaze:plugin/websocket@0.1.0 **/
export function connect(url: string): bigint;
export function send(socket: bigint, message: Message): void;
export function close(socket: bigint): void;
export type Error = import('./glaze-plugin-types.js').Error;
export type Message = MessageText | MessageBinary;
export interface MessageText {
  tag: 'text',
  val: string,
}
export interface MessageBinary {
  tag: 'binary',
  val: Uint8Array,
}
export type SocketEvent = SocketEventOpened | SocketEventMessage | SocketEventClosed;
export interface SocketEventOpened {
  tag: 'opened',
  val: bigint,
}
export interface SocketEventMessage {
  tag: 'message',
  val: [bigint, Message],
}
export interface SocketEventClosed {
  tag: 'closed',
  val: [bigint, string | undefined],
}
