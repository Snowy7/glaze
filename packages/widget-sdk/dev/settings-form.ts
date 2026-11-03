// Builds the settings form from a manifest, like the shell does in edit mode.

import type { ListField, ListSetting, ScalarSetting, SettingDef, Settings } from "../src/types";

type Scalar = string | number | boolean;
type Field = ScalarSetting | ListField;

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
      return def.min ?? 0;
    case "select":
      return def.options[0] ?? "";
    case "color":
      return "#ffffff";
    case "string":
      return "";
  }
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
      for (const option of def.options) select.append(new Option(option, option, false, option === value));
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
    case "color": {
      const input = el("input");
      input.type = "color";
      input.value = typeof value === "string" ? value : "#ffffff";
      input.addEventListener("input", () => set(input.value));
      return input;
    }
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
  for (const def of defs) {
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
