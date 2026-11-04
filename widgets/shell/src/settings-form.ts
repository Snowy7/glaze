// A form generated from a widget manifest's `settings`. Each change is reported on its own
// (`onChange(key, value)`). The form keeps its own copy of what it shows and does not re-render
// when the host answers; only `update(values)` refreshes the rows that change on the host's side
// (a picked file). A `list` setting reports its whole array on every edit.
//
// Sliders and colors preview while they move (`onPreview`: the widget sees the value, nothing is
// saved) and save once on release. Inside a `list` they save throttled instead.

import { h } from "./dom";
import type { ListField, Place, PlaceSetting, ScalarSetting, SettingDef } from "./protocol";
import {
  COMMON_FONTS,
  decodeColor,
  encodeColor,
  fileName,
  fontFamilyCss,
  formatReadout,
  isFontName,
  isHexColor,
  optionIcon,
  optionLabel,
  optionText,
  optionValue,
  placeLine,
  placeOf,
  readCandidates,
  readPlace,
  sections,
  selectStyle,
} from "./settings-model";

export interface SettingsFormHooks {
  /** Saves a setting. */
  onChange(key: string, value: unknown): void;
  /** Shows a value in the widget without saving it. */
  onPreview(key: string, value: unknown): void;
  /** A `file`/`image` setting: the host shows the dialog, copies the file and sets the setting. */
  onPickFile(key: string, extensions: readonly string[] | undefined): void;
  /** A `place` setting's suggestions: the widget's `plugin` `method` called with `{ query }`. */
  onSearch?(plugin: string, method: string, query: string): Promise<unknown>;
}

export interface SettingsForm {
  el: HTMLElement;
  /** Sends any change still waiting on a debounce or throttle, and saves previewed values (call when closing the form). */
  flush(): void;
  /** Call with the host's settings after each `state`: refreshes the file rows. */
  update(values: Record<string, unknown>): void;
}

type Scalar = string | number | boolean;
type FieldDef = ScalarSetting | ListField;
type Row = Record<string, Scalar>;

/** One rendered input, and how to show a value the host changed. */
interface Control {
  el: HTMLElement;
  show?: (value: unknown) => void;
}

/** How a field talks to the form. `preview` and `pick` exist only for top-level settings. */
interface FieldIo {
  emit(value: Scalar): void;
  preview?: (value: Scalar) => void;
  pick?: (extensions: readonly string[] | undefined) => void;
}

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

/** What a drag or color pick does: preview if the form can, else save throttled; `settle` is the release. */
function moving<T extends Scalar>(io: FieldIo, limit: MakeLimiter): { move(value: T): void; settle(value: T): void } {
  const { emit, preview } = io;
  if (preview) return { move: preview, settle: emit };
  const send = limit<T>(emit, 100, "throttle");
  return { move: send.push, settle: send.flush };
}

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
    case "slider":
      return readNumber(undefined, field.default, field.min);
    case "string":
    case "font":
      return readString(undefined, field.default);
    case "select":
      return typeof field.default === "string" ? field.default : optionValue(field.options[0] ?? "");
    case "color":
      return decodeColor(field.default, field.alpha === true) ? readString(undefined, field.default) : "#ffffff";
  }
}

/** A slider with a readout; `number` settings with both bounds use it too. */
function rangeControl(
  title: string,
  range: { min: number; max: number; step?: number | undefined; unit?: string | undefined },
  initial: number,
  io: FieldIo,
  limit: MakeLimiter,
): Control {
  const { min, max, step, unit } = range;
  const out = moving<number>(io, limit);
  const input = h("input");
  input.type = "range";
  input.min = String(min);
  input.max = String(max);
  input.step = String(step ?? "any");
  input.value = String(initial);
  input.setAttribute("aria-label", title);
  const readout = h("output", "readout");
  const paint = () => {
    input.style.setProperty("--p", `${((input.valueAsNumber - min) / (max - min || 1)) * 100}%`);
    readout.value = formatReadout(input.valueAsNumber, unit);
    input.setAttribute("aria-valuetext", readout.value);
  };
  paint();
  input.addEventListener("input", () => {
    paint();
    out.move(input.valueAsNumber);
  });
  input.addEventListener("change", () => out.settle(input.valueAsNumber));
  return { el: h("div", "field", h("div", "row", h("span", "t", title), readout), input) };
}

