// The widget grid as the widget host lays it out (docs/widgets.md, "Layout"), in DIPs, so the
// stage puts every widget where it really is and a drag lands on the cell it will get:
// the grid starts at the work area plus `margin`; as many whole cells as fit are laid out, then
// every cell is stretched so the grid spans the area edge to edge.

import type { Grid, MonitorInfo, PlacedWidget, WidgetInfo } from "./api";

export type Cells = [columns: number, rows: number];
export type Box = { x: number; y: number; width: number; height: number };

export type DesktopGrid = {
  /** The whole monitor in DIPs. */
  screen: { width: number; height: number };
  /** Where the first cell starts, in DIPs from the monitor's top left. */
  origin: [number, number];
  /** The stretched cell size and the gap, in DIPs. */
  cell: [number, number];
  gap: number;
  columns: number;
  rows: number;
};

/** One axis: how many whole cells fit, and their stretched size. */
function axis(length: number, nominal: number, gap: number): [count: number, size: number] {
  const count = Math.max(1, Math.floor((length + gap) / (nominal + gap)));
  return [count, (length - (count - 1) * gap) / count];
}

export function desktopGrid(monitor: MonitorInfo, grid: Grid, iconCell: Cells): DesktopGrid {
  const k = monitor.scale > 0 ? monitor.scale : 1;
  const work = { x: monitor.work.x / k, y: monitor.work.y / k, width: monitor.work.width / k, height: monitor.work.height / k };
  const nominal: Cells = grid.cell > 0 ? [grid.cell, grid.cell] : iconCell;
  const [columns, cellWidth] = axis(work.width - 2 * grid.margin, nominal[0], grid.gap);
  const [rows, cellHeight] = axis(work.height - 2 * grid.margin, nominal[1], grid.gap);
  return {
    screen: { width: monitor.width / k, height: monitor.height / k },
    origin: [work.x + grid.margin, work.y + grid.margin],
    cell: [cellWidth, cellHeight],
    gap: grid.gap,
    columns,
    rows,
  };
}

/** A widget's box in DIPs: `[c, r]` sits at `origin + [c, r] × (cell + gap)`. */
export function boxOf(g: DesktopGrid, [column, row]: Cells, [width, height]: Cells): Box {
  return {
    x: g.origin[0] + column * (g.cell[0] + g.gap),
    y: g.origin[1] + row * (g.cell[1] + g.gap),
    width: width * g.cell[0] + (width - 1) * g.gap,
    height: height * g.cell[1] + (height - 1) * g.gap,
  };
}

/** The cell nearest a point in DIPs (the top left of a dragged widget). */
export function cellAt(g: DesktopGrid, x: number, y: number): Cells {
  return [Math.round((x - g.origin[0]) / (g.cell[0] + g.gap)), Math.round((y - g.origin[1]) / (g.cell[1] + g.gap))];
}

/** The size the host shows: the saved one clamped to the manifest, or the default. */
export function sizeOf(item: PlacedWidget, widget: WidgetInfo | undefined): Cells {
  const fallback: Cells = widget?.size.default ?? [2, 2];
  if (!widget) return item.size ?? fallback;
  if (!widget.resizable || !item.size) return widget.size.default;
  const max = widget.size.max ?? [Infinity, Infinity];
  return [clamp(item.size[0], widget.size.min[0], max[0]), clamp(item.size[1], widget.size.min[1], max[1])];
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** A cell clamped so a widget of `size` stays on the grid. */
export function clampCell(g: DesktopGrid, [column, row]: Cells, [width, height]: Cells): Cells {
  return [clamp(column, 0, Math.max(0, g.columns - width)), clamp(row, 0, Math.max(0, g.rows - height))];
}

export type Placement = { instance: string; cell: Cells; size: Cells };

const overlaps = (a: Placement, b: Placement) =>
  a.cell[0] < b.cell[0] + b.size[0] && b.cell[0] < a.cell[0] + a.size[0] && a.cell[1] < b.cell[1] + b.size[1] && b.cell[1] < a.cell[1] + a.size[1];

/** Whether `candidate` fits on the grid without covering another widget. */
export function fits(g: DesktopGrid, candidate: Placement, others: readonly Placement[]): boolean {
  const [column, row] = candidate.cell;
  const [width, height] = candidate.size;
  const inside = column >= 0 && row >= 0 && column + width <= g.columns && row + height <= g.rows;
  return inside && others.every((other) => other.instance === candidate.instance || !overlaps(candidate, other));
}

/** The first free cell for a widget of `size`, row by row from the top left; null when the grid is full. */
export function firstFree(g: DesktopGrid, size: Cells, others: readonly Placement[]): Cells | null {
  for (let row = 0; row + size[1] <= g.rows; row++) {
    for (let column = 0; column + size[0] <= g.columns; column++) {
      if (fits(g, { instance: "", cell: [column, row], size }, others)) return [column, row];
    }
  }
  return null;
}

/** `\\.\DISPLAY2` -> `Display 2`. */
export function displayName(device: string | null): string {
  const number = device && /DISPLAY(\d+)/i.exec(device)?.[1];
  return number ? `Display ${number}` : (device ?? "Main display");
}

/** A fresh instance id, `<widget>-<n>`, as the host names them. */
export function nextInstanceId(widget: string, taken: readonly string[]): string {
  const base = widget.replace(/[^a-z0-9-]/g, "-");
  for (let n = 1; ; n++) if (!taken.includes(`${base}-${n}`)) return `${base}-${n}`;
}
