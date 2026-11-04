// The harness's appearance: read from the URL, and the panel controls that edit it.
//   surface=none|glass|solid  style=minimal|depth|neon|paper|aero  text=light|dark  scale=<0.5..2>  shadow=0|1  accent=<rrggbb>
//   appearance=<json, any Appearance fields>  themecss=<css>

import { DEFAULT_APPEARANCE, WIDGET_STYLES, normalizeAppearance } from "../src";
import type { Appearance, AppearanceState } from "../src/types";

/** Appearance fields from query params; the shortcuts win over the `appearance` JSON. */
export function lookFromParams(q: URLSearchParams): AppearanceState {
  let fields: Partial<Record<keyof Appearance, unknown>> = {};
  try {
    const parsed: unknown = JSON.parse(q.get("appearance") ?? "{}");
    if (typeof parsed === "object" && parsed !== null) fields = { ...parsed };
  } catch {
    /* ignore a malformed param */
  }
  const num = (key: string) => (q.has(key) ? Number(q.get(key)) : undefined);
  const shortcuts: Partial<Record<keyof Appearance, unknown>> = {
    surface: q.get("surface") ?? undefined,
    style: q.get("style") ?? undefined,
    text: q.get("text") ?? undefined,
    scale: num("scale"),
    opacity: num("opacity"),
    blur: num("blur"),
    radius: num("radius"),
    shadow: q.has("shadow") ? q.get("shadow") !== "0" : undefined,
    border: q.has("border") ? q.get("border") !== "0" : undefined,
    accent: q.has("accent") ? `#${q.get("accent")}` : undefined,
    tint: q.has("tint") ? `#${q.get("tint")}` : undefined,
  };
  for (const [key, value] of Object.entries(shortcuts)) {
    if (value !== undefined) fields[key as keyof Appearance] = value;
  }
  return { appearance: normalizeAppearance(fields), themeCss: q.get("themecss") };
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function row(label: string, control: HTMLElement): HTMLElement {
  const r = el("div", "row");
  r.append(el("label", undefined, label), control);
  return r;
}

function select<T extends string>(options: readonly T[], value: T, onChange: (v: T) => void): HTMLSelectElement {
  const s = el("select");
  for (const o of options) s.append(new Option(o, o, false, o === value));
  s.addEventListener("change", () => {
    const picked = options.find((o) => o === s.value);
    if (picked) onChange(picked);
  });
  return s;
}

function range(value: number, min: number, max: number, step: number, onChange: (v: number) => void): HTMLElement {
  const wrap = el("div", "range");
  const input = el("input");
  input.type = "range";
  input.min = String(min);
  input.max = String(max);
  input.step = String(step);
  input.value = String(value);
  const out = el("span", "val", String(value));
  input.addEventListener("input", () => {
    out.textContent = input.value;
    onChange(Number(input.value));
  });
  wrap.append(input, out);
  return wrap;
}

function check(value: boolean, onChange: (v: boolean) => void): HTMLInputElement {
  const i = el("input");
  i.type = "checkbox";
  i.checked = value;
  i.addEventListener("change", () => onChange(i.checked));
  return i;
}

function color(value: string, onChange: (v: string) => void): HTMLInputElement {
  const i = el("input");
  i.type = "color";
  i.value = value;
  i.addEventListener("input", () => onChange(i.value));
  return i;
}

/** The panel section: edits `look` in place and calls `changed` after each edit. */
export function appearanceForm(look: AppearanceState, changed: () => void): HTMLElement {
  const root = el("div");
  const a = look.appearance;
  const set = <K extends keyof Appearance>(key: K, value: Appearance[K]) => {
    look.appearance = { ...look.appearance, [key]: value };
    changed();
  };
  const reset = el("button", undefined, "Defaults");
  reset.addEventListener("click", () => {
    look.appearance = { ...DEFAULT_APPEARANCE };
    changed();
    root.replaceWith(appearanceForm(look, changed));
  });
  const css = el("textarea");
  css.rows = 3;
  css.placeholder = "theme CSS, e.g. [data-widget=clock] .time { font-weight: 700 }";
  css.value = look.themeCss ?? "";
  css.addEventListener("input", () => {
    look.themeCss = css.value || null;
    changed();
  });
  root.append(
    row("Style", select(WIDGET_STYLES, a.style, (v) => set("style", v))),
    row("Surface", select(["none", "glass", "solid"], a.surface, (v) => set("surface", v))),
    row("Text", select(["light", "dark"], a.text, (v) => set("text", v))),
    row("Scale", range(a.scale, 0.5, 2, 0.05, (v) => set("scale", v))),
    row("Tint", color(a.tint, (v) => set("tint", v))),
    row("Opacity", range(a.opacity, 0, 1, 0.05, (v) => set("opacity", v))),
    row("Blur", range(a.blur, 0, 64, 1, (v) => set("blur", v))),
    row("Radius", range(a.radius, 0, 48, 1, (v) => set("radius", v))),
    row("Accent", color(a.accent, (v) => set("accent", v))),
    row("Border", check(a.border, (v) => set("border", v))),
    row("Text shadow", check(a.shadow, (v) => set("shadow", v))),
    css,
    reset,
  );
  return root;
}