/** A swatch and a hex field; with `alpha`, an opacity slider too. */
function colorControl(title: string, def: Extract<FieldDef, { type: "color" }>, value: unknown, io: FieldIo, limit: MakeLimiter): Control {
  const withAlpha = def.alpha === true;
  const start = decodeColor(value, withAlpha) ?? decodeColor(def.default, withAlpha) ?? { rgb: "#ffffff", alpha: 1 };
  let rgb = start.rgb;
  let alpha = start.alpha;
  const out = moving<string>(io, limit);
  const encoded = () => encodeColor({ rgb, alpha }, withAlpha);

  const swatch = h("input", "swatch");
  swatch.type = "color";
  swatch.value = rgb;
  swatch.setAttribute("aria-label", title);
  const hex = h("input", "hex");
  hex.type = "text";
  hex.value = rgb;
  hex.maxLength = 7;
  hex.spellcheck = false;
  hex.setAttribute("aria-label", `${title} (hex)`);

  swatch.addEventListener("input", () => {
    rgb = swatch.value;
    hex.value = rgb;
    out.move(encoded());
  });
  swatch.addEventListener("change", () => out.settle(encoded()));
  hex.addEventListener("input", () => {
    if (!isHexColor(hex.value)) return;
    rgb = hex.value.toLowerCase();
    swatch.value = rgb;
    out.move(encoded());
  });
  hex.addEventListener("change", () => {
    if (isHexColor(hex.value)) out.settle(encoded());
    else hex.value = rgb; // not a color: snap back
  });

  const picker = h("div", "row gap", swatch, hex);
  if (!withAlpha) return { el: h("div", "field", h("span", "t", title), picker) };

  const opacity = h("input");
  opacity.type = "range";
  opacity.min = "0";
  opacity.max = "100";
  opacity.step = "1";
  opacity.value = String(Math.round(alpha * 100));
  opacity.setAttribute("aria-label", `${title} (opacity)`);
  const readout = h("output", "readout");
  const paint = () => {
    opacity.style.setProperty("--p", `${opacity.valueAsNumber}%`);
    readout.value = `${opacity.valueAsNumber}%`;
    opacity.setAttribute("aria-valuetext", readout.value);
  };
  paint();
  opacity.addEventListener("input", () => {
    alpha = opacity.valueAsNumber / 100;
    paint();
    out.move(encoded());
  });
  opacity.addEventListener("change", () => out.settle(encoded()));
  return { el: h("div", "field", h("span", "t", title), h("div", "stack", picker, h("div", "row gap alpha", opacity, readout))) };
}

/** A `select`: text segments, an icon bar, or a dropdown, by `selectStyle`. */
function selectControl(title: string, def: Extract<FieldDef, { type: "select" }>, value: unknown, emit: (value: string) => void): Control {
  const options = def.options;
  const values = options.map(optionValue);
  let current = typeof value === "string" && values.includes(value) ? value : fieldDefault(def);
  const style = selectStyle(options);

  if (style === "dropdown") {
    const select = h("select");
    select.setAttribute("aria-label", title);
    for (const option of options) select.append(new Option(optionText(option), optionValue(option), false, optionValue(option) === current));
    select.addEventListener("change", () => emit(select.value));
    return { el: h("div", "field", h("span", "t", title), select) };
  }

  const group = h("div", style === "icons" ? `seg icons${options.length >= 4 ? " dense" : ""}` : "seg");
  group.setAttribute("role", "radiogroup");
  group.setAttribute("aria-label", title);
  const buttons = options.map((option) => {
    const label = optionLabel(option);
    const button = h("button", "");
    button.type = "button";
    button.setAttribute("role", "radio");
    if (style === "icons") {
      button.append(h("span", "ico", optionIcon(option) ?? ""), h("span", "lbl", label));
      button.title = label; // the label is hidden when the bar is dense
      button.setAttribute("aria-label", label);
    } else {
      button.textContent = label;
    }
    button.addEventListener("click", () => {
      current = optionValue(option);
      mark();
      emit(current);
    });
    return { value: optionValue(option), button };
  });
  const mark = () => {
    for (const { value: v, button } of buttons) button.setAttribute("aria-checked", String(v === current));
  };
  mark();
  group.append(...buttons.map((b) => b.button));
  return { el: h("div", "field", h("span", "t", title), group) };
}

/** A text box for a font-family name, with the common fonts as chips, each in its own face. */
function fontControl(title: string, value: string, emit: (value: string) => void, limit: MakeLimiter): Control {
  const send = limit<string>(emit, 300, "debounce");
  let valid = value;
  const input = h("input");
  input.type = "text";
  input.value = value;
  input.spellcheck = false;
  input.setAttribute("aria-label", title);
  input.addEventListener("input", () => {
    const ok = isFontName(input.value);
    input.setAttribute("aria-invalid", String(!ok));
    if (!ok) return;
    valid = input.value;
    send.push(valid);
  });
  input.addEventListener("change", () => {
    if (isFontName(input.value)) send.flush();
    else {
      input.value = valid; // not a font name: snap back
      input.setAttribute("aria-invalid", "false");
    }
  });
  const chips = COMMON_FONTS.map((font) => {
    const chip = h("button", "chip", font);
    chip.type = "button";
    chip.style.fontFamily = fontFamilyCss(font);
    chip.addEventListener("click", () => {
      input.value = valid = font;
      input.setAttribute("aria-invalid", "false");
      send.push(font);
      send.flush();
    });
    return chip;
  });
  return { el: h("div", "field", h("span", "t", title), h("div", "stack", input, h("div", "chips", ...chips))) };
}

