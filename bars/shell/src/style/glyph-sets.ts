// Glyph sets (docs/bars.md, "Look" → `glyphs`): the built-in glyphs (icons.ts) swapped by key
// with CSS alone, like an icon pack. `fluent` draws Windows' own Segoe Fluent Icons font (pixel-
// exact on Windows 11; Segoe MDL2 Assets on 10); `symbols` draws SF Symbols-style outlines as
// masks in the text color. Keys an icon pack covers are left to the pack. Pure, so it can be
// tested.

import type { Glyphs } from "../protocol";

/** Segoe Fluent Icons codepoints by glyph key (checked against the font on Windows 11). */
export const FLUENT: Readonly<Record<string, string>> = {
  search: "E721",
  "wifi-1": "E872",
  "wifi-2": "E873",
  "wifi-3": "E874",
  "wifi-4": "E701",
  offline: "F384",
  ethernet: "E839",
  "volume-mute": "E74F",
  "volume-low": "E993",
  "volume-mid": "E994",
  "volume-high": "E995",
  bolt: "E945",
  quick: "E9E9",
  power: "E7E8",
  play: "E768",
  pause: "E769",
  chevron: "E70D",
  desktop: "E7F4",
  trash: "E74D",
  "trash-full": "E74D",
  // The flyouts' glyphs (flyouts/glyphs.ts).
  "fly-search": "E721",
  "fly-wifi": "E701",
  "fly-wifi-1": "E872",
  "fly-wifi-2": "E873",
  "fly-wifi-3": "E874",
  "fly-wifi-4": "E701",
  "fly-bluetooth": "E702",
  "fly-plane": "E709",
  "fly-moon": "E708",
  "fly-sun": "E706",
  "fly-leaf": "F1E8",
  "fly-gauge": "EC48",
  "fly-focus": "E81D",
  "fly-nightlight": "F08C",
  "fly-saver": "F1E8",
  "fly-gear": "E713",
  "fly-chevron-right": "E76C",
  "fly-chevron-left": "E76B",
  "fly-chevron-down": "E70D",
  "fly-chevron-up": "E70E",
  "fly-check": "E73E",
  "fly-close": "E711",
  "fly-lock": "E72E",
  "fly-bell": "EA8F",
  "fly-bell-off": "E7ED",
  "fly-globe": "E774",
  "fly-keyboard": "E765",
  "fly-calendar": "E787",
  "fly-speaker": "E995",
  "fly-speaker-mute": "E74F",
  "fly-mixer": "E9E9",
  "fly-ethernet": "E839",
  "fly-vpn": "E705",
  "fly-refresh": "E72C",
  "fly-power": "E7E8",
  "fly-apps": "F0E2",
  "fly-file": "E8A5",
  "fly-clipboard": "E77F",
  "fly-image": "E91B",
  "fly-link": "E71B",
  "fly-play": "E768",
  "fly-pause": "E769",
  "fly-next": "E893",
  "fly-previous": "E892",
  "fly-pin": "E718",
  "fly-user": "E77B",
  "fly-overflow": "E70E",
};

/** Battery0 … Battery9, then Battery10 (full). */
const FLUENT_BATTERY = ["E850", "E851", "E852", "E853", "E854", "E855", "E856", "E857", "E858", "E859", "E83F"];

const svgUrl = (inner: string): string =>
  `url("data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.55" stroke-linecap="round" stroke-linejoin="round">${inner}</svg>`,
  )}")`;
const F = `fill="#000" stroke="none"`;

/** A Wi-Fi fan of `lit` (0–3) solid arcs over faint ones, SF style. */
const wifi = (lit: number): string => {
  const arc = (r: number, i: number) => {
    const a = (Math.PI / 4) * 1.02;
    const [x0, y0, x1, y1] = [8 - r * Math.sin(a), 13 - r * Math.cos(a), 8 + r * Math.sin(a), 13 - r * Math.cos(a)];
    return `<path d="M${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 0 1 ${x1.toFixed(2)} ${y1.toFixed(2)}" stroke-width="1.9"${i < lit ? "" : ` stroke-opacity=".3"`}/>`;
  };
  return svgUrl(`${[3.4, 6.6, 9.8].map(arc).join("")}<circle cx="8" cy="13" r="1.35" ${F}/>`);
};

/** A horizontal battery filled to `level` tenths, macOS menu bar style. */
const battery = (level: number): string => {
  const w = (level / 10) * 9.6;
  return svgUrl(
    `<rect x="1" y="4.6" width="12.6" height="6.8" rx="2.1" stroke-width="1.1" stroke-opacity=".55"/>` +
      `<path d="M14.9 6.9v2.2" stroke-width="1.3" stroke-opacity=".55"/>` +
      (w > 0.3 ? `<rect x="2.5" y="6.1" width="${w.toFixed(2)}" height="3.8" rx="1" ${F}/>` : ""),
  );
};

const speaker = `<path d="M2 6.1h2.3l3.2-2.8v9.4l-3.2-2.8H2Z" ${F}/>`;

