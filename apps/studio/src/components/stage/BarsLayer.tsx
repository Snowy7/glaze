import { useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";

import type { BarsCatalog } from "../../lib/api";
import { contains, dropTarget, union, type BarGeometry, type ModuleSpot, type ZoneId } from "../../lib/barsGeometry";
import type { Resolved } from "../../lib/barsDoc";
import type { BarKind, BarsDoc } from "../../lib/barsSchema";
import type { Box } from "../../lib/desktopGrid";
import { describeModule } from "../../lib/modules";
import { BarFrame, useBarsShell } from "../bars/BarFrame";
import { BarsSilhouette } from "./BarsSilhouette";
import { Outline, useStageScale, useStageScreen } from "./Stage";

// The top bar and the bottom bar on the stage: the real bars page, live, one frame each. Studio
// reads where their surfaces and modules are, so a click picks a bar or a module and a drag moves
// a module within a bar, to another zone or bar, or off the bar to remove it.

/** What is picked on the bars: a whole bar, or one of its modules. */
export type BarPick = { bar: BarKind; module?: { zone: ZoneId; ref: string } };

/** A module drop: where it came from and where it goes (null: off the bars, removed). */
export type ModuleMove = { from: { bar: BarKind; zone: ZoneId; ref: string }; to: { bar: BarKind; zone: ZoneId; ordinal: number } | null };

type Props = {
  doc: BarsDoc;
  config: Resolved;
  catalog: BarsCatalog;
  wallpaper: string | null;
  /** `show`: just the bars; `bars`: a bar can be picked; `modules`: bars and modules, and modules drag. */
  mode: "show" | "bars" | "modules";
  selection?: BarPick | null;
  onSelect?: (pick: BarPick) => void;
  onMove?: (move: ModuleMove) => void;
};

/** The module lists a bar has: the top bar's three, a taskbar dock's two; a floating dock has none. */
export function zonesOf(config: Resolved, bar: BarKind): readonly ZoneId[] {
  if (bar === "top") return ["left", "center", "right"];
  return config.dock.style === "taskbar" ? ["left", "right"] : [];
}

const DRAG_THRESHOLD = 4;
const BARS: readonly BarKind[] = ["top", "dock"];

/** The bars on the stage's screen (`useStageScreen`: the real one, or the narrower one the Bars page draws). */
export function BarsLayer({ doc, config, catalog, wallpaper, mode, selection = null, onSelect, onMove }: Props) {
  const screen = useStageScreen();
  const shell = useBarsShell();
  const k = useStageScale();
  const layer = useRef<HTMLDivElement>(null);
  const [geometry, setGeometry] = useState<Partial<Record<BarKind, BarGeometry>>>({});
  const [hover, setHover] = useState<BarPick | null>(null);
  const [drag, setDrag] = useState<{ spot: ModuleSpot; x: number; y: number; moved: boolean } | null>(null);

  if (shell === false) return <BarsSilhouette config={config} catalog={catalog} screen={screen} selected={selection?.bar ?? null} onSelect={mode === "show" ? null : (bar) => onSelect?.({ bar })} />;
  if (shell === null) return null;

  const on = BARS.filter((bar) => (bar === "top" ? config.top.enabled : config.dock.enabled));
  const shown = on.flatMap((bar) => geometry[bar] ?? []);

  /** A page point (pointer) in stage DIPs. */
  const point = (event: { clientX: number; clientY: number }) => {
    const box = layer.current?.getBoundingClientRect();
    return box ? { x: (event.clientX - box.left) / k, y: (event.clientY - box.top) / k } : { x: 0, y: 0 };
  };

  /** What's under a stage point: a module (in `modules` mode), else a bar. */
  const pick = (x: number, y: number): { pick: BarPick; spot?: ModuleSpot } | null => {
    for (const g of [...shown].reverse()) {
      const band = union(g.bands);
      if (!band || !contains(band, x, y, 2)) continue;
      const spot = mode === "modules" ? g.modules.find((m) => contains(m.box, x, y, 2)) : undefined;
      return { pick: spot ? { bar: g.bar, module: { zone: spot.zone, ref: spot.ref } } : { bar: g.bar }, spot };
    }
    return null;
  };

  function onPointerMove(event: ReactPointerEvent) {
    if (drag) return;
    const { x, y } = point(event);
    setHover(pick(x, y)?.pick ?? null);
  }

  function onPointerDown(event: ReactPointerEvent) {
    if (event.button !== 0) return;
    const start = point(event);
    const hit = pick(start.x, start.y);
    if (!hit) return;
    event.preventDefault();
    event.stopPropagation();
    const spot = hit.spot;
    let current = { moved: false, x: start.x, y: start.y };
    const move = (e: PointerEvent) => {
      const p = point(e);
      const moved = current.moved || Math.hypot(p.x - start.x, p.y - start.y) * k > DRAG_THRESHOLD;
      current = { moved, x: p.x, y: p.y };
      if (moved && spot && onMove) setDrag({ spot, ...current });
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setDrag(null);
      if (current.moved && spot && onMove) {
        const to = dropTarget(shown, (bar) => zonesOf(config, bar), current.x, current.y);
        onMove({ from: { bar: spot.bar, zone: spot.zone, ref: spot.ref }, to });
        onSelect?.({ bar: to?.bar ?? spot.bar, ...(to && { module: { zone: to.zone, ref: spot.ref } }) });
      } else onSelect?.(hit.pick);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  const target = drag ? dropTarget(shown, (bar) => zonesOf(config, bar), drag.x, drag.y) : null;
  const interactive = mode !== "show";

  return (
    <div ref={layer} className="absolute inset-0">
      {on.map((bar) => (
        <BarFrame key={bar} bar={bar} doc={doc} width={screen.width} height={screen.height} wallpaper={wallpaper} clip onGeometry={(g) => setGeometry((all) => ({ ...all, [bar]: g }))} />
      ))}
      {interactive &&
        shown.map((g) => {
          const band = union(g.bands, 4);
          return (
            band && (
              <button
                key={g.bar}
                type="button"
                aria-label={g.bar === "top" ? "Top bar" : "Bottom bar"}
                aria-pressed={selection?.bar === g.bar && !selection.module}
                className="absolute z-20 cursor-default outline-none"
                style={boxStyle(band)}
                onPointerMove={onPointerMove}
                onPointerLeave={() => !drag && setHover(null)}
                onPointerDown={onPointerDown}
                // The keyboard's click (no pointer): pick the bar.
                onClick={(e) => e.detail === 0 && onSelect?.({ bar: g.bar })}
              />
            )
          );
        })}
      {shown.map((g) => (
        <Marks key={g.bar} geometry={g} selection={selection} hover={drag ? null : hover} dragging={drag?.spot ?? null} />
      ))}
      {drag?.moved && <DragGhost spot={drag.spot} x={drag.x} y={drag.y} removing={target === null} />}
      {target && <Caret geometry={shown.find((g) => g.bar === target.bar)} zone={target.zone} ordinal={target.ordinal} />}
    </div>
  );
}

const boxStyle = (box: Box): CSSProperties => ({ left: box.x, top: box.y, width: box.width, height: box.height });

/** The hover and selection frames on one bar and its modules. */
function Marks({ geometry, selection, hover, dragging }: { geometry: BarGeometry; selection: BarPick | null; hover: BarPick | null; dragging: ModuleSpot | null }) {
  const screen = useStageScreen();
  const band = union(geometry.bands);
  if (!band) return null;
  const name = geometry.bar === "top" ? "Top bar" : "Bottom bar";
  const isBar = (p: BarPick | null) => p?.bar === geometry.bar && !p.module;
  const isModule = (p: BarPick | null, spot: ModuleSpot) => p?.bar === geometry.bar && p.module?.ref === spot.ref && p.module.zone === spot.zone;
  // Name tags go where there's room: beside a bar that stands upright, else above it, or below one at the top.
  const beside = band.height > band.width ? (band.x + band.width / 2 < screen.width / 2 ? "right" : "left") : undefined;
  const below = band.y < 48;
  return (
    <>
      <Outline box={band} selected={isBar(selection)} hovered={isBar(hover)} label={name} radius={8} below={below} beside={beside} />
      {geometry.modules.map((spot) => (
        <div key={`${spot.zone}:${spot.ref}`} className={dragging?.ref === spot.ref && dragging.zone === spot.zone ? "opacity-40" : ""}>
          <Outline box={spot.box} selected={isModule(selection, spot)} hovered={isModule(hover, spot)} label={describeModule(spot.ref).label} radius={6} below={below} beside={beside} end={spot.box.x > band.x + band.width / 2} />
        </div>
      ))}
    </>
  );
}

/** Where a dragged module would land: a bright line between modules. */
function Caret({ geometry, zone, ordinal }: { geometry: BarGeometry | undefined; zone: ZoneId; ordinal: number }) {
  const k = useStageScale();
  const band = geometry && union(geometry.bands);
  if (!geometry || !band) return null;
  const vertical = band.height > band.width;
  const modules = geometry.modules.filter((m) => m.zone === zone);
  const zoneBox = geometry.zones.find((z) => z.zone === zone)?.box;
  const after = modules[ordinal - 1];
  const before = modules[ordinal];
  const at = before ? (vertical ? before.box.y : before.box.x) - 3 : after ? (vertical ? after.box.y + after.box.height : after.box.x + after.box.width) + 3 : zoneBox ? (vertical ? zoneBox.y + zoneBox.height / 2 : zoneBox.x + zoneBox.width / 2) : vertical ? band.y + band.height / 2 : band.x + band.width / 2;
  const thick = 2.5 / k;
  const style: CSSProperties = vertical ? { left: band.x + 4, top: at - thick / 2, width: band.width - 8, height: thick } : { left: at - thick / 2, top: band.y + 4, width: thick, height: band.height - 8 };
  return <div aria-hidden className="pointer-events-none absolute z-40 rounded-full bg-white shadow-[0_0_0_1px_rgb(0_0_0/0.4)]" style={style} />;
}

/** The module being dragged, following the pointer; it says "Remove" off the bars. */
function DragGhost({ spot, x, y, removing }: { spot: ModuleSpot; x: number; y: number; removing: boolean }) {
  const k = useStageScale();
  const { label, icon: Icon } = describeModule(spot.ref);
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute z-50 flex items-center rounded-md bg-white font-sans font-semibold whitespace-nowrap text-black shadow-pop"
      style={{ left: x + 10 / k, top: y + 10 / k, fontSize: 12 / k, gap: 6 / k, padding: `${5 / k}px ${8 / k}px`, borderRadius: 6 / k }}
    >
      <Icon style={{ width: 13 / k, height: 13 / k }} />
      {removing ? `Remove ${label}` : label}
    </div>
  );
}
