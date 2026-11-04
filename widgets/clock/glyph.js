// @ts-check
// The Greeting face's time-of-day glyphs: a sunrise, a sun, a sunset and a moon, drawn as outlines with a soft
// highlight fill (`.fill`, colored in style.css). Markup built from constants only.

/** @typedef {import("./greeting.js").Period} Period */

/** @type {Record<Period, string>} */
const BODIES = {
  morning:
    '<path class="fill" d="M5 17a7 7 0 0 1 14 0z"/><path d="M2 17h20"/><path d="M12 6v2.5M5.2 9.2l1.7 1.7M18.8 9.2l-1.7 1.7M2.5 13.6H4M20 13.6h1.5"/><path d="M7.5 20.5h9"/>',
  afternoon:
    '<circle class="fill" cx="12" cy="12" r="4.2"/><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6"/>',
  evening:
    '<path class="fill" d="M6 16a6 6 0 0 1 12 0z"/><path d="M2 16h20"/><path d="M12 6.6v2M5.2 9.6l1.4 1.4M18.8 9.6 17.4 11"/><path d="M6 19.5h12M9 22.5h6"/>',
  night:
    '<path class="fill" d="M20 14.5A8.2 8.2 0 1 1 9.5 4a6.6 6.6 0 0 0 10.5 10.5z"/><path d="M18.5 3v3M17 4.5h3M21 9v2M20 10h2"/>',
};

/**
 * The glyph of a part of the day as an <svg> string (24x24, sized by CSS).
 * @param {Period} period
 */
export function glyphSvg(period) {
  return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${BODIES[period]}</svg>`;
}
