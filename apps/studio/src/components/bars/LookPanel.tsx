import { FolderOpen } from "lucide-react";
import { useState } from "react";

import { pickImage } from "../../lib/api";
import { useBars } from "../../lib/barsContext";
import { CORNERS, STATES, type Corner, type Family, type Fill, type FillType, type GlassStyle, type GlyphSet, type ImageFit, type Look, type Scheme, type State } from "../../lib/barsSchema";
import { useActiveSkins } from "../../lib/hooks";
import { AppearanceEditor } from "../AppearanceEditor";
import { Block, Group, IconButton, INPUT_BASE, Note, Segmented, Select } from "../ui";
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

export function LookPanel() {
  const skins = useActiveSkins();
  const { doc, resolved, set } = useBars();
  return (
    <div className="flex flex-col gap-6">
      <Group title="Surface">
        <ChoiceSetting<Scheme> path={["look", "scheme"]} title="Scheme" detail="Sets the text colors. Follow Windows switches live with light and dark mode." options={SCHEMES} fallback="auto" wide />
        <Block title="Background" detail="What the bar and the dock's pill are made of.">
          <BackgroundEditor />
        </Block>
        <Block title="Material" detail="How the surface is laid over what's behind it.">
          <div className="space-y-3">
            <FieldSlider path={["look", "tint"]} label="Tint" min={0} max={1} step={0.01} fallback={0.6} format={percent} />
            <FieldSlider path={["look", "saturation"]} label="Saturation" min={0} max={3} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
            <FieldSlider path={["look", "noise"]} label="Grain" min={0} max={0.5} step={0.005} fallback={0} format={percent} />
            <FieldSlider path={["look", "radius"]} label="Corner radius" min={0} max={64} fallback={16} format={px} />
          </div>
          <Note>Tint is laid over an image, gradient or wallpaper background. A strip top bar is always square.</Note>
        </Block>
        <SelectSetting<Corner> path={["look", "corners"]} title="Corner shape" detail="Squircle, bevel, scoop and notch use CSS corner-shape." options={CORNER_OPTIONS} fallback="round" />
      </Group>

      <Group title="Glass">
        <ChoiceSetting<GlassStyle> path={["look", "glass", "style"]} title="Material" detail={GLASS_OPTIONS.find((o) => o.value === glassStyle(resolved.look.glass))?.hint} options={GLASS_OPTIONS} fallback="frosted" wide />
        <Block title="Knobs" detail="The same names as the widgets' liquid style. Flyouts use a stronger lens and more frost of their own.">
          <div className="space-y-3">
            <FieldSlider path={["look", "glass", "intensity"]} label="Intensity" min={0} max={1} step={0.01} fallback={1} format={percent} />
            <FieldSlider path={["look", "glass", "refraction"]} label="Refraction" min={0} max={1} step={0.01} fallback={0.6} format={percent} />
            <FieldSlider path={["look", "glass", "highlight"]} label="Highlight" min={0} max={1} step={0.01} fallback={0.75} format={percent} />
            <FieldSlider path={["look", "glass", "tint"]} label="Glass tint" min={0} max={1} step={0.01} fallback={0.12} format={percent} />
            <FieldSlider path={["look", "glass", "blur"]} label="Blur" min={0} max={32} step={0.5} fallback={1} format={px} />
            <FieldSlider path={["look", "glass", "depth"]} label="Depth" min={0} max={1} step={0.01} fallback={0.2} format={percent} />
            <FieldSlider path={["look", "glass", "sheen"]} label="Sheen" min={0} max={1} step={0.01} fallback={0} format={percent} />
          </div>
        </Block>
        <ToggleSetting path={["look", "flyouts", "blur"]} title="Glass flyouts" detail="Menus and panels from this bar are glass (off: solid)." fallback />
      </Group>

      <Group title="Family">
        <SelectSetting<Family> path={["look", "family"]} title="Menus and panels" detail="Which system the flyouts this bar opens are drawn after." options={FAMILY_OPTIONS} fallback="mono" />
        <SelectSetting<GlyphSet> path={["look", "glyphs"]} title="Icons" detail="The module glyphs. An icon pack, if set, wins." options={GLYPH_OPTIONS} fallback="default" />
      </Group>

      <Group title="Edges and depth">
        <Block title="Border, glow and shadow" detail="Per state: hover and pressed start from rest.">
          <StateEdges />
        </Block>
      </Group>

      <Group title="Buttons">
        <Block title="Module and task buttons" detail="The look of each button at rest, under the pointer, and pressed (or open, or the app in front).">
          <ItemStates />
        </Block>
      </Group>

      <Group title="Indicators">
        <Block title="Running indicator colors" detail="The style (dot, bar, glow, pill) is chosen in Dock.">
          <div className="space-y-3">
            <FieldColor path={["look", "indicator", "running"]} label="Running" />
            <FieldColor path={["look", "indicator", "active"]} label="In front" />
            <FieldColor path={["look", "indicator", "attention"]} label="Attention" />
          </div>
        </Block>
      </Group>

      <ThemeVarsPanel />

      <Group title="Base appearance">
        <div className="px-5 py-4">
          <p className="mb-3 text-[13px] text-ink-3">The tint, opacity, blur, text and accent the look builds on. They are shared with the theme and with widgets.</p>
          <AppearanceEditor target="bars" hidePreview value={doc.appearance ?? {}} onChange={(appearance) => set(["appearance"], Object.keys(appearance).length === 0 ? undefined : appearance)} below={[skins.bars]} fallback="the theme" />
        </div>
      </Group>
    </div>
  );
}

