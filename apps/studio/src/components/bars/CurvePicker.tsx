import { useRef, type CSSProperties, type PointerEvent } from "react";

import { tidy } from "../../lib/barsDoc";
import { NAMED_CURVES, type Curve, type NamedCurve, type Spring } from "../../lib/barsSchema";
import { describeCurve, easing, springAt, springSettle } from "../../lib/motion";
import { Segmented, Slider } from "../ui";

/** A dot that runs along a track with `curve` and `duration` (a spring brings its own), over and over. */
export function CurvePreview({ curve, duration, width = 120 }: { curve: Curve; duration: number; width?: number }) {
  const { easing: timing, duration: own } = easing(curve);
  const ms = Math.max(own ?? duration, 40);
  return (
    <span className="relative block h-3 shrink-0 rounded-full bg-surface-hi" style={{ width }} aria-hidden>
      <span
        className="absolute top-0.5 left-0.5 size-2 rounded-full bg-ink"
        style={{ "--run": `${width - 12}px`, animation: `curve-run ${ms / 0.4}ms ${timing} infinite` } as CSSProperties}
      />
    </span>
  );
}

type Mode = "named" | "bezier" | "spring";
const modeOf = (curve: Curve): Mode => (typeof curve === "string" ? "named" : Array.isArray(curve) ? "bezier" : "spring");

const START: Record<Mode, Curve> = {
  named: "smooth",
  bezier: [0.22, 0.7, 0.2, 1],
  spring: { mass: 1, stiffness: 220, damping: 22 },
};

/**
 * Picks an easing: a named preset, a cubic Bézier edited on its graph, or a spring (mass, stiffness,
 * damping). A spring sets its own duration, so its settling time is shown instead.
 */
export function CurvePicker({ label, value, duration, onChange }: { label: string; value: Curve; duration: number; onChange: (curve: Curve) => void }) {
  const mode = modeOf(value);
  return (
    <div className="space-y-3">
      <Segmented<Mode>
        className="w-80"
        label={`${label} curve type`}
        hideLabel
        value={mode}
        onChange={(next) => next !== mode && onChange(START[next])}
        options={[{ value: "named", label: "Preset" }, { value: "bezier", label: "Bézier" }, { value: "spring", label: "Spring" }]}
      />
      {typeof value === "string" && (
        <ul className="grid grid-cols-3 gap-2" aria-label={`${label} presets`}>
          {NAMED_CURVES.map((name: NamedCurve) => (
            <li key={name}>
              <button
                type="button"
                aria-pressed={value === name}
                onClick={() => onChange(name)}
                className={`flex w-full cursor-default items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] font-medium ring-1 transition ${value === name ? "bg-solid text-solid-ink ring-transparent" : "bg-canvas text-ink-2 ring-line-strong hover:ring-line-hi"}`}
              >
                {name}
              </button>
            </li>
          ))}
        </ul>
      )}
      {Array.isArray(value) && <BezierEditor label={label} value={value} onChange={onChange} />}
      {typeof value === "object" && !Array.isArray(value) && <SpringEditor label={label} value={value} onChange={onChange} />}
      <div className="flex items-center gap-3 text-xs text-ink-3">
        <CurvePreview curve={value} duration={duration} width={160} />
        {describeCurve(value)}
        {typeof value !== "string" && !Array.isArray(value) && <span>settles in {Math.round(springSettle(value) * 1000)} ms</span>}
      </div>
    </div>
  );
}

// The graph shows x 0..1 and y -0.5..1.5; the host takes y from -2 to 3, which the fields allow.
const GRAPH = { size: 160, yMin: -0.5, yMax: 1.5 } as const;
const gx = (x: number) => x * GRAPH.size;
const gy = (y: number) => ((GRAPH.yMax - y) / (GRAPH.yMax - GRAPH.yMin)) * GRAPH.size;

