// A form generated from a widget manifest's `settings`. Each change is reported on its own
// (`onChange(key, value)`); the host answers with a fresh `state`, so the form never re-renders.
// A `list` setting reports its whole array on every edit.

import { h } from "./dom";
import type { ListField, ScalarSetting, SettingDef } from "./protocol";

export interface SettingsForm {
  el: HTMLElement;
  /** Sends any change still waiting on a debounce or throttle (call when closing the form). */
  flush(): void;
}

type Scalar = string | number | boolean;
type FieldDef = ScalarSetting | ListField;
type Row = Record<string, Scalar>;

const HEX = /^#[0-9a-f]{6}$/i;

interface Limiter<T> {
  push(value: T): void;
  flush(): void;
}

/** Debounce waits for quiet; throttle sends the first value at once, then at most one per `ms`. */
function rateLimited<T>(fn: (value: T) => void, ms: number, mode: "debounce" | "throttle"): Limiter<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: { value: T } | undefined;
  const fire = () => {
    timer = undefined;
    if (!queued) return;
    const { value } = queued;
    queued = undefined;
    fn(value);
    if (mode === "throttle") timer = setTimeout(fire, ms);
  };
  return {
    push(value) {
      if (mode === "throttle" && timer === undefined) {
        fn(value);
        timer = setTimeout(fire, ms);
        return;
      }
      queued = { value };
      if (mode === "debounce") {
        clearTimeout(timer);
        timer = setTimeout(fire, ms);
      }
    },
    flush() {
      clearTimeout(timer);
      timer = undefined;
      fire();
    },
  };
}

type MakeLimiter = <T>(fn: (value: T) => void, ms: number, mode: "debounce" | "throttle") => Limiter<T>;

const pretty = (n: number) => String(Math.round(n * 1000) / 1000);

// Value readers: a saved value of the right type, else the default, else the type's zero.
const readBool = (value: unknown, def: unknown) => (typeof value === "boolean" ? value : def === true);
const readString = (value: unknown, def: unknown) => (typeof value === "string" ? value : typeof def === "string" ? def : "");
const readNumber = (value: unknown, def: unknown, min: number | undefined) =>
  typeof value === "number" ? value : typeof def === "number" ? def : (min ?? 0);

/** What a new list row gets for a field: its default, else false / 0 (or min) / "" / first option / "#ffffff". */
function fieldDefault(field: ListField): Scalar {
  switch (field.type) {
    case "boolean":
      return readBool(undefined, field.default);
    case "number":
      return readNumber(undefined, field.default, field.min);
    case "string":
      return readString(undefined, field.default);
    case "select":
      return typeof field.default === "string" ? field.default : (field.options[0] ?? "");
    case "color":
      return typeof field.default === "string" && HEX.test(field.default) ? field.default : "#ffffff";
  }
}

/** One labelled input for a scalar setting (or a list field). `emit` gets each new value. */
function renderField(def: FieldDef, value: unknown, emit: (value: Scalar) => void, limit: MakeLimiter): HTMLElement {
  const title = def.label || def.key;

  switch (def.type) {
    case "boolean": {
      let on = readBool(value, def.default);
      const toggle = h("button", "switch");
      toggle.type = "button";
      toggle.setAttribute("role", "switch");
      toggle.setAttribute("aria-label", title);
      toggle.setAttribute("aria-checked", String(on));
      toggle.addEventListener("click", () => {
        on = !on;
        toggle.setAttribute("aria-checked", String(on));
        emit(on);
      });
      return h("div", "field inline", h("span", "t", title), toggle);
    }

    case "number": {
      const initial = readNumber(value, def.default, def.min);
      const send = limit<number>(emit, 100, "throttle");
      const input = h("input");
      input.setAttribute("aria-label", title);
      input.step = String(def.step ?? "any");
      if (def.min !== undefined) input.min = String(def.min);
      if (def.max !== undefined) input.max = String(def.max);

      if (def.min !== undefined && def.max !== undefined) {
        const { min, max } = def;
        const readout = h("output", "readout", pretty(initial));
        input.type = "range";
        input.value = String(initial);
        const paint = () => input.style.setProperty("--p", `${((input.valueAsNumber - min) / (max - min || 1)) * 100}%`);
        paint();
        input.addEventListener("input", () => {
          paint();
          readout.value = pretty(input.valueAsNumber);
          send.push(input.valueAsNumber);
        });
        input.addEventListener("change", send.flush);
        return h("div", "field", h("div", "row", h("span", "t", title), readout), input);
      }

      input.type = "number";
      input.value = String(initial);
      input.addEventListener("input", () => {
        const n = input.valueAsNumber;
        const inRange = (def.min === undefined || n >= def.min) && (def.max === undefined || n <= def.max);
        if (Number.isFinite(n) && inRange) send.push(n);
      });
      input.addEventListener("change", send.flush);
      return h("div", "field", h("span", "t", title), input);
    }

    case "string": {
      const send = limit<string>(emit, 300, "debounce");
      const input = h("input");
      input.type = "text";
      input.value = readString(value, def.default);
      input.setAttribute("aria-label", title);
      input.spellcheck = false;
      input.addEventListener("input", () => send.push(input.value));
      input.addEventListener("change", send.flush);
      return h("div", "field", h("span", "t", title), input);
    }

    case "select": {
      const options = def.options;
      let current = typeof value === "string" && options.includes(value) ? value : fieldDefault(def);
      if (options.length <= 3) {
        const group = h("div", "seg");
        group.setAttribute("role", "radiogroup");
        group.setAttribute("aria-label", title);
        const buttons = options.map((option) => {
          const button = h("button", "", option);
          button.type = "button";
          button.setAttribute("role", "radio");
          button.addEventListener("click", () => {
            current = option;
            mark();
            emit(option);
          });
          return { option, button };
        });
        const mark = () => {
          for (const { option, button } of buttons) button.setAttribute("aria-checked", String(option === current));
        };
        mark();
        group.append(...buttons.map((b) => b.button));
        return h("div", "field", h("span", "t", title), group);
      }
      const select = h("select");
      select.setAttribute("aria-label", title);
      for (const option of options) select.append(new Option(option, option, false, option === current));
      select.addEventListener("change", () => emit(select.value));
      return h("div", "field", h("span", "t", title), select);
    }

    case "color": {
      const send = limit<string>(emit, 100, "throttle");
      const swatch = h("input", "swatch");
      swatch.type = "color";
      swatch.value = typeof value === "string" && HEX.test(value) ? value : String(fieldDefault(def));
      swatch.setAttribute("aria-label", title);
      const hex = h("input", "hex");
      hex.type = "text";
      hex.value = swatch.value;
      hex.maxLength = 7;
      hex.spellcheck = false;
      hex.setAttribute("aria-label", `${title} (hex)`);
      swatch.addEventListener("input", () => {
        hex.value = swatch.value;
        send.push(swatch.value);
      });
      swatch.addEventListener("change", send.flush);
      hex.addEventListener("input", () => {
        if (!HEX.test(hex.value)) return;
        swatch.value = hex.value.toLowerCase();
        send.push(swatch.value);
      });
      hex.addEventListener("change", () => {
        hex.value = swatch.value; // snap back if the text wasn't a valid color
        send.flush();
      });
      return h("div", "field", h("span", "t", title), h("div", "row gap", swatch, hex));
    }
  }
}

