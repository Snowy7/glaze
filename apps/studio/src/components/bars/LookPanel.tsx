import { FolderOpen } from "lucide-react";
import { useState } from "react";

import { pickImage } from "../../lib/api";
import { useBars } from "../../lib/barsContext";
import { CORNERS, STATES, type BarKind, type Corner, type Family, type Fill, type FillType, type GlassStyle, type GlyphSet, type ImageFit, type Look, type Scheme, type State } from "../../lib/barsSchema";
import { useActiveSkins } from "../../lib/hooks";
import { LookFields } from "../inspectors/LookFields";
import { Block, Group, IconButton, INPUT_BASE, Prop, Segmented, Select } from "../ui";
import { ChoiceSetting, FieldColor, FieldPaint, FieldSlider, SelectSetting, ToggleSetting, percent, px } from "./BarSettings";
import { GradientEditor } from "./PaintEditor";
import { ThemeVarsPanel } from "./ThemeVarsPanel";

const SCHEMES: readonly { value: Scheme; label: string; hint?: string }[] = [
  { value: "auto", label: "Follow Windows", hint: "Light or dark with Windows, live" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const CORNER_OPTIONS: readonly { value: Corner; label: string }[] = CORNERS.map((value) => ({ value, label: value[0]?.toUpperCase() + value.slice(1) }));

const FILL_LABELS: Record<FillType, string> = { none: "None", solid: "Solid", gradient: "Gradient", image: "Image", wallpaper: "Wallpaper glass", blur: "Live blur" };
const FILL_TYPES: readonly FillType[] = ["none", "solid", "gradient", "image", "wallpaper", "blur"];

const GLASS_OPTIONS: readonly { value: GlassStyle; label: string; hint: string }[] = [
  { value: "liquid", label: "Liquid", hint: "What's behind, bent by a lens along the rim and clear in the middle, with a specular edge" },
  { value: "frosted", label: "Frosted", hint: "What's behind, blurred" },
  { value: "none", label: "None", hint: "The background alone" },
];
const FAMILY_OPTIONS: readonly { value: Family; label: string }[] = [
  { value: "mono", label: "mypaper" },
  { value: "fluent", label: "Windows 11" },
  { value: "macos", label: "macOS" },
  { value: "material", label: "ChromeOS" },
];
const GLYPH_OPTIONS: readonly { value: GlyphSet; label: string }[] = [
  { value: "default", label: "mypaper" },
  { value: "fluent", label: "Segoe Fluent Icons" },
  { value: "symbols", label: "SF Symbols style" },
];

/** A fresh background of each type, with the values the host would pick. */
const FILL_START: Record<FillType, Fill> = {
  none: { type: "none" },
  solid: { type: "solid", color: "#141416eb" },
  gradient: { type: "gradient", angle: 180, stops: ["#101820", "#2a3a55"] },
  image: { type: "image", path: "", fit: "cover" },
  wallpaper: { type: "wallpaper" },
  blur: { type: "blur" },
};

/** The material a `look.glass` names (a string is the style alone). */
const glassStyle = (glass: Look["glass"]): GlassStyle => (typeof glass === "string" ? glass : (glass?.style ?? "frosted"));

/** A bar's look at a glance: light or dark, and the glass it's made of. */
export function LookCommon() {
  const { resolved } = useBars();
  return (
    <>
      <ChoiceSetting<Scheme> path={["look", "scheme"]} title="Light or dark" options={SCHEMES} fallback="auto" />
      <ChoiceSetting<GlassStyle> path={["look", "glass", "style"]} title="Glass" detail={GLASS_OPTIONS.find((o) => o.value === glassStyle(resolved.look.glass))?.hint} options={GLASS_OPTIONS} fallback="frosted" />
    </>
  );
}

/**
 * Everything else about one bar's look (the editor is scoped to that bar, see `forBar`): its
 * background and material, the glass's strength, menus and icons, edges, buttons and indicators.
 * What a bar can't show isn't offered.
 */
export function LookPanel({ bar }: { bar: BarKind }) {
  const { resolved } = useBars();
  const square = bar === "top" && resolved.top.layout === "strip";
  const glass = glassStyle(resolved.look.glass);
  return (
    <div className="flex flex-col gap-6">
      <Group title="Surface">
        <Block title="Background">
          <BackgroundEditor />
        </Block>
        <FieldSlider path={["look", "tint"]} label="Tint" min={0} max={1} step={0.01} fallback={0.6} format={percent} />
        <FieldSlider path={["look", "saturation"]} label="Saturation" min={0} max={3} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
        <FieldSlider path={["look", "noise"]} label="Grain" min={0} max={0.5} step={0.005} fallback={0} format={percent} />
        {!square && (
          <>
            <FieldSlider path={["look", "radius"]} label="Corner radius" min={0} max={64} fallback={16} format={px} />
            <SelectSetting<Corner> path={["look", "corners"]} title="Corner shape" options={CORNER_OPTIONS} fallback="round" />
          </>
        )}
      </Group>

      {glass !== "none" && (
      <Group title="Glass">
          <div className="flex flex-col gap-3.5">
            <FieldSlider path={["look", "glass", "intensity"]} label="Intensity" min={0} max={1} step={0.01} fallback={1} format={percent} />
            <FieldSlider path={["look", "glass", "refraction"]} label="Refraction" min={0} max={1} step={0.01} fallback={0.6} format={percent} />
            <FieldSlider path={["look", "glass", "highlight"]} label="Highlight" min={0} max={1} step={0.01} fallback={0.75} format={percent} />
            <FieldSlider path={["look", "glass", "tint"]} label="Glass tint" min={0} max={1} step={0.01} fallback={0.12} format={percent} />
            <FieldSlider path={["look", "glass", "blur"]} label="Blur" min={0} max={32} step={0.5} fallback={1} format={px} />
            <FieldSlider path={["look", "glass", "depth"]} label="Depth" min={0} max={1} step={0.01} fallback={0.2} format={percent} />
            <FieldSlider path={["look", "glass", "sheen"]} label="Sheen" min={0} max={1} step={0.01} fallback={0} format={percent} />
          </div>
      </Group>
      )}

      <Group title="Menus and panels">
        <ToggleSetting path={["look", "flyouts", "blur"]} title="Glass menus" fallback />
        <SelectSetting<Family> path={["look", "family"]} title="Drawn like" options={FAMILY_OPTIONS} fallback="mono" />
        <SelectSetting<GlyphSet> path={["look", "glyphs"]} title="Icons" detail="An icon pack, if you pick one, wins." options={GLYPH_OPTIONS} fallback="default" />
      </Group>

      <Group title="Edges and depth">
        <Block title="Border, glow and shadow" detail="Hover and pressed start from rest.">
          <StateEdges />
        </Block>
      </Group>

      <Group title="Buttons">
        <Block title="Module and app buttons" detail="At rest, under the pointer, and pressed or open.">
          <ItemStates />
        </Block>
      </Group>

      {bar === "dock" && (
        <Group title="Running indicators">
          <FieldColor path={["look", "indicator", "running"]} label="Running" />
          <FieldColor path={["look", "indicator", "active"]} label="In front" />
          <FieldColor path={["look", "indicator", "attention"]} label="Asking for attention" />
        </Group>
      )}

    </div>
  );
}

/** What both bars build on (shared with the theme): the base appearance, and the theme's own knobs. */
export function SharedLook() {
  const skins = useActiveSkins();
  const { doc, set } = useBars();
  return (
    <>
      <LookFields target="bars" prefix="bars.appearance" value={doc.appearance ?? {}} below={[skins.bars]} belowLabel="Theme" onChange={(appearance) => set(["appearance"], Object.keys(appearance).length === 0 ? undefined : appearance, "Change the bars' appearance")} />
      <ThemeVarsPanel />
    </>
  );
}

function BackgroundEditor() {
  const { resolved, catalog, set } = useBars();
  const fill = resolved.look.background;
  const allowed = catalog.backgroundTypes[resolved.preset];
  const path = ["look", "background"] as const;
  const type: FillType | "preset" = fill?.type ?? "preset";

  return (
    <div className="flex flex-col gap-3.5">
      <Select<FillType | "preset">
        label="Background"
        size="sm"
        className="w-full"
        value={type}
        onChange={(next) => {
          if (next === "preset") set(path, undefined);
          // An image needs a file from the start: the host refuses an empty path.
          else if (next === "image") void pickImage().then((file) => file && set(path, { ...FILL_START.image, path: file }));
          else set(path, FILL_START[next]);
        }}
        options={[{ value: "preset", label: "The design's" }, ...FILL_TYPES.map((t) => ({ value: t, label: FILL_LABELS[t], disabled: !allowed.includes(t) && t !== fill?.type }))]}
      />

      {fill?.type === "solid" && <FieldColor path={[...path, "color"]} label="Color" />}
      {fill?.type === "gradient" && (
        <GradientEditor label="Background" value={fill} onChange={(gradient) => set(path, { type: "gradient", ...gradient })} />
      )}
      {fill?.type === "image" && <ImageFields />}
      {fill?.type === "wallpaper" && (
        <>
          <FieldSlider path={[...path, "brightness"]} label="Brightness" min={0} max={2} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
          <FieldSlider path={[...path, "blur"]} label="Extra blur" min={0} max={64} fallback={0} format={px} />
          <FieldSlider path={[...path, "interval"]} label="Catch up every" min={2} max={600} fallback={10} format={(v) => `${v} s`} />
        </>
      )}
    </div>
  );
}

function ImageFields() {
  const { resolved, set } = useBars();
  const fill = resolved.look.background;
  const image = fill?.type === "image" ? fill : null;
  const path = ["look", "background"] as const;
  return (
    <>
      <Prop label="Image" stacked>
        <div className="flex gap-2">
          <input aria-label="Background image" readOnly value={image?.path.split(/[\\/]/).pop() ?? ""} title={image?.path} className={`${INPUT_BASE} ui-input-sm min-w-0 flex-1 text-[12px]`} />
          <IconButton icon={FolderOpen} size="sm" label="Choose an image" onClick={() => void pickImage().then((file) => file && set([...path, "path"], file))} />
        </div>
      </Prop>
      <Prop label="Fit">
        <Select<ImageFit> label="Image fit" size="sm" className="w-44" value={image?.fit ?? "cover"} options={IMAGE_FITS} onChange={(fit) => set([...path, "fit"], fit)} />
      </Prop>
      <Prop label="Position">
        <input aria-label="Image position" value={image?.position ?? "center"} spellCheck={false} onChange={(e) => set([...path, "position"], e.currentTarget.value)} className={`${INPUT_BASE} ui-input-sm w-44 font-mono text-[12px]`} />
      </Prop>
      <FieldSlider path={[...path, "brightness"]} label="Brightness" min={0} max={2} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
      <FieldSlider path={[...path, "blur"]} label="Blur" min={0} max={64} fallback={0} format={px} />
    </>
  );
}

const IMAGE_FITS: readonly { value: ImageFit; label: string }[] = [
  { value: "cover", label: "Fill" },
  { value: "contain", label: "Fit" },
  { value: "fill", label: "Stretch" },
  { value: "tile", label: "Tile" },
];

const STATE_LABELS: Record<State, string> = { rest: "Rest", hover: "Hover", active: "Pressed" };
const STATE_OPTIONS = STATES.map((value) => ({ value, label: STATE_LABELS[value] }));

function StateEdges() {
  const [state, setState] = useState<State>("rest");
  return (
    <div className="flex flex-col gap-3.5">
      <Segmented<State> className="w-full" size="sm" label="State" hideLabel value={state} options={STATE_OPTIONS} onChange={setState} />
      <div className="flex flex-col gap-3.5">
        <h4 className="text-[12px] font-medium text-ink-3">Border</h4>
        <FieldSlider path={["look", "border", state, "width"]} label="Width" min={0} max={8} step={0.5} fallback={state === "rest" ? 1 : 0} format={px} />
        <FieldPaint path={["look", "border", state, "color"]} label="Color" placeholder="#ffffff1f" />
        <h4 className="pt-1 text-[12px] font-medium text-ink-3">Glow</h4>
        <FieldColor path={["look", "glow", state, "color"]} label="Color" placeholder="accent" />
        <FieldSlider path={["look", "glow", state, "size"]} label="Size" min={0} max={64} fallback={0} format={px} />
        <FieldSlider path={["look", "glow", state, "opacity"]} label="Opacity" min={0} max={1} step={0.01} fallback={0.5} format={percent} />
        <h4 className="pt-1 text-[12px] font-medium text-ink-3">Shadow</h4>
        <FieldColor path={["look", "shadow", state, "color"]} label="Color" placeholder="#00000052" />
        <FieldSlider path={["look", "shadow", state, "x"]} label="Offset x" min={-64} max={64} fallback={0} format={px} />
        <FieldSlider path={["look", "shadow", state, "y"]} label="Offset y" min={-64} max={64} fallback={0} format={px} />
        <FieldSlider path={["look", "shadow", state, "blur"]} label="Blur" min={0} max={96} fallback={0} format={px} />
        <FieldSlider path={["look", "shadow", state, "spread"]} label="Spread" min={-32} max={32} fallback={0} format={px} />
      </div>
    </div>
  );
}

function ItemStates() {
  const [state, setState] = useState<State>("hover");
  return (
    <div className="flex flex-col gap-3.5">
      <FieldSlider path={["look", "items", "radius"]} label="Corner radius" min={0} max={32} fallback={8} format={px} />
      <Segmented<State> className="w-full" size="sm" label="Button state" hideLabel value={state} options={STATE_OPTIONS} onChange={setState} />
      <div className="flex flex-col gap-3.5">
        <FieldPaint path={["look", "items", state, "background"]} label="Background" placeholder="#ffffff1a" />
        <FieldColor path={["look", "items", state, "color"]} label="Text color" />
        <FieldSlider path={["look", "items", state, "scale"]} label="Scale" min={0.5} max={1.5} step={0.01} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
        <FieldSlider path={["look", "items", state, "border", "width"]} label="Border width" min={0} max={8} step={0.5} fallback={0} format={px} />
        <FieldPaint path={["look", "items", state, "border", "color"]} label="Border color" placeholder="#ffffff33" />
        <FieldColor path={["look", "items", state, "glow", "color"]} label="Glow color" placeholder="accent" />
        <FieldSlider path={["look", "items", state, "glow", "size"]} label="Glow size" min={0} max={64} fallback={0} format={px} />
      </div>
    </div>
  );
}
