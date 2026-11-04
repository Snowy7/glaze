import { appearanceData, appearanceVars, BUILT_IN, FONT_PRESETS, resolve, setsField, without, type Resolved } from "../../lib/appearance";
import { WIDGET_STYLES, type Appearance, type SkinKind, type Surface, type TextTone, type WidgetStyle } from "../../lib/api";
import { ColorField, Combobox, InspectorSection, More, Prop, Segmented, Slider, Switch, TilePicker, units, type Tile } from "../ui";

// One layer of how widgets (or the bars) look (docs/widgets.md, "Appearance"), as inspector
// properties: the style and surface as tiles, then the few settings that apply to that surface,
// then the rest under More options. A value nobody in this layer set shows where it comes from.

const STYLE_INFO: Record<WidgetStyle, { label: string; hint: string }> = {
  minimal: { label: "Minimal", hint: "Flat and quiet" },
  depth: { label: "Depth", hint: "Raised cards under a soft light" },
  neon: { label: "Neon", hint: "A glowing gradient edge" },
  paper: { label: "Paper", hint: "Soft matte with a grain" },
  aero: { label: "Aero", hint: "Glossy glass with a bright rim" },
};

const SURFACES: readonly { value: Surface; label: string; hint: string }[] = [
  { value: "none", label: "None", hint: "Content sits right on the wallpaper" },
  { value: "glass", label: "Glass", hint: "Tinted and blurred" },
  { value: "solid", label: "Solid", hint: "A flat color" },
];

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
};

export function LookFields({ target, value, below = [], belowLabel = "Theme", onChange, prefix, disabled = false }: Props) {
  const shown = resolve(BUILT_IN[target], ...below, value);
  const set = <K extends keyof Appearance>(key: K, next: Appearance[K], label: string) => !disabled && onChange({ ...value, [key]: next }, label);
  const prop = (key: keyof Appearance) => ({
    id: `${prefix}.${key}`,
    inherited: value[key] === undefined ? (setsField(below, key) ? belowLabel : "Default") : undefined,
    onReset: () => !disabled && onChange(without(value, key), `Reset ${key}`),
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
              min="4.6rem"
            />
          </Prop>
        )}
        <Prop {...prop("surface")} label="Surface" stacked>
          <TilePicker<Surface>
            label="Surface"
            value={shown.surface}
            options={SURFACES.map((surface): Tile<Surface> => ({ value: surface.value, label: surface.label, hint: surface.hint, preview: <Sample look={{ ...shown, surface: surface.value }} /> }))}
            onChange={(surface) => set("surface", surface, "Change surface")}
            min="5rem"
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
              <Slider className="w-full" label="Opacity" value={shown.opacity} min={0} max={1} step={0.01} {...units.percent} onChange={(v) => set("opacity", v, "Change opacity")} />
            </Prop>
            <Prop {...prop("blur")} label="Blur">
              <Slider className="w-full" label="Blur" value={shown.blur} min={0} max={64} {...units.px} onChange={(v) => set("blur", v, "Change blur")} />
            </Prop>
          </>
        )}
        <Prop {...prop("radius")} label="Corners">
          <Slider className="w-full" label="Corner radius" value={shown.radius} min={0} max={48} {...units.px} onChange={(v) => set("radius", v, "Change corners")} />
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
          <Slider className="w-full" label="Size" value={shown.scale} min={0.5} max={2} step={0.05} {...units.times} onChange={(v) => set("scale", v, "Change size")} />
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

/** A tiny clock card painted with a look, on a stand-in wallpaper: what a tile shows. */
export function Sample({ look }: { look: Resolved }) {
  return (
    <span className="ap-stage absolute inset-0" data-wallpaper="dark">
      <span className="ap-root absolute inset-0 grid place-items-center" {...appearanceData(look)} style={appearanceVars(look)}>
        <span className="ap-surface block w-[74%] px-2 py-1.5">
          <span className="block text-[14px] leading-none font-light tabular-nums">10:42</span>
          <span className="mt-1.5 block h-1 overflow-hidden rounded-full" style={{ background: "var(--fill-2)" }}>
            <span className="block h-full w-3/5 rounded-full" style={{ background: "var(--accent)" }} />
          </span>
        </span>
      </span>
    </span>
  );
}