/** A `file`/`image` row: the file's name (and a thumbnail), Choose and Clear. */
function fileControl(title: string, def: Extract<FieldDef, { type: "file" | "image" }>, value: string, io: FieldIo): Control {
  let url = value;
  const name = h("span", "file-name");
  const choose = h("button", "btn", "Choose…");
  choose.type = "button";
  choose.disabled = io.pick === undefined;
  choose.setAttribute("aria-label", `Choose ${title}`);
  choose.addEventListener("click", () => io.pick?.(def.extensions));
  const clear = h("button", "btn", "Clear");
  clear.type = "button";
  clear.setAttribute("aria-label", `Clear ${title}`);
  clear.addEventListener("click", () => {
    show("");
    io.emit("");
  });

  const thumb = def.type === "image" ? h("img", "thumb") : undefined;
  if (thumb) {
    thumb.alt = "";
    thumb.addEventListener("error", () => (thumb.hidden = true));
  }

  function show(next: unknown) {
    url = readString(next, "");
    name.textContent = url === "" ? "None" : fileName(url);
    name.title = url;
    name.classList.toggle("none", url === "");
    clear.disabled = url === "";
    if (thumb) {
      thumb.hidden = url === "";
      if (url !== "") thumb.src = url;
    }
  }
  show(url);

  const row = h("div", "file", ...(thumb ? [thumb] : []), name, choose, clear);
  return { el: h("div", "field", h("span", "t", title), row), show };
}

/**
 * A `place` row: "Automatic" or "Choose a town", and for a town a search box with suggestions
 * as you type. Saves `null` or `{ name, lat, lon }`.
 */
function placeControl(spec: PlaceSetting, value: unknown, emit: (value: Place | null) => void, search: SettingsFormHooks["onSearch"]): HTMLElement {
  let place = readPlace(value);
  let choosing = place !== null;
  const auto = h("button", "", "Automatic");
  const town = h("button", "", "Choose a town");
  const seg = h("div", "seg", auto, town);
  seg.setAttribute("role", "radiogroup");
  seg.setAttribute("aria-label", spec.label);
  for (const b of [auto, town]) {
    b.type = "button";
    b.setAttribute("role", "radio");
  }
  const chosen = h("div", "place-chosen");
  const input = h("input");
  input.type = "text";
  input.placeholder = "Search for a town";
  input.spellcheck = false;
  input.setAttribute("aria-label", `${spec.label}: search`);
  const list = h("div", "place-list");
  list.setAttribute("role", "listbox");
  const note = h("div", "place-note");
  const finder = h("div", "place-find", input, list, note);
  const canSearch = spec.search !== undefined && search !== undefined;

  function paint() {
    auto.setAttribute("aria-checked", String(!choosing));
    town.setAttribute("aria-checked", String(choosing));
    chosen.hidden = !choosing || place === null;
    chosen.textContent = place ? place.name : "";
    finder.hidden = !choosing;
  }

  let ask = 0;
  const lookup = rateLimited<string>(
    async (query) => {
      const mine = ++ask;
      list.replaceChildren();
      if (!spec.search || !search) return void (note.textContent = "Searching is not available here");
      if (query.trim().length < 2) return void (note.textContent = "");
      note.textContent = "Searching…";
      try {
        const found = readCandidates(await search(spec.search.plugin, spec.search.method, query.trim()));
        if (mine !== ask) return;
        note.textContent = found.length === 0 ? "No towns found" : "";
        list.replaceChildren(
          ...found.slice(0, 8).map((c) => {
            const item = h("button", "place-item", placeLine(c));
            item.type = "button";
            item.setAttribute("role", "option");
            item.addEventListener("click", () => {
              place = placeOf(c);
              input.value = "";
              list.replaceChildren();
              paint();
              emit(place);
            });
            return item;
          }),
        );
      } catch {
        if (mine === ask) note.textContent = "Could not search right now";
      }
    },
    300,
    "debounce",
  );
  input.addEventListener("input", () => lookup.push(input.value));
  if (!canSearch) note.textContent = "Searching is not available here";

  auto.addEventListener("click", () => {
    choosing = false;
    place = null;
    paint();
    emit(null);
  });
  town.addEventListener("click", () => {
    choosing = true;
    paint();
    input.focus();
  });
  paint();
  return h("div", "field", h("span", "t", spec.label || spec.key), seg, chosen, finder);
}