/** The rows of a saved list value, with missing or mistyped fields filled from the defaults. */
function readRows(saved: unknown, fields: readonly ListField[], fallback: unknown): Row[] {
  const source: unknown[] = Array.isArray(saved) ? saved : Array.isArray(fallback) ? fallback : [];
  return source.map((raw) => {
    const record = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
    return Object.fromEntries(fields.map((field) => [field.key, validValue(field, record[field.key]) ?? fieldDefault(field)]));
  });
}

/** `value` if it fits the field's type, else `undefined`. */
function validValue(field: ListField, value: unknown): Scalar | undefined {
  switch (field.type) {
    case "boolean":
      return typeof value === "boolean" ? value : undefined;
    case "number":
      return typeof value === "number" ? value : undefined;
    case "select":
      return typeof value === "string" && field.options.includes(value) ? value : undefined;
    case "color":
      return typeof value === "string" && HEX.test(value) ? value : undefined;
    case "string":
      return typeof value === "string" ? value : undefined;
  }
}

function renderList(
  def: Extract<SettingDef, { type: "list" }>,
  saved: unknown,
  onChange: (key: string, value: unknown) => void,
  limit: MakeLimiter,
): HTMLElement {
  let rows = readRows(saved, def.fields, def.default);
  const title = def.label || def.key;
  const body = h("div", "list");
  const add = h("button", "btn", "Add");
  add.type = "button";
  const count = h("span", "t");

  const commit = () => onChange(def.key, rows.map((r) => ({ ...r })));

  function paint(focusLast = false) {
    body.replaceChildren(
      ...rows.map((row, index) => {
        const remove = h("button", "icon-btn close");
        remove.type = "button";
        remove.setAttribute("aria-label", `Remove ${title} item ${index + 1}`);
        remove.addEventListener("click", () => {
          rows = rows.filter((r) => r !== row);
          commit();
          paint();
        });
        const fields = def.fields.map((field) =>
          renderField(
            field,
            row[field.key],
            (value) => {
              row[field.key] = value; // the row object, not its index: it may have moved since
              commit();
            },
            limit,
          ),
        );
        return h("div", "list-row", h("div", "row", h("span", "t", `${index + 1}`), remove), ...fields);
      }),
    );
    const full = def.maxItems !== undefined && rows.length >= def.maxItems;
    add.disabled = full;
    count.textContent = def.maxItems === undefined ? `${rows.length}` : `${rows.length} / ${def.maxItems}`;
    if (focusLast) body.querySelector<HTMLElement>(".list-row:last-child input, .list-row:last-child select")?.focus();
  }

  add.addEventListener("click", () => {
    if (def.maxItems !== undefined && rows.length >= def.maxItems) return;
    rows = [...rows, Object.fromEntries(def.fields.map((f) => [f.key, fieldDefault(f)]))];
    commit();
    paint(true);
  });

  paint();
  return h("div", "field", h("div", "row", h("span", "t", title), count), body, add);
}

export function buildSettingsForm(
  specs: readonly SettingDef[],
  values: Record<string, unknown>,
  onChange: (key: string, value: unknown) => void,
): SettingsForm {
  const el = h("div", "form");
  const limiters: Limiter<never>[] = [];

  const limit: MakeLimiter = (fn, ms, mode) => {
    const limiter = rateLimited(fn, ms, mode);
    limiters.push(limiter);
    return limiter;
  };

  for (const spec of specs) {
    el.append(
      spec.type === "list"
        ? renderList(spec, values[spec.key], onChange, limit)
        : renderField(spec, values[spec.key], (value) => onChange(spec.key, value), limit),
    );
  }

  return { el, flush: () => limiters.forEach((l) => l.flush()) };
}
