/** @module Interface mypaper:plugin/types@0.1.0 **/
/**
 * # Variants
 * 
 * ## `"small"`
 * 
 * ## `"medium"`
 * 
 * ## `"large"`
 */
export type SizeClass = "small" | "medium" | "large";
/**
 * A theme color role, or `#rrggbb` for brand fills. Roles follow the theme; hex doesn't.
 */
export type Tone = ToneLabel | ToneSecondary | ToneTertiary | ToneAccent | ToneWarning | ToneDestructive | TonePositive | ToneHex;
export interface ToneLabel {
  tag: 'label',
}
export interface ToneSecondary {
  tag: 'secondary',
}
export interface ToneTertiary {
  tag: 'tertiary',
}
export interface ToneAccent {
  tag: 'accent',
}
export interface ToneWarning {
  tag: 'warning',
}
export interface ToneDestructive {
  tag: 'destructive',
}
export interface TonePositive {
  tag: 'positive',
}
export interface ToneHex {
  tag: 'hex',
  val: string,
}
/**
 * The kit's text roles.
 * # Variants
 * 
 * ## `"large-title"`
 * 
 * ## `"title1"`
 * 
 * ## `"title2"`
 * 
 * ## `"title3"`
 * 
 * ## `"headline"`
 * 
 * ## `"body"`
 * 
 * ## `"callout"`
 * 
 * ## `"subheadline"`
 * 
 * ## `"footnote"`
 * 
 * ## `"caption"`
 */
export type TextRole = "large-title" | "title1" | "title2" | "title3" | "headline" | "body" | "callout" | "subheadline" | "footnote" | "caption";
/**
 * # Variants
 * 
 * ## `"start"`
 * 
 * ## `"center"`
 * 
 * ## `"end"`
 */
export type Align = "start" | "center" | "end";
export type Error = ErrorDenied | ErrorUnsupported | ErrorInvalid | ErrorFailed;
/**
 * The manifest doesn't declare the permission this call needs.
 */
export interface ErrorDenied {
  tag: 'denied',
  val: string,
}
/**
 * Not available in this host (yet), or not on this machine.
 */
export interface ErrorUnsupported {
  tag: 'unsupported',
  val: string,
}
export interface ErrorInvalid {
  tag: 'invalid',
  val: string,
}
export interface ErrorFailed {
  tag: 'failed',
  val: string,
}