function BackgroundEditor() {
  const { resolved, catalog, set } = useBars();
  const fill = resolved.look.background;
  const allowed = catalog.backgroundTypes[resolved.preset];
  const path = ["look", "background"] as const;
  const type: FillType | "preset" = fill?.type ?? "preset";

  return (
    <div className="space-y-4">
      <Segmented<FillType | "preset">
        className="max-w-xl"
        label="Background type"
        hideLabel
        value={type}
        onChange={(next) => {
          if (next === "preset") set(path, undefined);
          // An image needs a file from the start: the host refuses an empty path.
          else if (next === "image") void pickImage().then((file) => file && set(path, { ...FILL_START.image, path: file }));
          else set(path, FILL_START[next]);
        }}
        options={[
          { value: "preset", label: "Preset's", hint: "Follow the preset, or the appearance's tint when it has none" },
          ...FILL_TYPES.map((t) => ({
            value: t,
            label: FILL_LABELS[t],
            disabled: !allowed.includes(t) && t !== fill?.type,
            hint: allowed.includes(t) ? undefined : `The ${resolved.preset} preset's background can't be combined with this type. Pick another preset first.`,
          })),
        ]}
      />
      {allowed.length < FILL_TYPES.length && (
        <Note>
          Over the {resolved.preset} preset, only {allowed.map((t) => FILL_LABELS[t].toLowerCase()).join(", ")} can be chosen: the host merges your file over the preset key by key, and this preset's background leaves keys other types don't take. Applying another preset lifts that.
        </Note>
      )}

      {fill?.type === "solid" && <FieldColor path={[...path, "color"]} label="Color" />}
      {fill?.type === "gradient" && (
        <GradientEditor label="Background" value={fill} onChange={(gradient) => set(path, { type: "gradient", ...gradient })} />
      )}
      {fill?.type === "image" && <ImageFields />}
      {fill?.type === "wallpaper" && (
        <div className="space-y-3">
          <FieldSlider path={[...path, "brightness"]} label="Brightness" min={0} max={2} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
          <FieldSlider path={[...path, "blur"]} label="Extra blur" min={0} max={64} fallback={0} format={px} />
          <FieldSlider path={[...path, "interval"]} label="Refresh" min={2} max={600} fallback={10} format={(v) => `${v} s`} />
          <Note>
            Wallpaper glass shows a blurred snapshot of this monitor's wallpaper under the tint, lined up with the real one. The engine publishes it once per theme, and every few seconds while the wallpaper is animated (the refresh above). It stops in performance mode and for web and app wallpapers.
          </Note>
        </div>
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
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 text-[13px] text-ink-2">Image</span>
        <input aria-label="Background image" readOnly value={image?.path ?? ""} title={image?.path} className={`${INPUT_BASE} flex-1 font-mono text-[13px]`} />
        <IconButton icon={FolderOpen} label="Choose an image" onClick={() => void pickImage().then((file) => file && set([...path, "path"], file))} />
      </div>
      <Note>The host copies the file into its own folder and serves it from there. A path inside the theme's folder works too (write it in bars.json).</Note>
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 text-[13px] text-ink-2">Fit</span>
        <Select<ImageFit> label="Image fit" className="w-40" value={image?.fit ?? "cover"} options={(["cover", "contain", "fill", "tile"] as const).map((f) => ({ value: f, label: f }))} onChange={(fit) => set([...path, "fit"], fit)} />
        <span className="ml-3 text-[13px] text-ink-2">Position</span>
        <input aria-label="Image position" value={image?.position ?? "center"} spellCheck={false} onChange={(e) => set([...path, "position"], e.currentTarget.value)} className={`${INPUT_BASE} w-40 font-mono text-[13px]`} />
      </div>
      <FieldSlider path={[...path, "brightness"]} label="Brightness" min={0} max={2} step={0.05} fallback={1} format={(v) => `${v.toFixed(2)}x`} />
      <FieldSlider path={[...path, "blur"]} label="Blur" min={0} max={64} fallback={0} format={px} />
    </div>
  );
}

const STATE_LABELS: Record<State, string> = { rest: "Rest", hover: "Hover", active: "Pressed" };
const STATE_OPTIONS = STATES.map((value) => ({ value, label: STATE_LABELS[value] }));

function StateEdges() {
  const [state, setState] = useState<State>("rest");
  return (
    <div className="space-y-4">
      <Segmented<State> className="w-72" label="State" hideLabel value={state} options={STATE_OPTIONS} onChange={setState} />
      <div className="space-y-3">
        <h4 className="text-[12px] font-semibold tracking-[0.1em] text-ink-3 uppercase">Border</h4>
        <FieldSlider path={["look", "border", state, "width"]} label="Width" min={0} max={8} step={0.5} fallback={state === "rest" ? 1 : 0} format={px} />
        <FieldPaint path={["look", "border", state, "color"]} label="Color" placeholder="#ffffff1f" />
        <h4 className="pt-2 text-[12px] font-semibold tracking-[0.1em] text-ink-3 uppercase">Glow</h4>
        <FieldColor path={["look", "glow", state, "color"]} label="Color" placeholder="accent" />
        <FieldSlider path={["look", "glow", state, "size"]} label="Size" min={0} max={64} fallback={0} format={px} />
        <FieldSlider path={["look", "glow", state, "opacity"]} label="Opacity" min={0} max={1} step={0.01} fallback={0.5} format={percent} />
        <h4 className="pt-2 text-[12px] font-semibold tracking-[0.1em] text-ink-3 uppercase">Shadow</h4>
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
    <div className="space-y-4">
      <FieldSlider path={["look", "items", "radius"]} label="Corner radius" min={0} max={32} fallback={8} format={px} />
      <Segmented<State> className="w-72" label="Button state" hideLabel value={state} options={STATE_OPTIONS} onChange={setState} />
      <div className="space-y-3">
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
