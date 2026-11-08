/** @module Interface glaze:plugin/lifecycle@0.1.0 **/
export function init(info: InitInfo): void;
export function onEvent(event: Event): void;
/**
 * State to keep across an unload; handed back in `init`.
 */
export function save(): Uint8Array;
export type SizeClass = import('./glaze-plugin-types.js').SizeClass;
export type Reply = import('./glaze-plugin-http.js').Reply;
export type SocketEvent = import('./glaze-plugin-websocket.js').SocketEvent;
export type PipeEvent = import('./glaze-plugin-pipes.js').PipeEvent;
export type NativeReply = import('./glaze-plugin-native.js').NativeReply;
export type NativeMessage = import('./glaze-plugin-native.js').NativeMessage;
export interface InitInfo {
  plugin: string,
  /**
   * The plugin's settings as JSON (`{}` when none).
   */
  settings: string,
  /**
   * What `save` returned before the last unload.
   */
  state?: Uint8Array,
}
/**
 * A face, bar module or panel; `face` is its manifest id.
 */
export interface FaceOpen {
  instance: string,
  face: string,
  size: SizeClass,
  settings: string,
}
export interface Action {
  instance: string,
  action: string,
  value?: string,
}
/**
 * # Variants
 * 
 * ## `"opened"`
 * 
 * ## `"closed"`
 * 
 * ## `"focused"`
 * 
 * ## `"moved"`
 */
export type WindowChange = "opened" | "closed" | "focused" | "moved";
export interface WindowEvent {
  kind: WindowChange,
  id: bigint,
  title: string,
  process: string,
}
export type Event = EventOpen | EventResize | EventClose | EventAction | EventCommand | EventTimer | EventHttp | EventSocket | EventPipe | EventNative | EventNativeEvent | EventWindow | EventMonitors | EventNotification | EventMedia | EventPower | EventTheme | EventSuspend | EventResume;
export interface EventOpen {
  tag: 'open',
  val: FaceOpen,
}
export interface EventResize {
  tag: 'resize',
  val: FaceOpen,
}
export interface EventClose {
  tag: 'close',
  val: string,
}
export interface EventAction {
  tag: 'action',
  val: Action,
}
export interface EventCommand {
  tag: 'command',
  val: string,
}
export interface EventTimer {
  tag: 'timer',
  val: string,
}
export interface EventHttp {
  tag: 'http',
  val: Reply,
}
export interface EventSocket {
  tag: 'socket',
  val: SocketEvent,
}
export interface EventPipe {
  tag: 'pipe',
  val: PipeEvent,
}
export interface EventNative {
  tag: 'native',
  val: NativeReply,
}
export interface EventNativeEvent {
  tag: 'native-event',
  val: NativeMessage,
}
export interface EventWindow {
  tag: 'window',
  val: WindowEvent,
}
export interface EventMonitors {
  tag: 'monitors',
}
export interface EventNotification {
  tag: 'notification',
  val: string,
}
export interface EventMedia {
  tag: 'media',
  val: string,
}
export interface EventPower {
  tag: 'power',
}
export interface EventTheme {
  tag: 'theme',
}
export interface EventSuspend {
  tag: 'suspend',
}
export interface EventResume {
  tag: 'resume',
}