/** SF Symbols-style outlines by glyph key. */
export const SYMBOLS: Readonly<Record<string, string>> = {
  search: svgUrl(`<circle cx="6.8" cy="6.8" r="4.6" stroke-width="1.7"/><path d="M10.3 10.3l3.9 3.9" stroke-width="1.9"/>`),
  "wifi-1": wifi(1),
  "wifi-2": wifi(2),
  "wifi-3": wifi(2),
  "wifi-4": wifi(3),
  offline: svgUrl(`${[6.6, 9.8].map((r) => `<path d="M${(8 - r * 0.7).toFixed(2)} ${(13 - r * 0.7).toFixed(2)}A${r} ${r} 0 0 1 ${(8 + r * 0.7).toFixed(2)} ${(13 - r * 0.7).toFixed(2)}" stroke-width="1.9" stroke-opacity=".35"/>`).join("")}<path d="M2.5 2.5l11 11" stroke-width="1.7"/>`),
  ethernet: svgUrl(`<rect x="3" y="2.5" width="10" height="7.5" rx="1.6"/><path d="M8 10v3.5M4.5 13.5h7M6 5.5v1.8M8 5.5v1.8M10 5.5v1.8"/>`),
  "volume-mute": svgUrl(`${speaker}<path d="M10.6 6.2l3.6 3.6M14.2 6.2l-3.6 3.6" stroke-width="1.5"/>`),
  "volume-low": svgUrl(`${speaker}<path d="M10.2 6.2a2.6 2.6 0 0 1 0 3.6"/>`),
  "volume-mid": svgUrl(`${speaker}<path d="M10.2 6.2a2.6 2.6 0 0 1 0 3.6M12.1 4.4a5.2 5.2 0 0 1 0 7.2"/>`),
  "volume-high": svgUrl(`${speaker}<path d="M10.2 6.2a2.6 2.6 0 0 1 0 3.6M12.1 4.4a5.2 5.2 0 0 1 0 7.2M13.9 2.7a7.7 7.7 0 0 1 0 10.6"/>`),
  bolt: svgUrl(`<path d="M9.2 1.4 3.9 9.1h3.5l-.9 5.5 5.6-8.1H8.6Z" ${F}/>`),
  // Control Center: two switches.
  quick: svgUrl(`<rect x="1.6" y="2.6" width="12.8" height="4.6" rx="2.3" stroke-width="1.3"/><circle cx="11.9" cy="4.9" r="1.45" ${F}/><rect x="1.6" y="8.8" width="12.8" height="4.6" rx="2.3" ${F}/><circle cx="4.1" cy="11.1" r="1.45" fill="#fff" stroke="none"/>`),
  power: svgUrl(`<path d="M8 1.6v6" stroke-width="1.7"/><path d="M4.5 3.8a5.6 5.6 0 1 0 7 0" stroke-width="1.7"/>`),
  play: svgUrl(`<path d="M4.6 2.8c0-.7.8-1.1 1.4-.7l7.3 5.2c.5.4.5 1.1 0 1.4L6 13.9c-.6.4-1.4 0-1.4-.7Z" ${F}/>`),
  pause: svgUrl(`<rect x="3.6" y="2.6" width="3.1" height="10.8" rx="1" ${F}/><rect x="9.3" y="2.6" width="3.1" height="10.8" rx="1" ${F}/>`),
  chevron: svgUrl(`<path d="M3.8 6.2 8 10.4l4.2-4.2" stroke-width="1.7"/>`),
  desktop: svgUrl(`<rect x="1.8" y="2.6" width="12.4" height="8.6" rx="1.8"/><path d="M5.6 13.6h4.8"/>`),
  trash: svgUrl(`<path d="M2.6 4.2h10.8M6 4.2V2.9c0-.4.3-.7.7-.7h2.6c.4 0 .7.3.7.7v1.3M3.9 4.2l.8 8.7c.1.7.6 1.2 1.3 1.2h4c.7 0 1.2-.5 1.3-1.2l.8-8.7"/>`),
  "trash-full": svgUrl(`<path d="M2.6 4.2h10.8M6 4.2V2.9c0-.4.3-.7.7-.7h2.6c.4 0 .7.3.7.7v1.3M3.9 4.2l.8 8.7c.1.7.6 1.2 1.3 1.2h4c.7 0 1.2-.5 1.3-1.2l.8-8.7"/><path d="M5.6 7.6h4.8M5.9 10.2h4.2"/>`),
};

const box = (key: string, level?: number) =>
  `.glyph:has(> svg[data-glyph="${key}"${level === undefined ? "" : `][data-level="${level}"`}])`;

/** The rules that draw `set` for every key it has that `covered` (an icon pack's keys) lacks. */
export function glyphSetCss(set: Glyphs, covered: ReadonlySet<string> = new Set()): string {
  const rules: string[] = [];
  if (set === "fluent") {
    // The font goes on the pseudo-elements: on the box itself it would resize it (1.2em).
    const font = `font: normal 1.2em/1 "Segoe Fluent Icons", "Segoe MDL2 Assets"; grid-area: 1 / 1;`;
    const draw = (target: string, code: string) =>
      rules.push(
        `${target} { display: grid; place-items: center; }`,
        `${target}::before { content: "\\${code}"; ${font} }`,
        `${target} > svg { display: none; }`,
      );
    for (const [key, code] of Object.entries(FLUENT)) if (!covered.has(key)) draw(box(key), code);
    // Wi-Fi: the level over the faint full fan, as the taskbar draws it.
    for (const key of ["wifi-1", "wifi-2", "wifi-3", "fly-wifi-1", "fly-wifi-2", "fly-wifi-3"]) {
      if (!covered.has(key)) rules.push(`${box(key)}::after { content: "\\E701"; ${font} opacity: .3; }`);
    }
    if (!covered.has("battery")) FLUENT_BATTERY.forEach((code, level) => draw(box("battery", level), code));
  } else if (set === "symbols") {
    const draw = (target: string, url: string) =>
      rules.push(
        `${target} { background-color: currentColor; -webkit-mask: ${url} center / contain no-repeat; mask: ${url} center / contain no-repeat; }`,
        `${target} > svg { visibility: hidden; }`,
      );
    for (const [key, url] of Object.entries(SYMBOLS)) if (!covered.has(key)) draw(box(key), url);
    if (!covered.has("battery")) for (let level = 0; level <= 10; level++) draw(box("battery", level), battery(level));
  }
  return rules.join("\n");
}
