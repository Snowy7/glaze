/** @module Interface mypaper:plugin/system@0.1.0 **/
export function monitors(): Array<Monitor>;
export function theme(): ThemeInfo;
export function power(): PowerInfo;
/**
 * Needs `windows`.
 */
export function foreground(): WindowInfo | undefined;
export type Error = import('./mypaper-plugin-types.js').Error;
export interface Monitor {
  id: string,
  x: number,
  y: number,
  width: number,
  height: number,
  scale: number,
  primary: boolean,
}
export interface ThemeInfo {
  dark: boolean,
  accent: string,
}
export interface PowerInfo {
  onBattery: boolean,
  percent?: number,
  saver: boolean,
}
export interface WindowInfo {
  id: bigint,
  title: string,
  process: string,
  x: number,
  y: number,
  width: number,
  height: number,
}
