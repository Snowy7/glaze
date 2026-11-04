import { ChevronRight } from "lucide-react";
import { useState } from "react";

import { useBars } from "../../lib/barsContext";
import { MOTION_STYLES, type Curve, type MotionStyle, type Performance, type ReduceMotion } from "../../lib/barsSchema";
import { describeCurve, leavesOf, resolveLeaf, SCOPE_PATH, type Leaf, type Scope } from "../../lib/motion";
import { Block, Group, Note, Notice, Row, Segmented, Select, Switch } from "../ui";
import { ChoiceSetting, FieldSlider, boolAt, numberAt } from "./BarSettings";
import { CurvePicker, CurvePreview } from "./CurvePicker";

const SCOPES: readonly { value: Scope; label: string; hint?: string }[] = [
  { value: "global", label: "Everything", hint: "The animation node itself: every bar and flyout" },
  { value: "bar", label: "Both bars", hint: "animation.bar: the top bar and the dock" },
  { value: "top", label: "Top bar", hint: "animation.bar.top" },
  { value: "dock", label: "Dock", hint: "animation.bar.dock" },
];

const LEAF_INFO: Record<Leaf, { label: string; hint: string }> = {
  hover: { label: "Hover", hint: "A button reacting to the pointer" },
  press: { label: "Press", hint: "A button being pressed" },
  open: { label: "Open", hint: "Flyouts and menus appearing" },
  magnify: { label: "Magnify", hint: "The dock's hover zoom. Keeps its own quick timing, so a slow global duration never makes it lag." },
  bounce: { label: "Launch bounce", hint: "An app launching from the dock" },
  attention: { label: "Attention", hint: "An app asking for attention" },
};

const STYLE_LABELS: Record<MotionStyle, string> = { slide: "Slide", fade: "Fade", pop: "Pop", scale: "Scale", none: "None" };
const REDUCE: readonly { value: ReduceMotion; label: string; hint?: string }[] = [
  { value: "system", label: "Follow Windows", hint: "Windows' \"Animation effects\" setting, followed live" },
  { value: "always", label: "Always reduce" },
  { value: "never", label: "Never reduce" },
];
const PERFORMANCE: readonly { value: Performance; label: string; hint?: string }[] = [
  { value: "auto", label: "On battery", hint: "On while the laptop is unplugged" },
  { value: "on", label: "Always" },
  { value: "off", label: "Never" },
];

/** The animation tree: global, both bars, top bar and dock, each with its own leaves (docs/bars.md, "Animation"). */
export function MotionPanel() {
  const [scope, setScope] = useState<Scope>("global");
  return (
    <div className="flex flex-col gap-6">
      <Group title="Reduced motion and performance">
        <ChoiceSetting<ReduceMotion> path={["animation", "reduceMotion"]} title="Reduce motion" detail="Reduced, every duration is 0." options={REDUCE} fallback="system" wide />
        <ChoiceSetting<Performance> path={["performance"]} title="Performance mode" detail="Turns off blur, grain, glow and every animation, and stops periodic wallpaper snapshots." options={PERFORMANCE} fallback="auto" />
      </Group>

      <Notice title="How the tree resolves">
        Each value is the most specific one on its path: everything, then both bars, then top bar or dock, then the same leaf under each of those. Anything switched off along the way turns that animation off. Everything is CSS, so
        nothing runs while the bars are idle.
      </Notice>

      <Segmented<Scope> label="Scope" hideLabel value={scope} options={SCOPES} onChange={setScope} />

      <NodeFields scope={scope} />

      <Group title={`${SCOPES.find((s) => s.value === scope)?.label}: leaves`}>
        {leavesOf(scope).map((leaf) => (
          <LeafRow key={`${scope}:${leaf}`} scope={scope} leaf={leaf} />
        ))}
      </Group>
    </div>
  );
}

const at = (scope: Scope, ...rest: string[]) => ["animation", ...SCOPE_PATH[scope], ...rest];

