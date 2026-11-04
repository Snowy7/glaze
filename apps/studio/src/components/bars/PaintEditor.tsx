import { Plus, X } from "lucide-react";
import { useEffect, useId, useState } from "react";

import { gradientCss, joinHex, splitHex, type ColorContext } from "../../lib/barLook";
import { tidy } from "../../lib/barsDoc";
import { COLOR_TOKENS, type Color, type Gradient, type Paint, type Stop } from "../../lib/barsSchema";
import { Button, IconButton, INPUT_BASE, Segmented, Select, Slider } from "../ui";

// Colors, gradients and paints as `bars.json` writes them (docs/bars.md, "Look"). The host
// takes `#rgb`…`#rrggbbaa`, a token such as `accent`, or `var(--name)` for a theme variable.

const VALID_COLOR = /^(#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})|var\(--[\w-]+\))$/i;
const isColor = (text: string) => VALID_COLOR.test(text) || (COLOR_TOKENS as readonly string[]).includes(text);

/** The stand-in colors previews use for tokens and variables, so a swatch can show them. */
const SWATCH: ColorContext = { text: "#ffffff", accent: "#c8c8c8", vars: {} };

type ColorValueProps = {
  label: string;
  value: Color | undefined;
  onChange: (value: Color) => void;
  /** Offered when the color may be left unset. */
  onClear?: () => void;
  /** The color shown while `value` is unset. */
  placeholder?: Color;
  /** Only `#rrggbb` or `#rrggbbaa`: what a theme variable of type color takes. */
  hexOnly?: boolean;
};

const isHexColor = (text: string) => /^#([0-9a-f]{6}|[0-9a-f]{8})$/i.test(text);

/** A swatch, a text box and an opacity slider for one color; tokens come from a menu. */
export function ColorValue({ label, value, onChange, onClear, placeholder = "#ffffff", hexOnly = false }: ColorValueProps) {
  const id = useId();
  const shown = value ?? placeholder;
  const hex = splitHex(shown);
  // Typing is held here until it is a color the host takes.
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);

  return (
    <div className="flex flex-wrap items-center gap-2">
      <label htmlFor={id} className={`flex h-10 shrink-0 cursor-default items-center rounded-lg bg-canvas p-1 ring-1 ring-line-strong hover:ring-line-hi has-focus-visible:outline-2 has-focus-visible:outline-ink ${value === undefined ? "opacity-60" : ""}`}>
        <span className="relative size-8 overflow-hidden rounded-md" style={{ background: "repeating-conic-gradient(#888 0 25%, #bbb 0 50%) 0 0 / 8px 8px" }}>
          <span className="absolute inset-0" style={{ background: hex ? shown : (tokenCss(shown) ?? "#888") }} />
          <input id={id} type="color" aria-label={`${label} color`} value={hex?.rgb ?? "#808080"} onChange={(e) => onChange(joinHex(e.currentTarget.value, hex?.alpha ?? 1))} className="absolute inset-0 size-full cursor-default opacity-0" />
        </span>
      </label>
      <input
        aria-label={`${label} value`}
        value={draft}
        spellCheck={false}
        onChange={(e) => {
          const text = e.currentTarget.value.trim();
          setDraft(e.currentTarget.value);
          if (hexOnly ? isHexColor(text) : isColor(text)) onChange(text.startsWith("#") ? text.toLowerCase() : text);
        }}
        className={`${INPUT_BASE} w-36 font-mono text-[13px] ${value === undefined ? "opacity-60" : ""}`}
      />
      {!hexOnly && (
        <Select<string>
          label={`${label} token`}
          className="w-36"
          value={(COLOR_TOKENS as readonly string[]).includes(shown) ? shown : ""}
          options={[{ value: "", label: "Token…" }, ...COLOR_TOKENS.map((token) => ({ value: token, label: token }))]}
          onChange={(token) => token !== "" && onChange(token)}
        />
      )}
      {hex && (
        <Slider className="w-40" label={`${label} opacity`} value={Math.round(hex.alpha * 100)} min={0} max={100} format={(v) => `${v}%`} onChange={(v) => onChange(joinHex(hex.rgb, v / 100))} />
      )}
      {onClear && value !== undefined && <IconButton icon={X} label={`Unset ${label}`} onClick={onClear} />}
    </div>
  );
}

