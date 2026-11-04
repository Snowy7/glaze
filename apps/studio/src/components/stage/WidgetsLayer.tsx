import { useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";

import type { Appearance, PlacedWidget, WidgetInfo } from "../../lib/api";
import { mergeLayers, resolve as resolveLook, WIDGET_DEFAULTS } from "../../lib/appearance";
import { boxOf, cellAt, clampCell, fits, sizeOf, type Cells, type DesktopGrid, type Placement } from "../../lib/desktopGrid";
import { settingsWithDefaults } from "../../lib/widgetHost";
import { widgetIcon } from "../../lib/widgetIcons";
import { Outline, useStageScale } from "./Stage";
import { WidgetFrame } from "./WidgetFrame";

type Props = {
  grid: DesktopGrid;
  /** The widgets on this monitor. */
  items: readonly PlacedWidget[];
  installed: readonly WidgetInfo[];
  /** The layers under each widget's own look, lowest first (the theme's, then the look of all widgets). */
  below: readonly Appearance[];
  theme: string;
  selected: string | null;
  /** Null: the widgets are shown, not edited (another layer has the focus). */
  onSelect: ((instance: string) => void) | null;
  onPlace?: (instance: string, cell: Cells, size: Cells) => void;
  /** Draw the grid's cells (while a widget is being moved). */
  showGrid?: boolean;
};

type Drag = { instance: string; kind: "move" | "resize"; dx: number; dy: number; moved: boolean };

const DRAG_THRESHOLD = 4;


/**
 * The placed widgets, live, at their cells. Click selects; drag moves on the grid (snapping to
 * cells, refusing overlaps); the selected one has a resize handle; arrow keys nudge it a cell.
 */
export function WidgetsLayer({ grid, items, installed, below, theme, selected, onSelect, onPlace, showGrid = false }: Props) {
  const k = useStageScale();
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);

  const placements: Placement[] = items.map((item) => ({ instance: item.instance, cell: item.cell ?? [0, 0], size: sizeOf(item, installed.find((w) => w.id === item.widget)) }));

  /** Where a drag would put the widget, and whether it can go there. */
  function target(placement: Placement, widget: WidgetInfo | undefined, d: Drag): { cell: Cells; size: Cells; ok: boolean } {
    const box = boxOf(grid, placement.cell, placement.size);
    if (d.kind === "move") {
      const cell = clampCell(grid, cellAt(grid, box.x + d.dx, box.y + d.dy), placement.size);
      return { cell, size: placement.size, ok: fits(grid, { ...placement, cell }, placements) };
    }
    const min = widget?.size.min ?? [1, 1];
    const max = widget?.size.max ?? [grid.columns, grid.rows];
    const stride = (axis: 0 | 1) => grid.cell[axis] + grid.gap;
    const size: Cells = [
      Math.min(max[0], Math.max(min[0], Math.round((box.width + d.dx + grid.gap) / stride(0)))),
      Math.min(max[1], Math.max(min[1], Math.round((box.height + d.dy + grid.gap) / stride(1)))),
    ];
    return { cell: placement.cell, size, ok: fits(grid, { ...placement, size }, placements) };
  }

  function begin(event: PointerEvent, placement: Placement, widget: WidgetInfo | undefined, kind: Drag["kind"]) {
    if (event.button !== 0 || !onSelect) return;
    event.stopPropagation();
    event.preventDefault();
    const start = { x: event.clientX, y: event.clientY };
    let current: Drag = { instance: placement.instance, kind, dx: 0, dy: 0, moved: false };
    const canEdit = onPlace !== undefined && (kind === "move" || widget?.resizable !== false);
    const move = (e: globalThis.PointerEvent) => {
      const dx = (e.clientX - start.x) / k;
      const dy = (e.clientY - start.y) / k;
      const moved = current.moved || Math.hypot(e.clientX - start.x, e.clientY - start.y) > DRAG_THRESHOLD;
      current = { ...current, dx, dy, moved };
      if (moved && canEdit) setDrag(current);
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setDrag(null);
      if (!current.moved || !canEdit) return onSelect(placement.instance);
      const result = target(placement, widget, current);
      const same = result.cell.every((v, i) => v === placement.cell[i]) && result.size.every((v, i) => v === placement.size[i]);
      if (!result.ok) {
        setRefused(placement.instance);
        window.setTimeout(() => setRefused(null), 400);
      } else if (!same) onPlace?.(placement.instance, result.cell, result.size);
      onSelect(placement.instance);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  function nudge(event: KeyboardEvent, placement: Placement) {
    const step = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!step || !onPlace) return;
    event.preventDefault();
    const cell: Cells = [placement.cell[0] + (step[0] ?? 0), placement.cell[1] + (step[1] ?? 0)];
    if (fits(grid, { ...placement, cell }, placements)) onPlace(placement.instance, cell, placement.size);
  }

  const dragging = drag ? placements.find((p) => p.instance === drag.instance) : undefined;
  const preview = drag && dragging ? target(dragging, installed.find((w) => w.id === items.find((i) => i.instance === drag.instance)?.widget), drag) : null;

  return (
    <div className="absolute inset-0">
      {(showGrid || drag) && <GridDots grid={grid} />}
      {preview && (
        <div
          aria-hidden
          className={`absolute z-20 transition-[left,top,width,height] duration-100 ${preview.ok ? "bg-white/12" : "bg-black/30"}`}
          style={{ ...boxStyle(boxOf(grid, preview.cell, preview.size), 14), boxShadow: `inset 0 0 0 ${1.5 / k}px ${preview.ok ? "rgb(255 255 255 / 0.85)" : "rgb(255 255 255 / 0.35)"}` }}
        />
      )}
      {items.map((item, index) => {
        const placement = placements[index];
        if (!placement) return null;
        const widget = installed.find((w) => w.id === item.widget);
        const box = boxOf(grid, placement.cell, placement.size);
        const own = drag?.instance === item.instance ? drag : null;
        const shift: CSSProperties = own?.kind === "move" ? { transform: `translate(${own.dx}px, ${own.dy}px)`, zIndex: 25, opacity: 0.92 } : {};
        const resize: CSSProperties = own?.kind === "resize" ? { width: Math.max(box.width / 2, box.width + own.dx), height: Math.max(box.height / 2, box.height + own.dy) } : {};
        const look = resolveLook(WIDGET_DEFAULTS, ...below, item.appearance);
        const isSelected = selected === item.instance;
        return (
          <div
            key={item.instance}
            className={`absolute ${refused === item.instance ? "animate-shake" : ""}`}
            style={{ ...boxStyle(box), ...shift, ...resize }}
            onPointerEnter={() => setHovered(item.instance)}
            onPointerLeave={() => setHovered((h) => (h === item.instance ? null : h))}
          >
            {widget && !widget.error && widget.manifest ? (
              <WidgetFrame
                widget={widget}
                state={{
                  instance: item.instance,
                  manifest: widget.manifest,
                  settings: settingsWithDefaults(widget.manifest.settings, item.settings),
                  size: { cells: placement.size, px: [Math.round(box.width), Math.round(box.height)], dpi: 96 },
                  // What the shell sends: only set fields; the SDK fills in the rest.
                  appearance: mergeLayers(...below, item.appearance),
                  theme,
                }}
              />
            ) : (
              <MissingWidget name={widget?.name ?? item.widget} radius={look.radius} />
            )}
            {onSelect && (
              <button
                type="button"
                aria-label={`${widget?.name ?? item.widget} widget`}
                aria-pressed={isSelected}
                className="absolute inset-0 cursor-default outline-none"
                style={{ borderRadius: look.radius }}
                onPointerDown={(e) => begin(e, placement, widget, "move")}
                onKeyDown={(e) => nudge(e, placement)}
                onClick={(e) => e.detail === 0 && onSelect(item.instance)}
              />
            )}
            <Outline box={{ x: 0, y: 0, width: box.width, height: box.height }} selected={isSelected && !own} hovered={hovered === item.instance && !drag && !!onSelect} label={widget?.name ?? item.widget} radius={look.radius} />
            {isSelected && onPlace && widget?.resizable !== false && !own && (
              <span
                role="presentation"
                className="absolute z-40 cursor-nwse-resize rounded-full bg-white shadow ring-black/40"
                style={{ right: -7 / k, bottom: -7 / k, width: 12 / k, height: 12 / k, boxShadow: `0 0 0 ${1.5 / k}px rgb(0 0 0 / 0.45)` }}
                onPointerDown={(e) => begin(e, placement, widget, "resize")}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

const boxStyle = (box: { x: number; y: number; width: number; height: number }, radius = 0): CSSProperties => ({ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: radius });

/** The grid's cells, faintly, while arranging. */
function GridDots({ grid }: { grid: DesktopGrid }) {
  const k = useStageScale();
  const cells = Array.from({ length: grid.columns * grid.rows }, (_, i) => boxOf(grid, [i % grid.columns, Math.floor(i / grid.columns)], [1, 1]));
  return (
    <div className="pointer-events-none absolute inset-0 z-10" aria-hidden>
      {cells.map((cell, i) => (
        <span key={i} className="absolute rounded-[6px]" style={{ ...boxStyle(cell, 8), boxShadow: `inset 0 0 0 ${1 / k}px rgb(255 255 255 / 0.13)` }} />
      ))}
    </div>
  );
}

/** A placed widget that isn't installed (kept in the layout, not shown on the desktop). */
function MissingWidget({ name, radius }: { name: string; radius: number }) {
  const Icon = widgetIcon(name);
  return (
    <div className="absolute inset-0 grid place-items-center border-2 border-dashed border-white/25 bg-black/30 text-white/60" style={{ borderRadius: radius }}>
      <div className="flex flex-col items-center gap-2 text-center">
        <Icon className="size-8" strokeWidth={1.5} aria-hidden />
        <span className="text-sm font-medium">{name} isn't installed</span>
      </div>
    </div>
  );
}