function BezierEditor({ label, value, onChange }: { label: string; value: [number, number, number, number]; onChange: (curve: Curve) => void }) {
  const [x1, y1, x2, y2] = value;
  const svg = useRef<SVGSVGElement>(null);

  /** Moves control point `which` (0 or 1) to the pointer. */
  function drag(which: 0 | 1, event: PointerEvent) {
    const box = svg.current?.getBoundingClientRect();
    if (!box) return;
    const x = Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
    const y = GRAPH.yMax - ((event.clientY - box.top) / box.height) * (GRAPH.yMax - GRAPH.yMin);
    const clamped = Math.min(3, Math.max(-2, y));
    onChange(which === 0 ? [tidy(x), tidy(clamped), x2, y2] : [x1, y1, tidy(x), tidy(clamped)]);
  }

  const handle = (which: 0 | 1, x: number, y: number) => (
    <circle
      key={which}
      cx={gx(x)}
      cy={gy(y)}
      r={6}
      className="cursor-grab fill-ink stroke-canvas active:cursor-grabbing"
      strokeWidth={2}
      onPointerDown={(e) => e.currentTarget.setPointerCapture(e.pointerId)}
      onPointerMove={(e) => e.buttons === 1 && drag(which, e)}
    />
  );

  const field = (name: string, index: number, min: number, max: number) => (
    <Slider key={name} className="w-48" label={`${label} ${name}`} value={value[index] ?? 0} min={min} max={max} step={0.01} format={(v) => v.toFixed(2)} onChange={(v) => onChange(value.map((n, i) => (i === index ? tidy(v) : n)) as [number, number, number, number])} />
  );

  return (
    <div className="flex flex-wrap items-start gap-5">
      <svg ref={svg} width={GRAPH.size} height={GRAPH.size} className="shrink-0 touch-none rounded-lg bg-canvas ring-1 ring-line-strong" role="img" aria-label="Curve graph; drag the two handles">
        <line x1={0} y1={gy(0)} x2={GRAPH.size} y2={gy(0)} className="stroke-line" />
        <line x1={0} y1={gy(1)} x2={GRAPH.size} y2={gy(1)} className="stroke-line" />
        <line x1={gx(0)} y1={gy(0)} x2={gx(x1)} y2={gy(y1)} className="stroke-ink-3" />
        <line x1={gx(1)} y1={gy(1)} x2={gx(x2)} y2={gy(y2)} className="stroke-ink-3" />
        <path d={`M ${gx(0)} ${gy(0)} C ${gx(x1)} ${gy(y1)}, ${gx(x2)} ${gy(y2)}, ${gx(1)} ${gy(1)}`} className="fill-none stroke-ink" strokeWidth={2} />
        {handle(0, x1, y1)}
        {handle(1, x2, y2)}
      </svg>
      <div className="space-y-2">
        {field("x1", 0, 0, 1)}
        {field("y1", 1, -2, 3)}
        {field("x2", 2, 0, 1)}
        {field("y2", 3, -2, 3)}
      </div>
    </div>
  );
}

function SpringEditor({ label, value, onChange }: { label: string; value: Spring; onChange: (curve: Curve) => void }) {
  const samples = Array.from({ length: 49 }, (_, i) => {
    const settle = Math.max(springSettle(value), 0.05);
    const t = (i / 48) * settle;
    return `${(i / 48) * GRAPH.size},${gy(springAt(t, value)) * 0.6 + 12}`;
  }).join(" ");
  return (
    <div className="flex flex-wrap items-start gap-5">
      <svg width={GRAPH.size} height={GRAPH.size * 0.6 + 24} className="shrink-0 rounded-lg bg-canvas ring-1 ring-line-strong" role="img" aria-label="Spring response over time">
        <line x1={0} y1={gy(1) * 0.6 + 12} x2={GRAPH.size} y2={gy(1) * 0.6 + 12} className="stroke-line" />
        <polyline points={samples} className="fill-none stroke-ink" strokeWidth={2} />
      </svg>
      <div className="space-y-2">
        <Slider className="w-56" label={`${label} mass`} value={value.mass} min={0.1} max={10} step={0.1} format={(v) => `mass ${v.toFixed(1)}`} onChange={(mass) => onChange({ ...value, mass: tidy(mass) })} />
        <Slider className="w-56" label={`${label} stiffness`} value={value.stiffness} min={1} max={2000} step={1} format={(v) => `stiff ${v}`} onChange={(stiffness) => onChange({ ...value, stiffness })} />
        <Slider className="w-56" label={`${label} damping`} value={value.damping} min={0} max={200} step={1} format={(v) => `damp ${v}`} onChange={(damping) => onChange({ ...value, damping })} />
      </div>
    </div>
  );
}
