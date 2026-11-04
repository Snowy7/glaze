// Module lists to elements, shared by the top bar and the dock's `taskbar` style: groups split by
// `|`, built-in modules from their factories, custom modules from the `custom` event.

import { button, h } from "../dom";
import type { Scope } from "../host";
import { MODULES, type BarsConfig, type Module, type ModuleEntry, type ModuleName } from "../protocol";
import { factories } from "./modules";

const isBuiltin = (name: ModuleName): name is Module => MODULES.some((m) => m === name);

/**
 * A zone's modules in groups (islands): split at `|`, empty groups dropped, unknown names and
 * repeats left out (the first occurrence wins, across all the zones given `seen`).
 */
export function groupsOf(entries: readonly ModuleEntry[], seen = new Set<string>()): ModuleName[][] {
  const groups: ModuleName[][] = [[]];
  for (const entry of entries) {
    if (entry === "|") {
      groups.push([]);
      continue;
    }
    const known = isBuiltin(entry) || /^custom:[a-z0-9_-]{1,32}$/.test(entry);
    if (!known || seen.has(entry)) continue;
    seen.add(entry);
    groups[groups.length - 1]!.push(entry);
  }
  return groups.filter((group) => group.length > 0);
}

/** A custom module: what its command printed, as text with an optional icon. */
function custom(id: string, scope: Scope): HTMLElement {
  const icon = h("span", "glyph");
  icon.hidden = true;
  const text = h("span", "custom-text");
  const el = button("mod custom", icon, text);
  scope.on("custom", (outputs) => {
    const out = outputs[id];
    el.hidden = !out || (!out.text && !out.icon && !out.error);
    if (!out) return;
    text.textContent = out.error ? "—" : out.text;
    el.title = out.tooltip ?? out.text;
    el.dataset.error = String(!!out.error);
    if (out.class) el.dataset.class = out.class;
    else delete el.dataset.class;
    const iconText = out.icon ?? "";
    icon.hidden = !iconText;
    // Short icons are text (an emoji); longer ones name an icon-pack key.
    if ([...iconText].length <= 2) {
      icon.className = "glyph custom-emoji";
      icon.textContent = iconText;
    } else {
      icon.className = "glyph";
      icon.innerHTML = `<svg data-glyph="${iconText.replace(/[^a-z0-9-]/g, "")}" viewBox="0 0 16 16"></svg>`;
    }
  });
  return el;
}

export function buildModule(name: ModuleName, config: BarsConfig, scope: Scope): HTMLElement {
  const el = isBuiltin(name) ? factories[name]({ config, scope }) : custom(name.slice("custom:".length), scope);
  el.dataset.module = name;
  return el;
}
