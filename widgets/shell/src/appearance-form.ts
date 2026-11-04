// The Appearance panel's form: one control per appearance field, each with "Use default" while the
// edited layer sets it. Sliders and color pickers preview live (`onPreview`, every move) and save
// once on release (`onCommit`); everything else saves as soon as it changes. The form never
// re-renders when the host answers: it keeps its own copy of the layer.

import { FONT_PRESETS, HEX, RANGES, STYLES } from "./appearance";
import { h } from "./dom";
import type { AppearanceValues, Layer, LayerPatch, SurfaceKind, TextTone } from "./protocol";

export interface AppearanceFormOptions {
  /** What an unset field shows: the values of the layers below the edited one (`accent` may be `auto`). */
  below: AppearanceValues;
  /** The layer being edited. */
  layer: Layer;
  /** The color `accent: auto` stands for (shown while accent is on auto). */
  autoAccent: string;
  /** The whole edited layer, on every move of a slider or picker. */
  onPreview(layer: Layer): void;
  /** The change to save: a value, or `null` for "Use default". */
  onCommit(patch: LayerPatch): void;
}

type Key = keyof AppearanceValues;

/** One control: how to show a value, and the element to put in the form. */
interface Control {
  el: HTMLElement;
  show(value: AppearanceValues[Key]): void;
}

const SURFACES: readonly SurfaceKind[] = ["none", "glass", "solid"];
const TONES: readonly TextTone[] = ["light", "dark"];

const pretty = (n: number) => String(Math.round(n * 100) / 100);

export interface AppearanceForm {
  el: HTMLElement;
  /** Saves any previewed change that was never saved (call when closing the panel). */
  flush(): void;
}

