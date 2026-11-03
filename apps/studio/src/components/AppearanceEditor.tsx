import { RotateCcw } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { BUILT_IN, FONT_PRESETS, resolve, setCount, without } from "../lib/appearance";
import type { Appearance, SkinKind, Surface, TextTone } from "../lib/api";
import { AppearancePreview, type BarLayout } from "./AppearancePreview";
import { Button, ColorField, INPUT_CLASS, Segmented, Slider, Switch } from "./ui";

type Props = {
  target: SkinKind;
  /** This layer: only the fields the user has set. */
  value: Appearance;
  onChange: (next: Appearance) => void;
  /** The layers under this one, lowest first (e.g. the active theme's look). Unset fields show their result. */
  below?: readonly Appearance[];
  /** What "Use default" falls back to, in words: "the theme", "the built-in look". */
  fallback: string;
  /** The top bar's modules, drawn in the bars preview. */
  layout?: BarLayout;
};

const SURFACES: readonly { value: Surface; label: string; hint: string }[] = [
  { value: "none", label: "None", hint: "No background: text sits on the wallpaper" },
  { value: "glass", label: "Glass", hint: "Tinted and blurred" },
  { value: "solid", label: "Solid", hint: "A flat tinted background" },
];

const TONES: readonly { value: TextTone; label: string; hint: string }[] = [
  { value: "light", label: "Light", hint: "For dark wallpapers" },
  { value: "dark", label: "Dark", hint: "For light wallpapers" },
];

const percent = (value: number) => `${Math.round(value * 100)}%`;
const px = (value: number) => `${value}px`;

/**
 * Edits one layer of widget or bar appearance (docs/widgets.md, "Appearance").
 * Every field is either set or "default": unset fields show the value they get
 * from the layers below, dimmed, and touching one sets it. Put it on a `bg-surface`
 * card: the live preview sticks to the top while the fields scroll.
 */
export function AppearanceEditor({ target, value, onChange, below = [], fallback, layout }: Props) {
  const inherited = resolve(BUILT_IN[target], ...below);
  const shown = resolve(inherited, value);
  const set = <K extends keyof Appearance>(key: K, next: Appearance[K]) => onChange({ ...value, [key]: next });
  const count = setCount(value);

  /** A field's row: its label, whether it's set, and the control. */
  const field = <K extends keyof Appearance>(key: K, label: string, hint: string, control: ReactNode, note?: string) => (
    <Field label={label} hint={note ?? hint} isSet={value[key] !== undefined} onUseDefault={() => onChange(without(value, key))}>
      {control}
    </Field>
  );

  return (
    <div className="bg-surface">
      <div className="sticky top-0 z-10 -mx-1 bg-surface px-1 pt-1 pb-4">
        <AppearancePreview target={target} base={BUILT_IN[target]} layers={[...below, value]} layout={layout} />
      </div>

      <div className="mb-1 flex items-center justify-between gap-3">
        <p className="text-xs text-ink-3" aria-live="polite">
          {count === 0 ? `Everything follows ${fallback}.` : `${count} ${count === 1 ? "field" : "fields"} set; the rest follow ${fallback}.`}
        </p>
        <Button variant="ghost" icon={RotateCcw} disabled={count === 0} onClick={() => onChange({})}>
          Use all defaults
        </Button>
      </div>

      <div className="divide-y divide-line">
        {field(
          "surface",
          "Surface",
          "Background behind the content.",
          <Segmented<Surface> label="Surface" hideLabel value={shown.surface} options={SURFACES} onChange={(v) => set("surface", v)} />,
        )}
        {field("tint", "Tint", "The surface color.", <ColorField label="Tint color" value={shown.tint} onChange={(v) => set("tint", v)} />)}
        {field(
          "opacity",
          "Opacity",
          "How solid the surface is.",
          <Slider label="Opacity" value={shown.opacity} min={0} max={1} step={0.01} format={percent} onChange={(v) => set("opacity", v)} />,
        )}
        {field(
          "blur",
          "Blur",
          "Background blur behind the surface.",
          <Slider label="Blur" value={shown.blur} min={0} max={64} format={px} onChange={(v) => set("blur", v)} />,
          shown.surface === "glass" ? undefined : "Only glass surfaces blur.",
        )}
        {field(
          "radius",
          "Corner radius",
          target === "bars" ? "Defaults: square top bar, rounded dock." : "How round the corners are.",
          <Slider label="Corner radius" value={shown.radius} min={0} max={48} format={px} onChange={(v) => set("radius", v)} />,
        )}
        {field(
          "scale",
          "Scale",
          "Size of the content.",
          <Slider label="Scale" value={shown.scale} min={0.5} max={2} step={0.05} format={(v) => `${v.toFixed(2)}x`} onChange={(v) => set("scale", v)} />,
        )}
        {field(
          "border",
          "Border",
          "A hairline around the surface.",
          <SwitchLine label="Border" checked={shown.border} onChange={(v) => set("border", v)} />,
        )}
        {field(
          "shadow",
          "Text shadow",
          "A soft shadow that keeps text readable without a surface.",
          <SwitchLine label="Text shadow" checked={shown.shadow} onChange={(v) => set("shadow", v)} />,
        )}
        {field(
          "text",
          "Text",
          "Light text for dark wallpapers, dark text for light ones.",
          <Segmented<TextTone> label="Text" hideLabel value={shown.text} options={TONES} onChange={(v) => set("text", v)} />,
        )}
        {field("accent", "Accent", "Highlights. Auto takes it from the wallpaper.", <AccentControl value={shown.accent} onChange={(v) => set("accent", v)} />)}
        {field("font", "Font", "A CSS font-family list.", <FontControl value={shown.font} onChange={(v) => set("font", v)} />)}
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  isSet,
  onUseDefault,
  children,
}: {
  label: string;
  hint: string;
  isSet: boolean;
  onUseDefault: () => void;
  children: ReactNode;
}) {
  return (
    <div className="py-3.5">
      <div className="mb-2 flex items-baseline justify-between gap-3">
        <div className="min-w-0">
          <span className="text-[13.5px] font-medium text-ink">{label}</span>
          <span className="ml-2 text-xs text-ink-3">{hint}</span>
        </div>
        {isSet ? (
          <button
            type="button"
            onClick={onUseDefault}
            className="shrink-0 cursor-default rounded-md px-1.5 py-0.5 text-xs font-medium text-ink-2 ring-1 ring-line-strong transition hover:bg-surface-hi hover:text-ink hover:ring-line-hi"
          >
            Use default
          </button>
        ) : (
          <span className="shrink-0 px-1.5 text-xs text-ink-3">Default</span>
        )}
      </div>
      <div className={isSet ? "" : "opacity-55 transition-opacity focus-within:opacity-100 hover:opacity-100"}>{children}</div>
    </div>
  );
}

