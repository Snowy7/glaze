// @ts-check
// The tile icons: one 24x24 outline set (stroke 1.75, round caps and joins), drawn in currentColor.
// `iconSvg` returns markup from constants and numbers only, so it is safe for innerHTML.

/** @typedef {import("./toggles.js").TileId} TileId */
/** @typedef {import("./toggles.js").TileState} TileState */

const STATIC = {
  bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  focus: '<path d="M20.5 14.5A8.5 8.5 0 1 1 9.5 3.5a6.8 6.8 0 0 0 11 11z"/>',
  airplane:
    '<path d="M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2z"/>',
  nightlight:
    '<path d="M12 9V3"/><path d="m5.6 11.6 1.4 1.4"/><path d="M2 18h2"/><path d="M20 18h2"/><path d="m18.4 11.6-1.4 1.4"/><path d="M22 22H2"/><path d="m16 6-4 3-4-3"/><path d="M16 18a4 4 0 0 0-8 0"/>',
  sound:
    '<path d="M11 5 6.5 9H3v6h3.5L11 19z"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6"/><path d="M18.5 6.2a8.2 8.2 0 0 1 0 11.6"/>',
  display: '<rect x="2.5" y="4" width="19" height="12.5" rx="2.5"/><path d="M8 20.5h8"/><path d="M12 16.5v4"/>',
  ethernet: '<rect x="5" y="8" width="14" height="10" rx="2.5"/><path d="M9 8V5h6v3"/><path d="M9 12v2.5M12 12v2.5M15 12v2.5"/>',
};

/** The Wi-Fi arcs, the ones above the signal fade out. @param {number} bars 1 to 4 */
function wifi(bars) {
  const arc = (/** @type {string} */ d, /** @type {number} */ from) =>
    `<path d="${d}"${bars < from ? ' opacity=".28"' : ""}/>`;
  return (
    arc("M2.5 9.2a14 14 0 0 1 19 0", 4) +
    arc("M5.5 12.6a9.6 9.6 0 0 1 13 0", 3) +
    arc("M8.6 16a5.2 5.2 0 0 1 6.8 0", 2) +
    '<path d="M12 19.6h.01"/>'
  );
}

/** A battery whose fill is the charge, with a bolt while charging. @param {number} level 0 to 1 @param {boolean} charging */
function battery(level, charging) {
  const fill = Math.max(0, Math.min(1, level)) * 12.5;
  return (
    '<rect x="2" y="7" width="17" height="10" rx="2.6"/><path d="M22 10.5v3"/>' +
    (fill > 0.5 ? `<rect x="4.25" y="9.25" width="${fill.toFixed(1)}" height="5.5" rx="1.1" fill="currentColor" stroke="none"/>` : "") +
    (charging ? '<path d="m11.4 8.6-2.3 3.5h3.8l-2.3 3.5" stroke-width="1.5" class="bolt"/>' : "")
  );
}

/**
 * The icon of a tile as an <svg> string (24x24 viewBox, sized by CSS).
 * @param {TileId} id
 * @param {TileState} [state]
 */
export function iconSvg(id, state) {
  let body;
  if (id === "wifi") body = state?.link === "ethernet" ? STATIC.ethernet : wifi(state?.bars ?? 4);
  else if (id === "battery") body = battery(state?.level ?? 0.55, state?.charging ?? false);
  else body = STATIC[id];
  return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}
