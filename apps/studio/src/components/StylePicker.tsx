import type { KeyboardEvent } from "react";

import { appearanceData, appearanceVars, type Resolved } from "../lib/appearance";
import { WIDGET_STYLES, type WidgetStyle } from "../lib/api";

const STYLE_INFO: Record<WidgetStyle, { label: string; blurb: string; likes: string }> = {
  minimal: { label: "Minimal", blurb: "Flat, quiet, monochrome. What every widget had before.", likes: "any surface" },
  depth: { label: "Depth", blurb: "Raised cards under a soft key light: layered shadows, a bevel.", likes: "glass or solid" },
  neon: { label: "Neon", blurb: "A dark card with a glowing gradient outline and bright tracks.", likes: "solid or glass" },
  paper: { label: "Paper", blurb: "Soft matte with a grain, shallow inset shadows, no gloss.", likes: "solid" },
  aero: { label: "Aero", blurb: "Glossy glass: a sheen across the top, a bright rim.", likes: "glass" },
};

/** The five widget design languages (docs/widgets.md, "Styles"), each drawn on the current look. Arrow keys move the choice. */
export function StylePicker({ look, value, onChange }: { look: Resolved; value: WidgetStyle; onChange: (style: WidgetStyle) => void }) {
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    if (step === undefined) return;
    event.preventDefault();
    const next = WIDGET_STYLES[(WIDGET_STYLES.indexOf(value) + step + WIDGET_STYLES.length) % WIDGET_STYLES.length];
    if (!next) return;
    onChange(next);
    event.currentTarget.querySelector<HTMLElement>(`[data-style="${next}"]`)?.focus();
  }

  return (
    <div role="radiogroup" aria-label="Widget style" onKeyDown={onKeyDown} className="grid grid-cols-[repeat(auto-fill,minmax(5.75rem,1fr))] gap-2">
      {WIDGET_STYLES.map((style) => {
        const on = style === value;
        // A style is how the surface is lit, so each tile draws one even when the look has none.
        const tile: Resolved = { ...look, style, surface: look.surface === "none" ? "glass" : look.surface };
        return (
          <button
            key={style}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={on ? 0 : -1}
            data-style={style}
            title={`${STYLE_INFO[style].blurb} Likes ${STYLE_INFO[style].likes}.`}
            onClick={() => onChange(style)}
            className={`cursor-default rounded-lg p-1 text-left transition-[box-shadow,background-color,color] duration-150 ${
              on ? "bg-surface-top text-ink ring-2 ring-ink" : "bg-canvas text-ink-3 ring-1 ring-line-strong hover:bg-surface-hi hover:text-ink hover:ring-line-hi"
            }`}
          >
            <div className="ap-stage relative h-16 overflow-hidden rounded-md" data-wallpaper="dark" aria-hidden>
              <div className="ap-root absolute inset-0 grid place-items-center" {...appearanceData(tile)} style={appearanceVars(tile)}>
                <div className="ap-surface w-[72%] px-2 py-1.5">
                  <div className="text-[15px] leading-none font-light tabular-nums">10:42</div>
                  <div className="mt-1.5 h-1 overflow-hidden rounded-full" style={{ background: "var(--fill-2)" }}>
                    <div className="h-full w-3/5 rounded-full" style={{ background: "var(--accent)" }} />
                  </div>
                </div>
              </div>
            </div>
            <div className="px-1 pt-1.5 pb-0.5 text-[12px] font-medium">{STYLE_INFO[style].label}</div>
          </button>
        );
      })}
    </div>
  );
}

export function styleBlurb(style: WidgetStyle): string {
  return `${STYLE_INFO[style].blurb} Looks best on ${STYLE_INFO[style].likes}.`;
}
