// Per-module style overrides (docs/bars.md, "Modules"): `bars.json` → `modules.<name>.style` and
// `.icon` as CSS rules on `.mod[data-module="<name>"]`. Pure, so it can be tested.

import type { BarsConfig, IconPack, ModuleName, ModuleOptions } from "../protocol";
import { cssColor, paintImage, paintLayers } from "./look";
import { glyphImageCss, iconUrl, isMono, matchKey } from "./icon-pack";

const safeName = (name: string): boolean => /^[a-z0-9:_-]+$/.test(name);

function styleRules(options: ModuleOptions): string[] {
  const { color, background, border, padding, radius, size, weight } = options.style;
  const rules: string[] = [];
  if (color) rules.push(`--i-fg: ${cssColor(color)}`, `--i-fg-hover: ${cssColor(color)}`, `color: ${cssColor(color)}`);
  if (background) {
    const [color, image] = paintLayers(background);
    rules.push(`--i-bgc: ${color}`, `--i-bgi: ${image}`);
  }
  if (border?.width !== undefined) rules.push(`--i-bw: ${border.width}px`);
  if (border?.color) rules.push(`--i-bi: ${paintImage(border.color)}`);
  if (padding !== undefined) rules.push(`padding-inline: ${padding}px`);
  if (radius !== undefined) rules.push(`--i-radius: ${radius}px`);
  if (size !== undefined) rules.push(`font-size: ${size}px`);
  if (weight !== undefined) rules.push(`font-weight: ${weight}`);
  return rules;
}

/** The icon a module's `icon` names: a pack key, or an image file served from `assets`. */
function iconCss(
  icon: string,
  pack: IconPack | null,
  assets: Record<string, string>,
): string | undefined {
  const file = assets[icon];
  if (file) return glyphImageCss(file, icon.toLowerCase().endsWith(".svg"));
  const entry = pack ? matchKey(pack, icon) : undefined;
  return entry && pack ? glyphImageCss(iconUrl(pack, entry), isMono(pack, entry)) : undefined;
}

/** CSS for every module's overrides. */
export function moduleCss(
  modules: BarsConfig["modules"],
  pack: IconPack | null,
  assets: Record<string, string>,
): string {
  const out: string[] = [];
  for (const [name, options] of Object.entries(modules) as [ModuleName, ModuleOptions | undefined][]) {
    if (!options || !safeName(name)) continue;
    const selector = `html .mod[data-module="${name}"]`;
    const rules = styleRules(options);
    if (rules.length) out.push(`${selector} { ${rules.join("; ")}; }`);
    const icon = options.icon ? iconCss(options.icon, pack, assets) : undefined;
    if (icon) {
      out.push(`${selector} .glyph:not(.small) { ${icon} }`, `${selector} .glyph:not(.small) > svg { visibility: hidden; }`);
    }
  }
  return out.join("\n");
}
