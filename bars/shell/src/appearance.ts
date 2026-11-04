// Applies the host's resolved appearance to <html> (docs/widgets.md, "Appearance"): data-*
// attributes, CSS variables, and the theme CSS as the last <style>.

import type { Appearance } from "./protocol";

function rgb(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  const digits = m?.[1] ?? "101010";
  const full = digits.length === 3 ? [...digits].map((c) => c + c).join("") : digits;
  const n = Number.parseInt(full, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** The `<html>` attributes and CSS variables for an appearance. Pure so it can be tested. */
export function appearanceTokens(a: Appearance): { attrs: Record<string, string>; vars: Record<string, string> } {
  const [r, g, b] = rgb(a.tint);
  const light = a.text === "light";
  const ink = light ? "255 255 255" : "0 0 0";
  return {
    attrs: {
      "data-surface": a.surface,
      "data-text": a.text,
      "data-shadow": a.shadow ? "on" : "off",
    },
    vars: {
      "--surface": a.surface === "none" ? "transparent" : `rgb(${r} ${g} ${b} / ${a.opacity})`,
      // Popovers and menus: the same tint, more opaque.
      "--raised": `rgb(${r} ${g} ${b} / ${Math.max(0.92, a.opacity)})`,
      "--blur": `${a.blur}px`,
      "--radius": `${a.radius}px`,
      "--line": a.border ? (light ? "rgb(255 255 255 / 0.10)" : "rgb(0 0 0 / 0.12)") : "transparent",
      "--edge": light ? "rgb(255 255 255 / 0.12)" : "rgb(0 0 0 / 0.14)",
      "--t0": light ? "#fff" : "#0a0a0a",
      "--t1": `rgb(${ink} / ${light ? 0.74 : 0.72})`,
      "--t2": `rgb(${ink} / ${light ? 0.46 : 0.5})`,
      "--t3": `rgb(${ink} / ${light ? 0.24 : 0.28})`,
      "--fill-1": `rgb(${ink} / ${light ? 0.06 : 0.05})`,
      "--fill-2": `rgb(${ink} / ${light ? 0.11 : 0.09})`,
      "--fill-3": `rgb(${ink} / ${light ? 0.2 : 0.16})`,
      "--accent": a.accent,
      "--font": a.font,
      "--scale": String(a.scale),
    },
  };
}

export function applyAppearance(a: Appearance, themeCss: string | null): void {
  const root = document.documentElement;
  const { attrs, vars } = appearanceTokens(a);
  for (const [name, value] of Object.entries(attrs)) root.setAttribute(name, value);
  for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value);

  document.getElementById("theme-css")?.remove();
  if (themeCss) {
    const style = document.createElement("style");
    style.id = "theme-css";
    style.textContent = themeCss;
    document.head.append(style);
  }
}
