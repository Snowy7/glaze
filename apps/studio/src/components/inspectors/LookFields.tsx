import { appearanceData, appearanceVars, BUILT_IN, FONT_PRESETS, resolve, setsField, without, type Resolved } from "../../lib/appearance";
import { WIDGET_STYLES, type Appearance, type Glass, type SkinKind, type Surface, type TextTone, type WidgetStyle } from "../../lib/api";
import { useWallpaperBackground } from "../../lib/hooks";
import { ColorField, Combobox, InspectorSection, More, Prop, Segmented, Slider, Switch, TilePicker, units, type Tile } from "../ui";

// One layer of how widgets (or the bars) look (docs/widgets.md, "Appearance"), as inspector
// properties: the style and surface as tiles, then the few settings that apply to that surface,
// then the rest under More options. A value nobody in this layer set shows where it comes from.

const STYLE_INFO: Record<WidgetStyle, { label: string; hint: string }> = {
  minimal: { label: "Minimal", hint: "Flat and quiet" },
  depth: { label: "Depth", hint: "Raised cards under a soft light" },
  neon: { label: "Neon", hint: "A glowing gradient edge" },
  paper: { label: "Paper", hint: "Soft matte with a grain" },
  aero: { label: "Aero", hint: "Light glass with a bright rim" },
  liquid: { label: "Liquid", hint: "Clear glass that bends the wallpaper at its rim" },
};

/**
 * What unset glass values come to. The widget kit doesn't publish its defaults yet; these are the
 * values of the theme crate's reference plate (crates/theme/src/glass.rs).
 */
const GLASS_DEFAULTS: Required<Glass> = { intensity: 0.8, refraction: 0.6, highlight: 0.7 };

const GLASS_FIELDS: readonly { key: keyof Glass; label: string; hint: string }[] = [
  { key: "intensity", label: "Strength", hint: "The whole effect" },
  { key: "refraction", label: "Bend", hint: "How much the rim bends what's behind it" },
  { key: "highlight", label: "Shine", hint: "The bright edge" },
];

const SURFACES: readonly { value: Surface; label: string; hint: string }[] = [
  { value: "none", label: "None", hint: "Content sits right on the wallpaper" },
  { value: "glass", label: "Glass", hint: "Tinted and blurred" },
  { value: "solid", label: "Solid", hint: "A flat color" },
];

/** Sliders sit beside their label, at one width so their tracks line up. */
const SLIDER = "w-[10.5rem] 2xl:w-[13rem]";

