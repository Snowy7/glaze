/** @module Interface mypaper:plugin/log@0.1.0 **/
export function write(level: Level, message: string): void;
/**
 * # Variants
 * 
 * ## `"debug"`
 * 
 * ## `"info"`
 * 
 * ## `"warn"`
 * 
 * ## `"error"`
 */
export type Level = "debug" | "info" | "warn" | "error";
