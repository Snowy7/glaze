// A form generated from a widget manifest's `settings`. Each change is reported on its own
// (`onChange(key, value)`); the host answers with a fresh `state`, so the form never re-renders.

import { h } from "./dom";
import type { SettingSpec } from "./protocol";

export interface SettingsForm {
  el: HTMLElement;
  /** Sends any change still waiting on a debounce or throttle (call when closing the form). */
  flush(): void;
}

const HEX = /^#[0-9a-f]{6}$/i;

/** Debounce waits for quiet; throttle sends the first value at once, then at most one per `ms`. */
function rateLimited<T>(fn: (value: T) => void, ms: number, mode: "debounce" | "throttle") {
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
    push(value: T) {
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

const pretty = (n: number) => String(Math.round(n * 1000) / 1000);

export function buildSettingsForm(
  specs: SettingSpec[],
  values: Record<string, unknown>,
  onChange: (key: string, value: unknown) => void,
): SettingsForm {
  const el = h("div", "form");
  const limiters: { flush(): void }[] = [];

  function limited<T>(key: string, ms: number, mode: "debounce" | "throttle") {
    const limiter = rateLimited<T>((value) => onChange(key, value), ms, mode);
    limiters.push(limiter);
    return limiter;
  }

  for (const spec of specs) {
    const saved = values[spec.key];
    const title = spec.label ?? spec.key;

    switch (spec.type) {
      case "boolean": {
        let on = typeof saved === "boolean" ? saved : spec.default;
        const toggle = h("button", "switch");
        toggle.type = "button";
        toggle.setAttribute("role", "switch");
        toggle.setAttribute("aria-label", title);
        toggle.setAttribute("aria-checked", String(on));
        toggle.addEventListener("click", () => {
          on = !on;
          toggle.setAttribute("aria-checked", String(on));
          onChange(spec.key, on);
        });
        el.append(h("div", "field inline", h("span", "t", title), toggle));
        break;
      }

      case "number": {
        const initial = typeof saved === "number" ? saved : spec.default;
        const send = limited<number>(spec.key, 100, "throttle");
        const input = h("input");
        input.setAttribute("aria-label", title);
        input.step = String(spec.step ?? "any");
        if (spec.min !== undefined) input.min = String(spec.min);
        if (spec.max !== undefined) input.max = String(spec.max);

        if (spec.min !== undefined && spec.max !== undefined) {
          const { min, max } = spec;
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
          el.append(h("div", "field", h("div", "row", h("span", "t", title), readout), input));
        } else {
          input.type = "number";
          input.value = String(initial);
          input.addEventListener("input", () => {
            const n = input.valueAsNumber;
            const inRange = (spec.min === undefined || n >= spec.min) && (spec.max === undefined || n <= spec.max);
            if (Number.isFinite(n) && inRange) send.push(n);
          });
          input.addEventListener("change", send.flush);
          el.append(h("div", "field", h("span", "t", title), input));
        }
        break;
      }

      case "string": {
        const send = limited<string>(spec.key, 300, "debounce");
        const input = h("input");
        input.type = "text";
        input.value = typeof saved === "string" ? saved : spec.default;
        input.setAttribute("aria-label", title);
        input.spellcheck = false;
        input.addEventListener("input", () => send.push(input.value));
        input.addEventListener("change", send.flush);
        el.append(h("div", "field", h("span", "t", title), input));
        break;
      }

      case "select": {
        let current = typeof saved === "string" && spec.options.includes(saved) ? saved : spec.default;
        if (spec.options.length <= 3) {
          const group = h("div", "seg");
          group.setAttribute("role", "radiogroup");
          group.setAttribute("aria-label", title);
          const buttons = spec.options.map((option) => {
            const button = h("button", "", option);
            button.type = "button";
            button.setAttribute("role", "radio");
            button.addEventListener("click", () => {
              current = option;
              mark();
              onChange(spec.key, option);
            });
            return { option, button };
          });
          const mark = () => {
            for (const { option, button } of buttons) button.setAttribute("aria-checked", String(option === current));
          };
          mark();
          group.append(...buttons.map((b) => b.button));
          el.append(h("div", "field", h("span", "t", title), group));
        } else {
          const select = h("select");
          select.setAttribute("aria-label", title);
          for (const option of spec.options) select.append(new Option(option, option, false, option === current));
          select.addEventListener("change", () => onChange(spec.key, select.value));
          el.append(h("div", "field", h("span", "t", title), select));
        }
        break;
      }

      case "color": {
        const send = limited<string>(spec.key, 100, "throttle");
        const initial = typeof saved === "string" && HEX.test(saved) ? saved : spec.default;
        const swatch = h("input", "swatch");
        swatch.type = "color";
        swatch.value = initial;
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
        el.append(h("div", "field", h("span", "t", title), h("div", "row gap", swatch, hex)));
        break;
      }
    }
  }

  return { el, flush: () => limiters.forEach((l) => l.flush()) };
}
