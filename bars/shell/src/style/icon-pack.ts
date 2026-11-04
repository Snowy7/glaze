// Icon packs (docs/bars.md, "Icon packs"): which pack icon an app gets, and the CSS that swaps the
// built-in glyphs for a pack's (`.glyph > svg[data-glyph="<key>"]`, see icons.ts). Mono icons are
// drawn as a mask in the text color; others as pictures.

import type { IconPack, IconPackEntry } from "../protocol";

export interface AppIdentity {
  aumid?: string;
  /** The process image (lowercase). */
  exe?: string;
  /** What the item launches: an exe, a `.lnk`, a folder or a file. */
  path?: string;
}

const fileName = (path: string): string => path.slice(Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/")) + 1);
const extensionOf = (path: string): string | undefined => {
  const name = fileName(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : undefined;
};
const same = (a: string | undefined, b: string | undefined): boolean =>
  a !== undefined && b !== undefined && a.toLowerCase() === b.toLowerCase();

/**
 * The entry for an app, most specific first: its AppUserModelID, its exe or path in full, its
 * exe or path's file name, the path's extension, then the pack's `missing` icon.
 */
export function matchApp(pack: IconPack, app: AppIdentity): IconPackEntry | undefined {
  const { entries } = pack;
  const candidates = [app.exe, app.path].filter((p): p is string => !!p);
  const byUmid = app.aumid ? entries.find((e) => same(e.umid, app.aumid)) : undefined;
  const byPath = entries.find((e) => e.path && /[\\/]/.test(e.path) && candidates.some((p) => same(e.path, p)));
  const byName = entries.find(
    (e) => e.path && !/[\\/]/.test(e.path) && candidates.some((p) => same(e.path, fileName(p))),
  );
  const extension = app.path ? extensionOf(app.path) : undefined;
  const byExtension = extension ? entries.find((e) => same(e.extension, extension)) : undefined;
  const missing = pack.missing ? { icon: pack.missing } : undefined;
  return byUmid ?? byPath ?? byName ?? byExtension ?? missing;
}

export function matchKey(pack: IconPack, key: string): IconPackEntry | undefined {
  return pack.entries.find((e) => e.key === key);
}

export const iconUrl = (pack: IconPack, entry: IconPackEntry): string =>
  pack.base + entry.icon.split(/[\\/]/).map(encodeURIComponent).join("/");

export const isMono = (pack: IconPack, entry: IconPackEntry): boolean => entry.mono ?? pack.mono;

/** CSS that draws an icon file into a `.glyph` box: as a mask in the text color, or a picture. */
export function glyphImageCss(url: string, mono: boolean): string {
  const image = `url("${url}") center / contain no-repeat`;
  return mono ? `background-color: currentColor; -webkit-mask: ${image}; mask: ${image};` : `background: ${image};`;
}

/** Rules that replace each built-in glyph the pack has a `key` for. */
export function packGlyphCss(pack: IconPack): string {
  return pack.entries
    .filter((e): e is IconPackEntry & { key: string } => !!e.key && /^[a-z0-9-]+$/.test(e.key))
    .map((e) => {
      const target = `.glyph:has(> svg[data-glyph="${e.key}"])`;
      return `${target} { ${glyphImageCss(iconUrl(pack, e), isMono(pack, e))} }\n${target} > svg { visibility: hidden; }`;
    })
    .join("\n");
}
