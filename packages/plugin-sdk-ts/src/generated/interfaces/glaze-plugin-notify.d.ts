/** @module Interface glaze:plugin/notify@0.1.0 **/
export function show(notification: Notification): void;
export type Error = import('./glaze-plugin-types.js').Error;
export interface Notification {
  title: string,
  body: string,
  action?: string,
}
