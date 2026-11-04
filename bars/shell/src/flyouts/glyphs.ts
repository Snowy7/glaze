// The flyouts' glyphs: monochrome 16x16 inline SVGs in the style of icons.ts (currentColor,
// 1.4 stroke), static trusted strings set with innerHTML by `glyph()`. Each is tagged
// `data-glyph="fly-<name>"`, so glyph sets (style/glyph-sets.ts) and icon packs can swap it.

const svg = (key: string, inner: string): string =>
  `<svg data-glyph="fly-${key}" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;
const FILL = `fill="currentColor" stroke="none"`;

export const glyphs = {
  search: svg("search", `<circle cx="7" cy="7" r="4.6"/><path d="M10.4 10.4 14 14"/>`),
  wifi: svg("wifi", `<path d="M1.64 6.64A9 9 0 0 1 14.36 6.64"/><path d="M3.76 8.76A6 6 0 0 1 12.24 8.76"/><path d="M5.88 10.88A3 3 0 0 1 10.12 10.88"/><circle cx="8" cy="13" r="1.15" ${FILL}/>`),
  bluetooth: svg("bluetooth", `<path d="M4.5 5 11 10.5 8 13V3l3 2.5L4.5 11"/>`),
  plane: svg("plane", `<path d="M8 1.8c.7 0 1 .7 1 1.5v3.4l4.8 2.6v1.4L9 9.3v2.8l1.6 1.2v1.2L8 13.8l-2.6.7v-1.2L7 12.1V9.3l-4.8 1.4V9.3L7 6.7V3.3c0-.8.3-1.5 1-1.5Z"/>`),
  moon: svg("moon", `<path d="M13 9.6A5.5 5.5 0 1 1 6.4 3a4.5 4.5 0 0 0 6.6 6.6Z"/>`),
  sun: svg("sun", `<circle cx="8" cy="8" r="2.6"/><path d="M8 1.6v1.3M8 13.1v1.3M1.6 8h1.3M13.1 8h1.3M3.5 3.5l.9.9M11.6 11.6l.9.9M3.5 12.5l.9-.9M11.6 4.4l.9-.9"/>`),
  leaf: svg("leaf", `<path d="M13.5 2.5C7 2.5 3 5.5 3 10a3.5 3.5 0 0 0 3.5 3.5C11 13.5 13.5 9 13.5 2.5Z"/><path d="M2.5 14C5 10.5 7.5 8.5 10 7"/>`),
  gauge: svg("gauge", `<path d="M2.5 11.5a5.5 5.5 0 1 1 11 0"/><path d="M8 11.5 10.6 7"/><circle cx="8" cy="11.5" r=".9" ${FILL}/>`),
  focus: svg("focus", `<circle cx="8" cy="8" r="5.5"/><circle cx="8" cy="8" r="2" ${FILL}/>`),
  nightlight: svg("nightlight", `<path d="M10.5 2.2a5.8 5.8 0 1 0 3.3 8.3A5 5 0 0 1 10.5 2.2Z"/><path d="M12 2v2M11 3h2"/>`),
  saver: svg("saver", `<rect x="1.5" y="4.25" width="11.5" height="7.5" rx="2"/><path d="M14.7 7v2"/><path d="M8 5.5 5.8 8.5h2.6L6.6 10.8"/>`),
  gear: svg("gear", `<circle cx="8" cy="8" r="2.2"/><path d="M8 1.6v1.6M8 12.8v1.6M1.6 8h1.6M12.8 8h1.6M3.5 3.5l1.1 1.1M11.4 11.4l1.1 1.1M3.5 12.5l1.1-1.1M11.4 4.6l1.1-1.1"/>`),
  chevronRight: svg("chevron-right", `<path d="M6.5 4l4 4-4 4"/>`),
  chevronLeft: svg("chevron-left", `<path d="M9.5 4l-4 4 4 4"/>`),
  chevronDown: svg("chevron-down", `<path d="M4 6.5l4 4 4-4"/>`),
  chevronUp: svg("chevron-up", `<path d="M4 9.5l4-4 4 4"/>`),
  check: svg("check", `<path d="M3.5 8.5l3 3 6-7"/>`),
  close: svg("close", `<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>`),
  lock: svg("lock", `<rect x="3.5" y="7" width="9" height="6.5" rx="1.6"/><path d="M5.5 7V5.2a2.5 2.5 0 0 1 5 0V7"/>`),
  bell: svg("bell", `<path d="M4 11V7.2a4 4 0 0 1 8 0V11l1.2 1.5H2.8Z"/><path d="M6.6 14a1.5 1.5 0 0 0 2.8 0"/>`),
  bellOff: svg("bell-off", `<path d="M4 11V7.2a4 4 0 0 1 6.6-3M12 6.6V11l1.2 1.5H5"/><path d="M6.6 14a1.5 1.5 0 0 0 2.8 0"/><path d="M2.5 2.5l11 11"/>`),
  globe: svg("globe", `<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c1.8 1.8 2.6 3.8 2.6 6S9.8 12.2 8 14C6.2 12.2 5.4 10.2 5.4 8S6.2 3.8 8 2Z"/>`),
  keyboard: svg("keyboard", `<rect x="1.5" y="4" width="13" height="8.5" rx="1.6"/><path d="M4 6.8h.01M6.5 6.8h.01M9 6.8h.01M11.5 6.8h.01M4.5 9.8h7"/>`),
  calendar: svg("calendar", `<rect x="2" y="3" width="12" height="11" rx="2"/><path d="M2 6.5h12M5.2 1.8v2.4M10.8 1.8v2.4"/>`),
  speaker: svg("speaker", `<path d="M2.5 6h2.2L8 3.2v9.6L4.7 10H2.5Z" ${FILL}/><path d="M10.4 6.3a2.5 2.5 0 0 1 0 3.4"/><path d="M12.1 4.6a5 5 0 0 1 0 6.8"/>`),
  speakerMute: svg("speaker-mute", `<path d="M2.5 6h2.2L8 3.2v9.6L4.7 10H2.5Z" ${FILL}/><path d="M10.5 6l3.5 4M14 6l-3.5 4"/>`),
  mixer: svg("mixer", `<path d="M4 2.5v11M8 2.5v11M12 2.5v11"/><rect x="2.6" y="8.5" width="2.8" height="2" rx=".6" ${FILL}/><rect x="6.6" y="4.5" width="2.8" height="2" rx=".6" ${FILL}/><rect x="10.6" y="9.5" width="2.8" height="2" rx=".6" ${FILL}/>`),
  ethernet: svg("ethernet", `<path d="M2.5 4h11v6.5H11V13H5v-2.5H2.5Z"/><path d="M6 6.5v2M8 6.5v2M10 6.5v2"/>`),
  vpn: svg("vpn", `<path d="M8 1.8 13 3.6v4c0 3.2-2.2 5.4-5 6.6-2.8-1.2-5-3.4-5-6.6v-4Z"/><path d="M6 8l1.5 1.5L10.5 6.5"/>`),
  refresh: svg("refresh", `<path d="M13 4.5V8h-3.5"/><path d="M12.6 8A4.8 4.8 0 1 1 11 4.4L13 6.5"/>`),
  power: svg("power", `<path d="M8 1.8v5.6"/><path d="M4.7 4a5.4 5.4 0 1 0 6.6 0"/>`),
  apps: svg("apps", `<rect x="2" y="2" width="5" height="5" rx="1.2"/><rect x="9" y="2" width="5" height="5" rx="1.2"/><rect x="2" y="9" width="5" height="5" rx="1.2"/><rect x="9" y="9" width="5" height="5" rx="1.2"/>`),
  file: svg("file", `<path d="M4.5 1.8H9l3 3v8.4a1 1 0 0 1-1 1H4.5a1 1 0 0 1-1-1V2.8a1 1 0 0 1 1-1Z"/><path d="M9 1.8v3h3"/>`),
  clipboard: svg("clipboard", `<rect x="3" y="3" width="10" height="11.2" rx="1.6"/><path d="M6 3V2.2h4V3"/><path d="M5.5 7h5M5.5 9.5h5M5.5 12h3"/>`),
  image: svg("image", `<rect x="2" y="3" width="12" height="10" rx="1.6"/><circle cx="5.8" cy="6.5" r="1.1"/><path d="M2.5 12 6.5 8.5l2.8 2.5 1.7-1.5 2.5 2.3"/>`),
  link: svg("link", `<path d="M6.8 9.2a2.6 2.6 0 0 0 3.7 0l2.2-2.2a2.6 2.6 0 0 0-3.7-3.7l-.8.8"/><path d="M9.2 6.8a2.6 2.6 0 0 0-3.7 0L3.3 9a2.6 2.6 0 0 0 3.7 3.7l.8-.8"/>`),
  play: svg("play", `<path d="M5 3.3v9.4L12.8 8Z" ${FILL}/>`),
  pause: svg("pause", `<rect x="4" y="3" width="3" height="10" rx=".9" ${FILL}/><rect x="9" y="3" width="3" height="10" rx=".9" ${FILL}/>`),
  next: svg("next", `<path d="M3.5 3.5v9l6.5-4.5Z" ${FILL}/><path d="M12 3.5v9"/>`),
  previous: svg("previous", `<path d="M12.5 3.5v9L6 8Z" ${FILL}/><path d="M4 3.5v9"/>`),
  pin: svg("pin", `<path d="M9.8 2.2 13.8 6.2 11.7 7l-2.4 2.4.3 2.7-1.4 1.4-2.2-2.2L3 14.3 1.7 13l2.9-2.9-2.2-2.2L3.8 6.5l2.7.3L8.9 4.4Z"/>`),
  user: svg("user", `<circle cx="8" cy="5.6" r="2.8"/><path d="M2.8 14a5.2 5.2 0 0 1 10.4 0"/>`),
  overflow: svg("overflow", `<path d="M4 9.5l4-4 4 4"/>`),
} as const;

/** Wi-Fi with `lit` bars of 4 (the dot alone is 1), dimming the rest. */
export function wifiBars(lit: 1 | 2 | 3 | 4): string {
  const arcs = ["M5.88 10.88A3 3 0 0 1 10.12 10.88", "M3.76 8.76A6 6 0 0 1 12.24 8.76", "M1.64 6.64A9 9 0 0 1 14.36 6.64"];
  return svg(
    `wifi-${lit}`,
    `${arcs.map((d, i) => `<path d="${d}"${i < lit - 1 ? "" : ` opacity=".28"`}/>`).join("")}<circle cx="8" cy="13" r="1.15" ${FILL}/>`,
  );
}
