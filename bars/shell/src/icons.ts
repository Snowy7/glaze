// Inline monochrome glyphs (16x16, currentColor): one consistent set instead of an icon font.
// Static trusted strings, set with innerHTML by `glyph()`.

import type { VolumeGlyph } from "./top/logic";

const svg = (inner: string): string =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;

const FILL = `fill="currentColor" stroke="none"`;

// Wi-Fi fan centered on (8, 13); arcs span +-45 degrees at radii 3, 6 and 9.
const WIFI_ARCS = [
  "M5.88 10.88A3 3 0 0 1 10.12 10.88",
  "M3.76 8.76A6 6 0 0 1 12.24 8.76",
  "M1.64 6.64A9 9 0 0 1 14.36 6.64",
] as const;
const arcs = (lit: number): string =>
  WIFI_ARCS.map((d, i) => `<path d="${d}" stroke-width="1.5"${i < lit ? "" : ` opacity=".28"`}/>`).join("");

export const icons = {
  /** The mypaper mark: a sheet with a folded corner. */
  mark: svg(
    `<path d="M4.5 1.5H9l4 4V12a2.5 2.5 0 0 1-2.5 2.5h-6A1.5 1.5 0 0 1 3 13V3a1.5 1.5 0 0 1 1.5-1.5Z" ${FILL}/><path d="M9 1.5v4h4Z" fill="currentColor" stroke="none" opacity=".45"/>`,
  ),
  wifi: (level: 1 | 2 | 3 | 4): string => svg(`${arcs(level - 1)}<circle cx="8" cy="13" r="1.15" ${FILL}/>`),
  offline: svg(`${arcs(0)}<circle cx="8" cy="13" r="1.15" ${FILL} opacity=".28"/><path d="M2.5 2.5l11 11"/>`),
  ethernet: svg(`<path d="M2.5 4h11v6.5H11V13H5v-2.5H2.5Z"/><path d="M6 6.5v2M8 6.5v2M10 6.5v2"/>`),
  volume: (level: VolumeGlyph): string => {
    const speaker = `<path d="M2.5 6h2.2L8 3.2v9.6L4.7 10H2.5Z" ${FILL}/>`;
    switch (level) {
      case "mute":
        return svg(`${speaker}<path d="M10.5 6l3.5 4M14 6l-3.5 4"/>`);
      case "low":
        return svg(`${speaker}<path d="M10.4 6.3a2.5 2.5 0 0 1 0 3.4"/>`);
      case "mid":
        return svg(`${speaker}<path d="M10.4 6.3a2.5 2.5 0 0 1 0 3.4"/><path d="M12.1 4.6a5 5 0 0 1 0 6.8"/>`);
      case "high":
        return svg(
          `${speaker}<path d="M10.4 6.3a2.5 2.5 0 0 1 0 3.4"/><path d="M12.1 4.6a5 5 0 0 1 0 6.8"/><path d="M13.7 3a7.3 7.3 0 0 1 0 10"/>`,
        );
    }
  },
  /** Battery outline with a fill proportional to `fraction` (0-1). */
  battery: (fraction: number): string => {
    const w = Math.max(0, Math.min(1, fraction)) * 8.4;
    return svg(
      `<rect x="1.5" y="4.25" width="11.5" height="7.5" rx="2"/><path d="M14.7 7v2"/>${
        w > 0.4 ? `<rect x="3.15" y="5.9" width="${w.toFixed(2)}" height="4.2" rx=".9" ${FILL}/>` : ""
      }`,
    );
  },
  bolt: svg(`<path d="M9 1.8 4.2 9h3.1L6.8 14.2 11.8 7H8.6Z" ${FILL}/>`),
  quick: svg(`<path d="M2 5h6.2M12.2 5H14M2 11h1.8M7.8 11H14"/><circle cx="10.2" cy="5" r="1.9"/><circle cx="5.8" cy="11" r="1.9"/>`),
  power: svg(`<path d="M8 1.8v5.6"/><path d="M4.7 4a5.4 5.4 0 1 0 6.6 0"/>`),
  play: svg(`<path d="M5 3.3v9.4L12.8 8Z" ${FILL}/>`),
  pause: svg(`<rect x="4" y="3" width="3" height="10" rx=".9" ${FILL}/><rect x="9" y="3" width="3" height="10" rx=".9" ${FILL}/>`),
  chevron: svg(`<path d="M4 6.5l4 4 4-4"/>`),
} as const;