const TONES: readonly { value: TextTone; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

type Props = {
  target: SkinKind;
  /** This layer: only the fields it sets. */
  value: Appearance;
  /** Layers under this one, lowest first; an unset field shows their result. */
  below?: readonly Appearance[];
  /** What the layers below are called, for an unset field ("Theme"). */
  belowLabel?: string;
  /** `label` names the undo step. */
  onChange: (next: Appearance, label: string) => void;
  /** Search anchors are `<prefix>.<field>`. */
  prefix: string;
  /** Read-only (a built-in theme). */
  disabled?: boolean;
  /** The layer's liquid glass values, where the layer has them (a theme's widget look). */
  glass?: { value: Glass | null; onChange: (next: Glass | null, label: string) => void };
};

export function LookFields({ target, value, below = [], belowLabel = "Theme", onChange, prefix, disabled = false, glass }: Props) {
  const shown = resolve(BUILT_IN[target], ...below, value);
  const set = <K extends keyof Appearance>(key: K, next: Appearance[K], label: string) => !disabled && onChange({ ...value, [key]: next }, label);
  const prop = (key: keyof Appearance) => ({
    id: `${prefix}.${key}`,
    inherited: value[key] === undefined ? (setsField(below, key) ? belowLabel : "Default") : undefined,
    onReset: disabled ? undefined : () => onChange(without(value, key), `Reset ${key}`),
  });

  return (
    <>
      <InspectorSection title={target === "widgets" ? "Style" : "Surface"}>
        {target === "widgets" && (
          <Prop {...prop("style")} label="Style" stacked>
            <TilePicker<WidgetStyle>
              label="Widget style"
              value={shown.style}
              options={WIDGET_STYLES.map((style) => ({ value: style, label: STYLE_INFO[style].label, hint: STYLE_INFO[style].hint, preview: <Sample look={{ ...shown, style, surface: shown.surface === "none" ? "glass" : shown.surface }} /> }))}
              onChange={(style) => set("style", style, "Change style")}
              columns={3}
            />
          </Prop>
        )}
        {shown.style === "liquid" && glass && <GlassFields prefix={prefix} glass={glass} disabled={disabled} />}
        <Prop {...prop("surface")} label="Surface" stacked>
          <TilePicker<Surface>
            label="Surface"
            value={shown.surface}
            options={SURFACES.map((surface): Tile<Surface> => ({ value: surface.value, label: surface.label, hint: surface.hint, preview: <Sample look={{ ...shown, surface: surface.value }} /> }))}
            onChange={(surface) => set("surface", surface, "Change surface")}
            columns={3}
            aspect="aspect-[5/3]"
          />
        </Prop>
        {shown.surface !== "none" && (
          <Prop {...prop("tint")} label="Color">
            <ColorField label="Surface color" value={shown.tint} onChange={(tint) => set("tint", tint, "Change color")} />
          </Prop>
        )}
        {shown.surface === "glass" && (
          <>
            <Prop {...prop("opacity")} label="Opacity">
              <Slider className={SLIDER} label="Opacity" value={shown.opacity} min={0} max={1} step={0.01} {...units.percent} onChange={(v) => set("opacity", v, "Change opacity")} />
            </Prop>
            <Prop {...prop("blur")} label="Blur">
              <Slider className={SLIDER} label="Blur" value={shown.blur} min={0} max={64} {...units.px} onChange={(v) => set("blur", v, "Change blur")} />
            </Prop>
          </>
        )}
        <Prop {...prop("radius")} label="Corners">
          <Slider className={SLIDER} label="Corner radius" value={shown.radius} min={0} max={48} {...units.px} onChange={(v) => set("radius", v, "Change corners")} />
        </Prop>
      </InspectorSection>
      <More>
        <Prop {...prop("text")} label="Text">
          <Segmented<TextTone> label="Text" hideLabel size="sm" value={shown.text} options={TONES} onChange={(v) => set("text", v, "Change text color")} />
        </Prop>
        <Prop {...prop("accent")} label="Accent">
          <AccentControl value={shown.accent} onChange={(v) => set("accent", v, "Change accent")} />
        </Prop>
        <Prop {...prop("font")} label="Font" stacked>
          <Combobox label="Font" className="w-full" value={shown.font} options={FONT_PRESETS.map((p) => ({ value: p.value, label: p.label }))} placeholder="Segoe UI Variable" onChange={(v) => set("font", v, "Change font")} />
        </Prop>
        <Prop {...prop("scale")} label="Size">
          <Slider className={SLIDER} label="Size" value={shown.scale} min={0.5} max={2} step={0.05} {...units.times} onChange={(v) => set("scale", v, "Change size")} />
        </Prop>
        {shown.surface !== "none" && (
          <Prop {...prop("border")} label="Edge line">
            <Switch label="Edge line" checked={shown.border} onChange={(v) => set("border", v, "Change edge line")} />
          </Prop>
        )}
        <Prop {...prop("shadow")} label="Text shadow">
          <Switch label="Text shadow" checked={shown.shadow} onChange={(v) => set("shadow", v, "Change text shadow")} />
        </Prop>
      </More>
    </>
  );
}

function AccentControl({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const auto = value === "auto";
  return (
    <div className="flex min-w-0 flex-col items-end gap-2">
      <Segmented<"auto" | "color">
        label="Accent"
        hideLabel
        size="sm"
        value={auto ? "auto" : "color"}
        options={[
          { value: "auto", label: "From wallpaper" },
          { value: "color", label: "Pick" },
        ]}
        onChange={(mode) => onChange(mode === "auto" ? "auto" : "#ffffff")}
      />
      {!auto && <ColorField label="Accent color" value={value} onChange={onChange} />}
    </div>
  );
}

/** Liquid glass: strength, bend and shine, each a share of the full effect. */
function GlassFields({ prefix, glass, disabled }: { prefix: string; glass: NonNullable<Props["glass"]>; disabled: boolean }) {
  const own = glass.value ?? {};
  const set = (key: keyof Glass, next: number | undefined, label: string) => {
    if (disabled) return;
    const { [key]: _old, ...rest } = own;
    const values = next === undefined ? rest : { ...rest, [key]: next };
    glass.onChange(Object.keys(values).length === 0 ? null : values, label);
  };
  return (
    <>
      {GLASS_FIELDS.map(({ key, label, hint }) => (
        <Prop key={key} id={`${prefix}.glass.${key}`} label={label} inherited={own[key] === undefined ? "Default" : undefined} onReset={() => set(key, undefined, `Reset ${label.toLowerCase()}`)}>
          <Slider className={SLIDER} label={`${label}: ${hint}`} value={own[key] ?? GLASS_DEFAULTS[key]} min={0} max={1} step={0.05} {...units.percent} onChange={(v) => set(key, v, `Change ${label.toLowerCase()}`)} />
        </Prop>
      ))}
    </>
  );
}

/** A tiny clock card painted with a look, on your wallpaper: what a tile shows. */
export function Sample({ look }: { look: Resolved }) {
  const wallpaper = useWallpaperBackground();
  return (
    <span className="ap-stage absolute inset-0" data-wallpaper="dark" style={wallpaper ? { background: wallpaper } : undefined}>
      <span className="ap-root absolute inset-0 grid place-items-center" {...appearanceData(look)} style={appearanceVars(look)}>
        <span className="ap-surface block w-[78%] px-1.5 py-1.5">
          <span className="block text-[12px] leading-none font-light tabular-nums">10:42</span>
          <span className="mt-1 block h-[3px] overflow-hidden rounded-full" style={{ background: "var(--fill-2)" }}>
            <span className="block h-full w-3/5 rounded-full" style={{ background: "var(--accent)" }} />
          </span>
        </span>
      </span>
    </span>
  );
}