export function buildAppearanceForm(options: AppearanceFormOptions): AppearanceForm {
  const { below, autoAccent, onPreview, onCommit } = options;
  const layer: Layer = { ...options.layer };
  const form = h("div", "form");
  const refreshers: (() => void)[] = [];
  /** Keys previewed but not saved yet. */
  const unsaved = new Set<Key>();

  const valueOf = <K extends Key>(key: K): AppearanceValues[K] => (layer[key] ?? below[key]) as AppearanceValues[K];

  /** Stores a new value for `key`; `final` also saves it. */
  function set<K extends Key>(key: K, value: AppearanceValues[K], final: boolean) {
    Object.assign(layer, { [key]: value });
    refresh();
    onPreview({ ...layer });
    if (final) commit(key, value);
    else unsaved.add(key);
  }

  function commit(key: Key, value: AppearanceValues[Key] | null) {
    unsaved.delete(key);
    const patch: LayerPatch = {};
    Object.assign(patch, { [key]: value });
    onCommit(patch);
  }

  function clear(key: Key) {
    delete layer[key];
    refresh();
    onPreview({ ...layer });
    commit(key, null);
  }

  const refresh = () => {
    form.dataset.surface = valueOf("surface"); // opacity and blur only apply to glass: the CSS dims them
    refreshers.forEach((r) => r());
  };

  /** A labelled row for `key`. `inline` puts the control beside the label (toggles). */
  function row(key: Key, label: string, control: Control, inline = false, readout?: HTMLElement) {
    const reset = h("button", "reset", "Use default");
    reset.type = "button";
    reset.addEventListener("click", () => clear(key));
    const tools = h("span", "tools", ...(readout ? [readout] : []), reset);
    const el = inline
      ? h("div", "field inline", h("span", "t", label), tools, control.el)
      : h("div", "field", h("div", "row", h("span", "t", label), tools), control.el);
    el.dataset.key = key;
    refreshers.push(() => {
      reset.hidden = layer[key] === undefined;
      el.classList.toggle("set", layer[key] !== undefined);
      control.show(valueOf(key));
    });
    form.append(el);
  }

  // ---- control builders ----

  function segmented<K extends "style" | "surface" | "text">(key: K, options: readonly AppearanceValues[K][]): Control {
    const group = h("div", "seg");
    group.setAttribute("role", "radiogroup");
    const buttons = options.map((option) => {
      const button = h("button", "", option);
      button.type = "button";
      button.setAttribute("role", "radio");
      button.addEventListener("click", () => set(key, option, true));
      return { option, button };
    });
    group.append(...buttons.map((b) => b.button));
    return {
      el: group,
      show: (value) => {
        for (const { option, button } of buttons) button.setAttribute("aria-checked", String(option === value));
      },
    };
  }

  function toggle(key: "border" | "shadow", label: string): Control {
    const button = h("button", "switch");
    button.type = "button";
    button.setAttribute("role", "switch");
    button.setAttribute("aria-label", label);
    button.addEventListener("click", () => set(key, !valueOf(key), true));
    return { el: button, show: (value) => button.setAttribute("aria-checked", String(value === true)) };
  }

  function slider(key: "opacity" | "blur" | "radius" | "scale", format: (n: number) => string): { control: Control; readout: HTMLElement } {
    const { min, max, step } = RANGES[key];
    const input = h("input");
    input.type = "range";
    input.min = String(min);
    input.max = String(max);
    input.step = String(step);
    const readout = h("output", "readout");
    const paint = () => {
      input.style.setProperty("--p", `${((input.valueAsNumber - min) / (max - min)) * 100}%`);
      readout.value = format(input.valueAsNumber);
    };
    input.addEventListener("input", () => {
      paint();
      set(key, input.valueAsNumber, false);
    });
    input.addEventListener("change", () => commit(key, input.valueAsNumber));
    return {
      control: {
        el: input,
        show: (value) => {
          if (typeof value === "number") input.value = String(value);
          paint();
        },
      },
      readout,
    };
  }

  /** A swatch and a hex field. Previews while the picker moves, saves when it closes. */
  function colorPicker(key: "tint" | "accent", disabledWhenAuto: boolean): Control {
    const swatch = h("input", "swatch");
    swatch.type = "color";
    swatch.setAttribute("aria-label", key);
    const hex = h("input", "hex");
    hex.type = "text";
    hex.maxLength = 7;
    hex.spellcheck = false;
    hex.setAttribute("aria-label", `${key} (hex)`);
    swatch.addEventListener("input", () => {
      hex.value = swatch.value;
      set(key, swatch.value, false);
    });
    swatch.addEventListener("change", () => commit(key, swatch.value));
    hex.addEventListener("input", () => {
      if (!HEX.test(hex.value)) return;
      swatch.value = hex.value.toLowerCase();
      set(key, swatch.value, false);
    });
    hex.addEventListener("change", () => {
      if (HEX.test(hex.value)) commit(key, hex.value.toLowerCase());
      else hex.value = swatch.value; // not a color: snap back
    });
    const el = h("div", "row gap", swatch, hex);
    return {
      el,
      show: (value) => {
        const auto = disabledWhenAuto && value === "auto";
        const color = auto ? autoAccent : typeof value === "string" && HEX.test(value) ? value : "#ffffff";
        swatch.value = color;
        hex.value = color;
        swatch.disabled = auto;
        hex.disabled = auto;
      },
    };
  }

  /** Auto, or a color: a segmented choice above the color picker. */
  function accent(): Control {
    const picker = colorPicker("accent", true);
    const modes = (["auto", "color"] as const).map((mode) => {
      const button = h("button", "", mode === "auto" ? "Auto" : "Color");
      button.type = "button";
      button.setAttribute("role", "radio");
      button.addEventListener("click", () => {
        if (mode === "auto") set("accent", "auto", true);
        else {
          // Pin the color currently shown (the palette's, when it was auto).
          const shown = valueOf("accent");
          set("accent", shown === "auto" ? autoAccent : shown, true);
        }
      });
      return { mode, button };
    });
    const group = h("div", "seg", ...modes.map((m) => m.button));
    group.setAttribute("role", "radiogroup");
    return {
      el: h("div", "stack", group, picker.el),
      show: (value) => {
        for (const { mode, button } of modes) {
          button.setAttribute("aria-checked", String((value === "auto") === (mode === "auto")));
        }
        picker.show(value);
      },
    };
  }

  /** A text field for a CSS font-family list, with presets. Empty means "Use default". */
  function fontField(): Control {
    const input = h("input");
    input.type = "text";
    input.spellcheck = false;
    input.setAttribute("aria-label", "font");
    input.addEventListener("input", () => {
      if (input.value.trim() !== "") set("font", input.value, false);
    });
    input.addEventListener("change", () => {
      const text = input.value.trim();
      if (text === "") clear("font");
      else set("font", text, true);
    });
    const presets = FONT_PRESETS.map((font) => {
      const chip = h("button", "chip", font.split(",")[0] ?? font);
      chip.type = "button";
      chip.title = font;
      chip.addEventListener("click", () => set("font", font, true));
      return chip;
    });
    return {
      el: h("div", "stack", input, h("div", "chips", ...presets)),
      show: (value) => {
        if (typeof value === "string" && document.activeElement !== input) input.value = value;
      },
    };
  }

  // ---- the form, in the order of the spec ----

  row("style", "Style", segmented("style", STYLES));
  row("surface", "Surface", segmented("surface", SURFACES));
  row("tint", "Tint", colorPicker("tint", false));
  const opacity = slider("opacity", pretty);
  row("opacity", "Opacity", opacity.control, false, opacity.readout);
  const blur = slider("blur", (n) => `${pretty(n)} px`);
  row("blur", "Blur", blur.control, false, blur.readout);
  const radius = slider("radius", (n) => `${pretty(n)} px`);
  row("radius", "Radius", radius.control, false, radius.readout);
  row("border", "Border", toggle("border", "Border"), true);
  row("shadow", "Text shadow", toggle("shadow", "Text shadow"), true);
  row("text", "Text", segmented("text", TONES));
  row("accent", "Accent", accent());
  row("font", "Font", fontField());
  const scale = slider("scale", (n) => `${pretty(n)}×`);
  row("scale", "Scale", scale.control, false, scale.readout);

  refresh();
  return {
    el: form,
    flush: () => {
      for (const key of [...unsaved]) commit(key, valueOf(key));
    },
  };
}
