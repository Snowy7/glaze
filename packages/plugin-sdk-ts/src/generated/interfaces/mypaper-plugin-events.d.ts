/** @module Interface mypaper:plugin/events@0.1.0 **/
export function subscribe(topics: Array<Topic>): void;
export function unsubscribe(topics: Array<Topic>): void;
export type Error = import('./mypaper-plugin-types.js').Error;
/**
 * # Variants
 * 
 * ## `"windows"`
 * 
 * ## `"monitors"`
 * 
 * ## `"notifications"`
 * 
 * ## `"media"`
 * 
 * ## `"power"`
 * 
 * ## `"theme"`
 */
export type Topic = "windows" | "monitors" | "notifications" | "media" | "power" | "theme";