/** One labelled input for a scalar setting (or a list field). */
function renderField(def: FieldDef, value: unknown, io: FieldIo, limit: MakeLimiter): Control {
  const title = def.label || def.key;
  const { emit } = io;

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
      return { el: h("div", "field inline", h("span", "t", title), toggle) };
    }

    case "number": {
      const initial = readNumber(value, def.default, def.min);
      if (def.min !== undefined && def.max !== undefined) {
        // Saved throttled while dragging (no live preview; that is what `slider` is for).
        return rangeControl(title, { min: def.min, max: def.max, step: def.step, unit: undefined }, initial, { emit }, limit);
      }
      const send = limit<number>(emit, 100, "throttle");
      const input = h("input");
      input.setAttribute("aria-label", title);
      input.step = String(def.step ?? "any");
      if (def.min !== undefined) input.min = String(def.min);
      if (def.max !== undefined) input.max = String(def.max);
      input.type = "number";
      input.value = String(initial);
      input.addEventListener("input", () => {
        const n = input.valueAsNumber;
        const inRange = (def.min === undefined || n >= def.min) && (def.max === undefined || n <= def.max);
        if (Number.isFinite(n) && inRange) send.push(n);
      });
      input.addEventListener("change", send.flush);
      return { el: h("div", "field", h("span", "t", title), input) };
    }

    case "slider":
      return rangeControl(title, def, readNumber(value, def.default, def.min), io, limit);

    case "string": {
      const send = limit<string>(emit, 300, "debounce");
      const input = h("input");
      input.type = "text";
      input.value = readString(value, def.default);
      input.setAttribute("aria-label", title);
      input.spellcheck = false;
      input.addEventListener("input", () => send.push(input.value));
      input.addEventListener("change", send.flush);
      return { el: h("div", "field", h("span", "t", title), input) };
    }

    case "select":
      return selectControl(title, def, value, emit);

    case "color":
      return colorControl(title, def, value, io, limit);

    case "font":
      return fontControl(title, readString(value, def.default), emit, limit);

    case "file":
    case "image":
      return fileControl(title, def, readString(value, def.default), io);
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
    case "slider":
      return typeof value === "number" ? value : undefined;
    case "select":
      return typeof value === "string" && field.options.some((o) => optionValue(o) === value) ? value : undefined;
    case "color":
      return typeof value === "string" && decodeColor(value, field.alpha === true) ? value : undefined;
    case "font":
      return typeof value === "string" && isFontName(value) ? value : undefined;
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
        const fields = def.fields.map(
          (field) =>
            renderField(
              field,
              row[field.key],
              {
                emit: (value) => {
                  row[field.key] = value; // the row object, not its index: it may have moved since
                  commit();
                },
              },
              limit,
            ).el,
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

let groupIds = 0;

export function buildSettingsForm(
  specs: readonly SettingDef[],
  values: Record<string, unknown>,
  hooks: SettingsFormHooks,
): SettingsForm {
  const el = h("div", "form");
  const limiters: Limiter<never>[] = [];
  /** Keys previewed but not saved yet, with the value shown. */
  const unsaved = new Map<string, Scalar>();
  const shows = new Map<string, (value: unknown) => void>();

  const limit: MakeLimiter = (fn, ms, mode) => {
    const limiter = rateLimited(fn, ms, mode);
    limiters.push(limiter);
    return limiter;
  };

  function control(spec: SettingDef): HTMLElement {
    if (spec.type === "list") return renderList(spec, values[spec.key], hooks.onChange, limit);
    if (spec.type === "place") return placeControl(spec, values[spec.key], (value) => hooks.onChange(spec.key, value), hooks.onSearch);
    const control = renderField(
      spec,
      values[spec.key],
      {
        emit: (value) => {
          unsaved.delete(spec.key);
          hooks.onChange(spec.key, value);
        },
        preview: (value) => {
          unsaved.set(spec.key, value);
          hooks.onPreview(spec.key, value);
        },
        pick: (extensions) => hooks.onPickFile(spec.key, extensions),
      },
      limit,
    );
    if (control.show) shows.set(spec.key, control.show);
    return control.el;
  }

  for (const section of sections(specs)) {
    const nodes = section.settings.map(control);
    if (section.group === undefined) {
      el.append(...nodes);
      continue;
    }
    const heading = h("div", "group-title", section.group);
    heading.id = `group-${++groupIds}`;
    heading.setAttribute("role", "heading");
    heading.setAttribute("aria-level", "3");
    const group = h("div", "group", heading, ...nodes);
    group.setAttribute("role", "group");
    group.setAttribute("aria-labelledby", heading.id);
    el.append(group);
  }

  return {
    el,
    flush() {
      limiters.forEach((l) => l.flush());
      for (const [key, value] of [...unsaved]) {
        unsaved.delete(key);
        hooks.onChange(key, value);
      }
    },
    update(next) {
      for (const [key, show] of shows) show(next[key]);
    },
  };
}