function SwitchLine({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <div className="flex items-center gap-3">
      <Switch label={label} checked={checked} onChange={onChange} />
      <span className="text-[13px] text-ink-2">{checked ? "On" : "Off"}</span>
    </div>
  );
}

function AccentControl({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const isAuto = value === "auto";
  return (
    <div className="flex items-center gap-3">
      <Segmented<"auto" | "color">
        className="min-w-0 flex-1"
        label="Accent"
        hideLabel
        value={isAuto ? "auto" : "color"}
        options={[
          { value: "auto", label: "Auto", hint: "From the wallpaper's palette" },
          { value: "color", label: "Color" },
        ]}
        onChange={(mode) => onChange(mode === "auto" ? "auto" : "#ffffff")}
      />
      {!isAuto && <ColorField label="Accent color" value={value} onChange={onChange} />}
    </div>
  );
}

const CUSTOM = "custom";

/** Presets, or a typed font-family list. "Custom" stays selected while you type, even if it matches a preset. */
function FontControl({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const inputId = useId();
  const [custom, setCustom] = useState(!FONT_PRESETS.some((p) => p.value === value));
  return (
    <div className="space-y-2">
      <Segmented<string>
        label="Font"
        hideLabel
        value={custom ? CUSTOM : value}
        options={[...FONT_PRESETS.map((p) => ({ value: p.value, label: p.label })), { value: CUSTOM, label: "Custom" }]}
        onChange={(next) => {
          setCustom(next === CUSTOM);
          if (next !== CUSTOM) onChange(next);
        }}
      />
      {custom && (
        <>
          <label htmlFor={inputId} className="sr-only">
            Font family list
          </label>
          <input
            id={inputId}
            value={value}
            onChange={(e) => onChange(e.currentTarget.value)}
            spellCheck={false}
            placeholder="Segoe UI Variable, sans-serif"
            className={INPUT_CLASS}
          />
        </>
      )}
    </div>
  );
}
