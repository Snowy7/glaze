// Small building blocks the flyouts share: sliders, toggle tiles, list rows, headers and app
// icons. Everything is a plain element with clean class names (`fly-*`) for themes to restyle.

import { button, glyph, glyphLetter, h } from "../dom";
import { fire } from "../host";
import { glyphs } from "./glyphs";

/** An app's icon: its image, or its first letter on a tile. */
export function appIcon(icon: string | undefined, name: string, className = "fly-icon"): HTMLElement {
  if (icon) {
    const img = h("img", className);
    img.src = icon;
    img.alt = "";
    img.draggable = false;
    return img;
  }
  return h("span", `${className} fly-monogram`, glyphLetter(name));
}

export interface SliderOptions {
  /** 0–1. */
  value: number;
  label: string;
  /** Called while dragging, at most once per frame, and once more on release. */
  onInput(value: number): void;
  icon?: HTMLElement;
}

/** A labelled range slider (0–1) whose fill follows `--value`. */
export function slider({ value, label, onInput, icon }: SliderOptions): { el: HTMLElement; set(value: number): void } {
  const input = h("input", "fly-slider");
  input.type = "range";
  input.min = "0";
  input.max = "100";
  input.step = "1";
  input.setAttribute("aria-label", label);
  const paint = () => input.style.setProperty("--value", `${input.value}%`);
  let frame = 0;
  let dragging = false;
  const send = () => {
    frame = 0;
    onInput(Number(input.value) / 100);
  };
  input.addEventListener("input", () => {
    dragging = true;
    paint();
    if (!frame) frame = requestAnimationFrame(send);
  });
  input.addEventListener("change", () => {
    dragging = false;
    cancelAnimationFrame(frame);
    send();
  });
  const set = (v: number) => {
    if (dragging) return; // the user's hand wins over the host's echo
    input.value = String(Math.round(Math.max(0, Math.min(1, v)) * 100));
    paint();
  };
  set(value);
  const el = h("div", "fly-slider-row", ...(icon ? [icon] : []), input);
  return { el, set };
}

export interface TileOptions {
  icon: string;
  label: string;
  detail?: string;
  on: boolean;
  onClick(): void;
  /** A chevron that opens more (the network list, the mixer). */
  onMore?(): void;
  disabled?: boolean;
}

/** A quick-settings toggle: a glyph, a label and a state line; `data-on` drives the look. */
export function tile(options: TileOptions): { el: HTMLElement; update(next: Partial<TileOptions>): void } {
  const main = button("fly-tile-main");
  const g = glyph(options.icon, "fly-tile-glyph");
  const label = h("span", "fly-tile-label", options.label);
  const detail = h("span", "fly-tile-detail", options.detail ?? "");
  main.append(g, h("span", "fly-tile-text", label, detail));
  const el = h("div", "fly-tile", main);
  let current = options;
  main.addEventListener("click", () => current.onClick());
  if (options.onMore) {
    const more = button("fly-tile-more", glyph(glyphs.chevronRight));
    more.setAttribute("aria-label", `${options.label} options`);
    more.addEventListener("click", () => current.onMore?.());
    el.append(more);
  }
  const update = (next: Partial<TileOptions>) => {
    current = { ...current, ...next };
    el.dataset.on = String(current.on);
    el.toggleAttribute("data-disabled", current.disabled === true);
    main.disabled = current.disabled === true;
    main.setAttribute("aria-pressed", String(current.on));
    label.textContent = current.label;
    detail.textContent = current.detail ?? "";
    g.innerHTML = current.icon;
  };
  update({});
  return { el, update };
}


/** A flyout's title row, with optional actions on the right. */
export function header(title: string, ...actions: HTMLElement[]): HTMLElement {
  return h("div", "fly-header", h("span", "fly-title", title), h("span", "fly-actions", ...actions));
}

/** A small icon button (refresh, settings, clear). */
export function iconButton(markup: string, label: string, onClick: () => void): HTMLButtonElement {
  const el = button("fly-icon-button", glyph(markup));
  el.title = label;
  el.setAttribute("aria-label", label);
  el.addEventListener("click", onClick);
  return el;
}

/** A text button opening a Settings page. */
export function settingsLink(label: string, uri: string): HTMLButtonElement {
  const el = button("fly-link", label);
  el.addEventListener("click", () => fire("openSettings", { uri }));
  return el;
}

/** A row: leading visual, a title and a detail line, trailing content. */
export function row(lead: HTMLElement | null, title: string, detail = "", ...trail: HTMLElement[]): HTMLButtonElement {
  const el = button("fly-row");
  el.append(
    ...(lead ? [lead] : []),
    h("span", "fly-row-text", h("span", "fly-row-title", title), ...(detail ? [h("span", "fly-row-detail", detail)] : [])),
    ...trail,
  );
  return el;
}

/** A centered message for an empty or unavailable list, with an optional action. */
export function notice(text: string, action?: HTMLElement): HTMLElement {
  return h("div", "fly-notice", h("span", "fly-notice-text", text), ...(action ? [action] : []));
}
