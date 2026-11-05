/** @module Interface mypaper:plugin/style@0.1.0 **/
export function apply(patch: StylePatch): void;
export function reset(surface: Surface): void;
export type Tone = import('./mypaper-plugin-types.js').Tone;
export type Error = import('./mypaper-plugin-types.js').Error;
/**
 * # Variants
 * 
 * ## `"top-bar"`
 * 
 * ## `"dock"`
 * 
 * ## `"panel"`
 * 
 * ## `"menu"`
 * 
 * ## `"widget"`
 * 
 * ## `"glass"`
 */
export type Surface = "top-bar" | "dock" | "panel" | "menu" | "widget" | "glass";
export type TokenValue = TokenValueColor | TokenValueNumber | TokenValueTone | TokenValueFlag;
export interface TokenValueColor {
  tag: 'color',
  val: string,
}
export interface TokenValueNumber {
  tag: 'number',
  val: number,
}
export interface TokenValueTone {
  tag: 'tone',
  val: Tone,
}
export interface TokenValueFlag {
  tag: 'flag',
  val: boolean,
}
export interface Token {
  name: string,
  value: TokenValue,
}
export interface StylePatch {
  surface: Surface,
  tokens: Array<Token>,
}
