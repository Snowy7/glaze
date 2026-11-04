// Builds the settings form from a manifest, like the shell does in edit mode (simpler, and
// independent of it). Every input change is applied at once, so sliders and colors preview live.
// Picked files become object URLs, which only last until the page reloads.

import { optionValue } from "../src";
import type { ListField, ListSetting, ScalarSetting, SelectOption, SettingDef, Settings } from "../src/types";
import { pickLocalFile } from "./fake";

type Scalar = string | number | boolean;
type Field = ScalarSetting | ListField;

const COMMON_FONTS = ["Segoe UI Variable", "Segoe UI", "Cascadia Code", "Consolas", "Georgia", "Bahnschrift", "Arial", "Verdana", "Trebuchet MS", "Times New Roman", "Courier New", "Impact", "Comic Sans MS"];

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function defaultFor(def: Field): Scalar {
  if (def.default !== undefined) return def.default;
  switch (def.type) {
    case "boolean":
      return false;
    case "number":
    case "slider":
      return def.min ?? 0;
    case "select":
      return optionValue(def.options[0] ?? "");
    case "color":
      return "#ffffff";
    case "string":
    case "font":
      return "";
  }
}

const optionText = (option: SelectOption): string =>
  typeof option === "string" ? option : `${option.icon ? `${option.icon} ` : ""}${option.label ?? option.value}`;