/** The node's own fields. For the bars it is the show animation (appearing, the dock's auto-hide reveal). */
function NodeFields({ scope }: { scope: Scope }) {
  const { resolved, set } = useBars();
  const tree = resolved.animation;
  const node = (scope === "global" ? tree : scope === "bar" ? tree.bar : tree.bar?.[scope]) ?? {};
  const curve: Curve = node.curve ?? tree.curve ?? "smooth";
  const bar = scope === "dock" ? "dock" : "top";
  const shown = resolveLeaf(tree, bar, "show");
  const label = scope === "global" ? "Everything" : scope === "bar" ? "Both bars" : scope === "top" ? "The top bar" : "The dock";

  return (
    <Group title={`${label}: timing`}>
      <Row title="Enabled" detail="Off turns off everything beneath this node, including its leaves.">
        <Switch label={`${label} enabled`} checked={boolAt(resolved, at(scope, "enabled")) ?? true} onChange={(on) => set(at(scope, "enabled"), on ? undefined : false)} />
      </Row>
      <Block title="Duration" detail="Milliseconds. A spring ignores this and takes as long as it needs to settle.">
        <FieldSlider path={at(scope, "duration")} label="Duration" min={0} max={1000} step={10} fallback={numberAt(tree, ["duration"]) ?? shown.duration} format={(v) => `${v} ms`} />
      </Block>
      <Block title="Curve" detail={`How it moves. This node's value: ${describeCurve(curve)}.`}>
        <CurvePicker label={label} value={curve} duration={numberAt(node, ["duration"]) ?? shown.duration} onChange={(next) => set(at(scope, "curve"), next)} />
        {node.curve !== undefined && (
          <button type="button" onClick={() => set(at(scope, "curve"), undefined)} className="mt-2 cursor-default text-xs text-ink-3 underline decoration-line-hi underline-offset-2 hover:text-ink">
            Reset the curve
          </button>
        )}
      </Block>
      <Row title="Style" detail={scope === "global" || scope === "bar" ? "How bars appear and flyouts open, unless a node below says otherwise." : "How this bar appears."}>
        <Select<MotionStyle> label={`${label} style`} className="w-40" value={node.style ?? shown.style} options={MOTION_STYLES.map((s) => ({ value: s, label: STYLE_LABELS[s] }))} onChange={(style) => set(at(scope, "style"), style)} />
      </Row>
    </Group>
  );
}

/** One leaf: what it comes to now, with its own switch, duration and curve behind a disclosure. */
function LeafRow({ scope, leaf }: { scope: Scope; leaf: Leaf }) {
  const { resolved, set } = useBars();
  const [open, setOpen] = useState(false);
  const bar = scope === "dock" ? "dock" : "top";
  const now = resolveLeaf(resolved.animation, bar, leaf);
  const own = (scope === "global" ? resolved.animation : scope === "bar" ? resolved.animation.bar : resolved.animation.bar?.[scope])?.[leaf] ?? {};
  const info = LEAF_INFO[leaf];
  const ownTiming = leaf === "magnify" || leaf === "bounce" || leaf === "attention";

  return (
    <div>
      <div className="flex items-center gap-4 px-5 py-3.5">
        <button type="button" aria-expanded={open} aria-label={`${info.label} details`} onClick={() => setOpen(!open)} className="grid size-6 shrink-0 cursor-default place-items-center rounded-md text-ink-3 hover:text-ink">
          <ChevronRight className={`size-4 transition-transform ${open ? "rotate-90" : ""}`} aria-hidden />
        </button>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-medium text-ink">{info.label}</div>
          <div className="mt-0.5 text-[12.5px] text-ink-3">
            {now.enabled ? `${now.duration} ms · ${describeCurve(now.curve)}${leaf === "open" ? ` · ${STYLE_LABELS[now.style]}` : ""}` : "Off"}
          </div>
        </div>
        <CurvePreview curve={now.curve} duration={now.duration} width={96} />
        <Switch label={`${info.label} enabled`} checked={boolAt(own, ["enabled"]) ?? true} onChange={(on) => set(at(scope, leaf, "enabled"), on ? undefined : false)} />
      </div>
      {open && (
        <div className="space-y-4 border-t border-line bg-canvas/50 px-5 py-4">
          <p className="text-[12.5px] text-ink-3">{info.hint}</p>
          <FieldSlider path={at(scope, leaf, "duration")} label="Duration" min={0} max={leaf === "bounce" || leaf === "attention" ? 2000 : 1000} step={10} fallback={now.duration} format={(v) => `${v} ms`} />
          <CurvePicker label={info.label} value={own.curve ?? now.curve} duration={now.duration} onChange={(curve) => set(at(scope, leaf, "curve"), curve)} />
          {leaf === "open" && (
            <div className="flex items-center gap-3">
              <span className="w-28 shrink-0 text-[13px] text-ink-2">Style</span>
              <Select<MotionStyle> label="Open style" className="w-40" value={own.style ?? now.style} options={MOTION_STYLES.map((s) => ({ value: s, label: STYLE_LABELS[s] }))} onChange={(style) => set(at(scope, leaf, "style"), style)} />
            </div>
          )}
          {ownTiming && scope !== "dock" && <Note>Only the dock's own value counts for this leaf.</Note>}
        </div>
      )}
    </div>
  );
}
