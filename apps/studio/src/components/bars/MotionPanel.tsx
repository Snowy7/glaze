import { ChevronRight } from "lucide-react";
import { useState } from "react";

import { useBars } from "../../lib/barsContext";
import { MOTION_STYLES, type BarKind, type Curve, type MotionStyle, type Performance, type ReduceMotion } from "../../lib/barsSchema";
import { easing, resolveLeaf, type Leaf } from "../../lib/motion";
import { Group, Select, Switch } from "../ui";
import { ChoiceSetting, FieldSlider, SettingRow, boolAt } from "./BarSettings";
import { CurvePicker, CurvePreview, curveName } from "./CurvePicker";

// Motion (docs/bars.md, "Animation"). One bar's own tree, edited through `forBar`: the root is how
// the bar appears; hover, press and open sit right under it; the dock's magnify, launch bounce and
// attention take their timing only from `bar.dock.<leaf>`, so that's where they are written.

const STYLE_LABELS: Record<MotionStyle, string> = { slide: "Slide", fade: "Fade", pop: "Pop", scale: "Grow", morph: "Grow from the button", none: "None" };
const STYLE_OPTIONS = MOTION_STYLES.map((value) => ({ value, label: STYLE_LABELS[value] }));

const LEAF_NAMES: Record<Leaf, string> = {
  hover: "Pointing at a button",
  press: "Pressing a button",
  open: "Menus opening",
  magnify: "Magnify",
  bounce: "Launch bounce",
  attention: "Asking for attention",
};

const DOCK_ONLY: readonly Leaf[] = ["magnify", "bounce", "attention"];

const ms = (v: number) => `${v} ms`;
const isSpring = (curve: Curve) => easing(curve).duration !== undefined;

/** Where a leaf lives in the bar's tree. */
const leafPath = (leaf: Leaf) => (DOCK_ONLY.includes(leaf) ? ["animation", "bar", "dock", leaf] : ["animation", leaf]);

/** One bar's motion: on or off, how it appears, then each movement (its own timing one click away). */
export function MotionPanel({ bar }: { bar: BarKind }) {
  const { resolved, set } = useBars();
  const tree = resolved.animation;
  const show = resolveLeaf(tree, bar, "show");
  const on = boolAt(resolved, ["animation", "enabled"]) ?? true;
  const leaves: readonly Leaf[] = bar === "dock" ? ["hover", "press", "open", ...DOCK_ONLY] : ["hover", "press", "open"];

  return (
    <>
      <SettingRow title="Animations" path={["animation", "enabled"]}>
        <Switch label="Animations" checked={on} onChange={(next) => set(["animation", "enabled"], next ? undefined : false, next ? "Turn animations on" : "Turn animations off")} />
      </SettingRow>
      {on && (
        <>
          <Group title={bar === "top" ? "Appearing" : "Appearing and auto-hide"}>
            <SettingRow title="Style" path={["animation", "style"]}>
              <Select<MotionStyle> label="Appearing style" size="sm" className="w-44" value={tree.style ?? show.style} options={STYLE_OPTIONS} onChange={(style) => set(["animation", "style"], style)} />
            </SettingRow>
            {!isSpring(show.curve) && <FieldSlider path={["animation", "duration"]} label="Duration" min={0} max={1000} step={10} fallback={show.duration} format={ms} />}
            <SettingRow title="Curve" path={["animation", "curve"]} stacked>
              <CurvePicker label="Appearing" value={tree.curve ?? show.curve} duration={show.duration} onChange={(curve) => set(["animation", "curve"], curve)} />
            </SettingRow>
          </Group>
          <Group title="Movements">
            <div className="-mx-2 flex flex-col">
              {leaves.map((leaf) => (
                <LeafRow key={leaf} bar={bar} leaf={leaf} />
              ))}
            </div>
          </Group>
        </>
      )}
    </>
  );
}

/** One movement: what it comes to now and its switch; its duration, curve (and style) when opened. */
function LeafRow({ bar, leaf }: { bar: BarKind; leaf: Leaf }) {
  const { resolved, set } = useBars();
  const [open, setOpen] = useState(false);
  const path = leafPath(leaf);
  const now = resolveLeaf(resolved.animation, bar, leaf);
  const own = boolAt(resolved, [...path, "enabled"]) ?? true;
  const name = LEAF_NAMES[leaf];

  return (
    <div className="rounded-lg">
      <div className="flex min-h-11 items-center gap-2 rounded-lg px-2 hover:bg-surface-hi/60">
        <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex min-w-0 flex-1 cursor-default items-center gap-2 py-1.5 text-left">
          <ChevronRight className={`size-4 shrink-0 text-ink-3 transition-transform duration-200 ${open ? "rotate-90" : ""}`} aria-hidden />
          <span className="min-w-0">
            <span className="block truncate text-[13px] text-ink">{name}</span>
            <span className="block truncate text-[12px] text-ink-3">{now.enabled ? `${now.duration} ms · ${curveName(now.curve)}` : "Off"}</span>
          </span>
        </button>
        {now.enabled && <CurvePreview curve={now.curve} duration={now.duration} width={56} />}
        <Switch label={name} size="sm" checked={own} onChange={(on) => set([...path, "enabled"], on ? undefined : false)} />
      </div>
      {open && own && (
        <div className="flex flex-col gap-3.5 px-2 pt-2 pb-3">
          {!isSpring(now.curve) && <FieldSlider path={[...path, "duration"]} label="Duration" min={0} max={leaf === "bounce" || leaf === "attention" ? 2000 : 1000} step={10} fallback={now.duration} format={ms} />}
          <SettingRow title="Curve" path={[...path, "curve"]} stacked>
            <CurvePicker label={name} value={now.curve} duration={now.duration} onChange={(curve) => set([...path, "curve"], curve)} />
          </SettingRow>
          {leaf === "open" && (
            <SettingRow title="Style" path={[...path, "style"]}>
              <Select<MotionStyle> label="Menus opening style" size="sm" className="w-44" value={now.style} options={STYLE_OPTIONS} onChange={(style) => set([...path, "style"], style)} />
            </SettingRow>
          )}
        </div>
      )}
    </div>
  );
}

const REDUCE: readonly { value: ReduceMotion; label: string; hint?: string }[] = [
  { value: "system", label: "Like Windows", hint: "Follows Windows' animation effects setting." },
  { value: "always", label: "Always" },
  { value: "never", label: "Never" },
];

const PERFORMANCE: readonly { value: Performance; label: string; hint?: string }[] = [
  { value: "auto", label: "On battery", hint: "While a laptop is unplugged." },
  { value: "on", label: "Always" },
  { value: "off", label: "Never" },
];

/** For both bars: reduced motion, and the mode that turns the effects off to save power. */
export function MotionAndPower() {
  const { resolved } = useBars();
  return (
    <>
      <ChoiceSetting<ReduceMotion> path={["animation", "reduceMotion"]} title="Reduce motion" detail={REDUCE.find((r) => r.value === (resolved.animation.reduceMotion ?? "system"))?.hint} options={REDUCE} fallback="system" />
      <ChoiceSetting<Performance> path={["performance"]} title="Save power" detail="Turns off blur, grain, glow and animations." options={PERFORMANCE} fallback="auto" />
    </>
  );
}