function tokenCss(token: string): string | undefined {
  if (token === "transparent") return "transparent";
  return token.startsWith("system-accent") ? "#4f8cff" : token === "accent" ? SWATCH.accent : token === "text" ? SWATCH.text : undefined;
}

const stopColor = (stop: Stop): Color => (typeof stop === "string" ? stop : stop.color);
const stopAt = (stop: Stop, index: number, count: number): number => (typeof stop === "string" ? index / Math.max(count - 1, 1) : stop.at);

/** A gradient: kind, angle, and 2 to 8 stops, each with a color and an optional position. */
export function GradientEditor({ label, value, onChange }: { label: string; value: Gradient; onChange: (value: Gradient) => void }) {
  const kind = value.kind ?? "linear";
  const setStop = (index: number, next: Stop) => onChange({ ...value, stops: value.stops.map((s, i) => (i === index ? next : s)) });
  return (
    <div className="space-y-3">
      <div className="h-8 rounded-lg ring-1 ring-line-strong" style={{ background: gradientCss({ ...value, kind: "linear", angle: 90 }, SWATCH) }} aria-hidden />
      <div className="flex flex-wrap items-center gap-3">
        <Segmented<"linear" | "radial" | "conic">
          className="w-64"
          label={`${label} kind`}
          hideLabel
          value={kind}
          options={[{ value: "linear", label: "Linear" }, { value: "radial", label: "Radial" }, { value: "conic", label: "Conic" }]}
          onChange={(next) => onChange({ ...value, kind: next === "linear" ? undefined : next })}
        />
        {kind !== "radial" && <Slider className="w-56" label={`${label} angle`} value={value.angle ?? 180} min={0} max={360} format={(v) => `${v}°`} onChange={(v) => onChange({ ...value, angle: v })} />}
      </div>
      <ul className="space-y-2">
        {value.stops.map((stop, index) => (
          <li key={index} className="flex flex-wrap items-center gap-2 rounded-xl bg-canvas p-2 ring-1 ring-line">
            <ColorValue label={`${label} stop ${index + 1}`} value={stopColor(stop)} onChange={(color) => setStop(index, typeof stop === "string" ? color : { color, at: stop.at })} />
            <Slider
              className="w-40"
              label={`${label} stop ${index + 1} position`}
              value={Math.round(stopAt(stop, index, value.stops.length) * 100)}
              min={0}
              max={100}
              format={(v) => `${v}%`}
              onChange={(v) => setStop(index, { color: stopColor(stop), at: tidy(v / 100) })}
            />
            <IconButton icon={X} label={`Remove stop ${index + 1}`} disabled={value.stops.length <= 2} onClick={() => onChange({ ...value, stops: value.stops.filter((_, i) => i !== index) })} />
          </li>
        ))}
      </ul>
      <Button icon={Plus} disabled={value.stops.length >= 8} onClick={() => onChange({ ...value, stops: [...value.stops, "#ffffff"] })}>
        Add a stop
      </Button>
    </div>
  );
}

export const DEFAULT_GRADIENT: Gradient = { angle: 135, stops: ["#ffffff33", "#ffffff0d"] };

/** A paint: a color, or a gradient. */
export function PaintValue({ label, value, onChange, onClear, placeholder }: { label: string; value: Paint | undefined; onChange: (value: Paint) => void; onClear?: () => void; placeholder?: Color }) {
  const mode = typeof value === "object" ? "gradient" : "color";
  return (
    <div className="space-y-2">
      <Segmented<"color" | "gradient">
        className="w-56"
        label={`${label} type`}
        hideLabel
        value={mode}
        options={[{ value: "color", label: "Color" }, { value: "gradient", label: "Gradient" }]}
        onChange={(next) => onChange(next === "gradient" ? DEFAULT_GRADIENT : (placeholder ?? "#ffffff"))}
      />
      {typeof value === "object" ? (
        <GradientEditor label={label} value={value} onChange={onChange} />
      ) : (
        <ColorValue label={label} value={value} onChange={onChange} onClear={onClear} placeholder={placeholder} />
      )}
    </div>
  );
}
