import { RotateCcw } from "lucide-react";
import type { ReactNode } from "react";

import { BUILT_IN, FONT_PRESETS, resolve, setCount, without } from "../lib/appearance";
import type { Appearance, SkinKind, Surface, TextTone } from "../lib/api";
import { AppearancePreview, type BarLayout } from "./AppearancePreview";
import { StylePicker, styleBlurb } from "./StylePicker";
import { Button, ColorField, Combobox, IconButton, Segmented, Slider, Switch, units } from "./ui";

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
  /** Leave out the sticky preview (the page shows its own, e.g. beside the fields). */
  hidePreview?: boolean;
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

const FONT_OPTIONS = FONT_PRESETS.map((preset) => ({ value: preset.value, label: preset.label }));

/**
 * Edits one layer of widget or bar appearance (docs/widgets.md, "Appearance").
 * Every field is either set (marked, with a reset) or "default": unset fields
 * show, dimmed, the value they get from the layers below, and touching one sets
 * it. Fields sit beside their labels when there is room and under them when not.
 * It has no card of its own: put it on one.
 */
export function AppearanceEditor({ target, value, onChange, below = [], fallback, layout, hidePreview = false }: Props) {
  const inherited = resolve(BUILT_IN[target], ...below);
  const shown = resolve(inherited, value);
  const set = <K extends keyof Appearance>(key: K, next: Appearance[K]) => onChange({ ...value, [key]: next });
  const count = setCount(value);

  /** A field's row: its label, whether it's set, and the control. */
  const field = <K extends keyof Appearance>(key: K, label: string, hint: string, control: ReactNode, stacked = false) => (
    <Field label={label} hint={hint} stacked={stacked} isSet={value[key] !== undefined} onUseDefault={() => onChange(without(value, key))}>
      {control}
    </Field>
  );

  return (
    <div className="@container">
      {!hidePreview && (
        <div className="sticky top-0 z-10 -mx-1 bg-surface px-1 pt-1 pb-4">
          <AppearancePreview target={target} base={BUILT_IN[target]} layers={[...below, value]} layout={layout} />
        </div>
      )}

      <div className="mb-1 flex items-center justify-between gap-3">
        <p className="text-[12.5px] text-ink-3" aria-live="polite">
          {count === 0 ? `Everything follows ${fallback}.` : `${count} ${count === 1 ? "field" : "fields"} set; the rest follow ${fallback}.`}
        </p>
        <Button variant="ghost" size="sm" icon={RotateCcw} disabled={count === 0} onClick={() => onChange({})}>
          Use all defaults
        </Button>
      </div>

      <div className="divide-y divide-line">
        {target === "widgets" && field("style", "Style", styleBlurb(shown.style), <StylePicker look={shown} value={shown.style} onChange={(v) => set("style", v)} />, true)}
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
          <Slider label="Opacity" value={shown.opacity} min={0} max={1} step={0.01} {...units.percent} onChange={(v) => set("opacity", v)} />,
        )}
        {field(
          "blur",
          "Blur",
          shown.surface === "glass" ? "Background blur behind the surface." : "Only glass surfaces blur.",
          <Slider label="Blur" value={shown.blur} min={0} max={64} {...units.px} onChange={(v) => set("blur", v)} />,
        )}
        {field(
          "radius",
          "Corner radius",
          target === "bars" ? "Defaults: square top bar, rounded dock." : "How round the corners are.",
          <Slider label="Corner radius" value={shown.radius} min={0} max={48} {...units.px} onChange={(v) => set("radius", v)} />,
        )}
        {field(
          "scale",
          "Scale",
          "Size of the content.",
          <Slider label="Scale" value={shown.scale} min={0.5} max={2} step={0.05} {...units.times} onChange={(v) => set("scale", v)} />,
        )}
        {field("border", "Border", "A hairline around the surface.", <SwitchLine label="Border" checked={shown.border} onChange={(v) => set("border", v)} />)}
        {field(
          "shadow",
          "Text shadow",
          "Keeps text readable without a surface.",
          <SwitchLine label="Text shadow" checked={shown.shadow} onChange={(v) => set("shadow", v)} />,
        )}
        {field(
          "text",
          "Text",
          "Light text for dark wallpapers, dark for light ones.",
          <Segmented<TextTone> label="Text" hideLabel value={shown.text} options={TONES} onChange={(v) => set("text", v)} />,
        )}
        {field("accent", "Accent", "Highlights. Auto takes it from the wallpaper.", <AccentControl value={shown.accent} onChange={(v) => set("accent", v)} />)}
        {field(
          "font",
          "Font",
          "A preset, or any CSS font-family list.",
          <Combobox label="Font" value={shown.font} options={FONT_OPTIONS} placeholder="Segoe UI Variable, sans-serif" onChange={(v) => set("font", v)} className="w-full max-w-80" />,
        )}
      </div>
    </div>
  );
}

/**
 * One field: label and hint, the control, and a reset while it's set. A bar on the left
 * marks a set field. Two lines in a narrow editor (or when `stacked`), one row in a wide one.
 */
function Field({ label, hint, isSet, onUseDefault, stacked, children }: { label: string; hint: string; isSet: boolean; onUseDefault: () => void; stacked: boolean; children: ReactNode }) {
  const wide = stacked ? "" : "@[36rem]:grid-cols-[11.5rem_minmax(0,1fr)_1.875rem] @[36rem]:[grid-template-areas:'label_control_reset']";
  return (
    <div className={`relative grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2.5 py-3.5 [grid-template-areas:'label_reset'_'control_control'] ${wide}`}>
      <span
        aria-hidden
        className={`absolute top-1/2 -left-2.5 h-6 w-[3px] -translate-y-1/2 rounded-full bg-ink transition-[opacity,transform] duration-200 ${isSet ? "scale-y-100 opacity-100" : "scale-y-0 opacity-0"}`}
      />
      <div className="min-w-0 [grid-area:label]">
        <div className="flex items-center gap-2 text-[13px] font-medium text-ink">
          {label}
          {!isSet && <span className="text-[11px] font-normal text-ink-4">Default</span>}
        </div>
        <div className="mt-0.5 text-[12px] leading-snug text-ink-3">{hint}</div>
      </div>
      <div className={`min-w-0 [grid-area:control] ${isSet ? "" : "opacity-60 transition-opacity focus-within:opacity-100 hover:opacity-100"}`}>{children}</div>
      <div className="flex justify-end [grid-area:reset]">
        {isSet && <IconButton icon={RotateCcw} size="sm" variant="ghost" label={`${label}: use default`} onClick={onUseDefault} />}
      </div>
    </div>
  );
}

function SwitchLine({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <div className="flex items-center gap-2.5">
      <Switch label={label} checked={checked} onChange={onChange} />
      <span className="text-[12.5px] text-ink-2">{checked ? "On" : "Off"}</span>
    </div>
  );
}

function AccentControl({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const isAuto = value === "auto";
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Segmented<"auto" | "color">
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