const fileName = (url: string): string => {
  const last = (url.split(/[?#]/, 1)[0] ?? "").split("/").filter(Boolean).at(-1) ?? "";
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
};

/** `#rrggbb` or `#rrggbbaa` -> the colour input's `#rrggbb` and the opacity 0..100. */
function splitColor(value: Scalar): { rgb: string; percent: number } {
  const text = String(value);
  const rgb = /^#[0-9a-f]{6}/i.test(text) ? text.slice(0, 7).toLowerCase() : "#ffffff";
  const alpha = /^#[0-9a-f]{8}$/i.test(text) ? parseInt(text.slice(7), 16) / 255 : 1;
  return { rgb, percent: Math.round(alpha * 100) };
}

function colorControl(alpha: boolean, value: Scalar, set: (v: Scalar) => void): HTMLElement {
  const state = splitColor(value);
  const swatch = el("input");
  swatch.type = "color";
  swatch.value = state.rgb;
  const emit = () => {
    const byte = Math.round((state.percent / 100) * 255).toString(16).padStart(2, "0");
    set(alpha ? state.rgb + byte : state.rgb);
  };
  swatch.addEventListener("input", () => {
    state.rgb = swatch.value;
    emit();
  });
  if (!alpha) return swatch;

  const box = el("div", "range");
  const opacity = el("input");
  opacity.type = "range";
  opacity.min = "0";
  opacity.max = "100";
  opacity.value = String(state.percent);
  opacity.title = "Opacity";
  const val = el("span", "val", `${state.percent}%`);
  opacity.addEventListener("input", () => {
    state.percent = opacity.valueAsNumber;
    val.textContent = `${state.percent}%`;
    emit();
  });
  box.append(swatch, opacity, val);
  return box;
}

function fileControl(def: Extract<Field, { type: "file" | "image" }>, value: Scalar, set: (v: Scalar) => void): HTMLElement {
  const box = el("div", "range");
  const thumb = def.type === "image" ? el("img") : undefined;
  if (thumb) {
    thumb.width = thumb.height = 24;
    thumb.alt = "";
  }
  const name = el("span", "val");
  name.style.flex = "1";
  name.style.textAlign = "left";
  const choose = el("button", undefined, "Choose...");
  const clear = el("button", undefined, "Clear");
  const show = (url: string) => {
    name.textContent = url ? fileName(url) : "None";
    clear.disabled = url === "";
    if (thumb) {
      thumb.hidden = url === "";
      if (url) thumb.src = url;
    }
  };
  show(String(value));
  choose.addEventListener("click", () => {
    void pickLocalFile(def.extensions ? { extensions: [...def.extensions] } : {}).then((file) => {
      if (!file) return;
      show(file.url);
      set(file.url);
    });
  });
  clear.addEventListener("click", () => {
    show("");
    set("");
  });
  box.append(...(thumb ? [thumb] : []), name, choose, clear);
  return box;
}

/** One input for a scalar setting. */
function control(def: Field, value: Scalar, set: (v: Scalar) => void): HTMLElement {
  switch (def.type) {
    case "boolean": {
      const input = el("input");
      input.type = "checkbox";
      input.checked = value === true;
      input.addEventListener("change", () => set(input.checked));
      return input;
    }
    case "select": {
      const select = el("select");
      for (const option of def.options) {
        select.append(new Option(optionText(option), optionValue(option), false, optionValue(option) === value));
      }
      select.addEventListener("change", () => set(select.value));
      return select;
    }
    case "number": {
      const input = el("input");
      input.type = "number";
      if (def.min !== undefined) input.min = String(def.min);
      if (def.max !== undefined) input.max = String(def.max);
      if (def.step !== undefined) input.step = String(def.step);
      input.value = String(value);
      input.addEventListener("input", () => input.value !== "" && set(Number(input.value)));
      return input;
    }
    case "slider": {
      const box = el("div", "range");
      const input = el("input");
      input.type = "range";
      input.min = String(def.min);
      input.max = String(def.max);
      input.step = String(def.step ?? "any");
      input.value = String(value);
      const val = el("span", "val", `${value}${def.unit ?? ""}`);
      input.addEventListener("input", () => {
        val.textContent = `${input.valueAsNumber}${def.unit ?? ""}`;
        set(input.valueAsNumber);
      });
      box.append(input, val);
      return box;
    }
    case "color":
      return colorControl(def.alpha === true, value, set);
    case "font": {
      const input = el("input");
      input.type = "text";
      input.value = String(value);
      const list = el("datalist");
      list.id = `fonts-${Math.random().toString(36).slice(2)}`;
      for (const font of COMMON_FONTS) {
        const option = new Option(font);
        option.style.fontFamily = `"${font}"`;
        list.append(option);
      }
      input.setAttribute("list", list.id);
      input.addEventListener("input", () => set(input.value));
      const box = el("div");
      box.append(input, list);
      return box;
    }
    case "file":
    case "image":
      return fileControl(def, value, set);
    case "string": {
      const input = el("input");
      input.type = "text";
      input.value = String(value);
      input.addEventListener("input", () => set(input.value));
      return input;
    }
  }
}

function labelled(label: string, input: HTMLElement, check = false): HTMLElement {
  const row = el("div", check ? "row check" : "row");
  const l = el("label", undefined, label);
  row.append(...(check ? [input, l] : [l, input]));
  return row;
}

function listEditor(def: ListSetting, items: Record<string, Scalar>[], set: (v: Record<string, Scalar>[]) => void): HTMLElement {
  const box = el("div");
  let rows = items.map((i) => ({ ...i }));
  const render = () => {
    box.replaceChildren();
    rows.forEach((row, index) => {
      const item = el("div", "list-item");
      for (const field of def.fields) {
        const value = row[field.key] ?? defaultFor(field);
        item.append(
          labelled(
            field.label,
            control(field, value, (v) => {
              row[field.key] = v;
              set(rows.map((r) => ({ ...r })));
            }),
            field.type === "boolean",
          ),
        );
      }
      const remove = el("button", undefined, "Remove");
      remove.addEventListener("click", () => {
        rows = rows.filter((_, i) => i !== index);
        set(rows.map((r) => ({ ...r })));
        render();
      });
      item.append(remove);
      box.append(item);
    });
    const add = el("button", undefined, "Add item");
    add.disabled = def.maxItems !== undefined && rows.length >= def.maxItems;
    add.addEventListener("click", () => {
      rows = [...rows, Object.fromEntries(def.fields.map((f) => [f.key, defaultFor(f)]))];
      set(rows.map((r) => ({ ...r })));
      render();
    });
    box.append(add);
  };
  render();
  return box;
}

function isRecordArray(value: unknown): value is Record<string, Scalar>[] {
  return Array.isArray(value) && value.every((v) => typeof v === "object" && v !== null);
}

/** Manifest defaults, overlaid with `values`. */
export function withDefaults(defs: readonly SettingDef[], values: Settings): Settings {
  return { ...Object.fromEntries(defs.map((d) => [d.key, d.default])), ...values };
}

export function settingsForm(defs: readonly SettingDef[], values: Settings, onChange: (s: Settings) => void): HTMLElement {
  const root = el("div");
  const current = withDefaults(defs, values);
  if (defs.length === 0) {
    root.append(el("div", "note", "This widget has no settings."));
    return root;
  }
  const emit = () => onChange({ ...current });

  // Ungrouped settings first, then a heading each time the group changes.
  const ordered = [...defs.filter((d) => d.group === undefined), ...defs.filter((d) => d.group !== undefined)];
  let group: string | undefined;
  for (const def of ordered) {
    if (def.group !== undefined && def.group !== group) root.append(el("h2", undefined, def.group));
    group = def.group;

    if (def.type === "list") {
      root.append(el("div", "note", def.label));
      const value = current[def.key];
      root.append(
        listEditor(def, isRecordArray(value) ? value : [], (v) => {
          current[def.key] = v;
          emit();
        }),
      );
      continue;
    }
    const raw = current[def.key];
    const value = typeof raw === "string" || typeof raw === "number" || typeof raw === "boolean" ? raw : def.default;
    root.append(
      labelled(
        def.label,
        control(def, value, (v) => {
          current[def.key] = v;
          emit();
        }),
        def.type === "boolean",
      ),
    );
  }
  return root;
}
