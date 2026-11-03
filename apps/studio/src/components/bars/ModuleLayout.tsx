import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { BAR_MODULES, type BarModule, type BarsConfig } from "../../lib/api";
import { MODULE_INFO } from "../../lib/modules";

export type Placement = Pick<BarsConfig["top"], "left" | "center" | "right">;

type Zone = keyof Placement | "available";
type ZoneInfo = { zone: Zone; label: string; hint: string };

const PLACED: readonly ZoneInfo[] = [
  { zone: "left", label: "Left", hint: "Starts at the screen's left edge" },
  { zone: "center", label: "Center", hint: "Middle of the bar" },
  { zone: "right", label: "Right", hint: "Ends at the screen's right edge" },
];
const AVAILABLE: ZoneInfo = { zone: "available", label: "Available", hint: "Not shown. Drop a module here to remove it" };
const ORDER: readonly Zone[] = [...PLACED.map((z) => z.zone), "available"];

type Drag = { module: BarModule };
type Target = { zone: Zone; index: number };

/** The modules not placed anywhere, in their documented order. */
function unplaced(placement: Placement): BarModule[] {
  const placed = new Set([...placement.left, ...placement.center, ...placement.right]);
  return BAR_MODULES.filter((module) => !placed.has(module));
}

/** `placement` with `module` taken out of wherever it is and put at `index` of `to`. */
export function moveModule(placement: Placement, module: BarModule, to: Zone, index: number): Placement {
  const without = (list: BarModule[]) => list.filter((m) => m !== module);
  const next: Placement = { left: without(placement.left), center: without(placement.center), right: without(placement.right) };
  if (to === "available") return next;
  // Taking the module out of this same list first shifts everything after it.
  const before = placement[to].indexOf(module);
  const at = before !== -1 && before < index ? index - 1 : index;
  next[to] = next[to].toSpliced(at, 0, module);
  return next;
}

/**
 * Left, Center, Right and Available lists of top-bar modules. Drag a module
 * between them, or focus one and use the arrow keys (left/right reorder,
 * up/down change list).
 */
export function ModuleLayout({ value, onChange }: { value: Placement; onChange: (next: Placement) => void }) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const refocus = useRef<BarModule | null>(null);

  // A module that changed list was re-created, so give it focus back.
  useEffect(() => {
    if (!refocus.current) return;
    document.querySelector<HTMLElement>(`[data-module="${refocus.current}"]`)?.focus();
    refocus.current = null;
  }, [value]);

  const listOf = (zone: Zone): BarModule[] => (zone === "available" ? unplaced(value) : value[zone]);

  function endDrag() {
    setDrag(null);
    setTarget(null);
  }

  function onKey(event: KeyboardEvent, module: BarModule, zone: Zone, index: number) {
    const list = listOf(zone);
    let next: Placement | null = null;
    if (event.key === "ArrowLeft" && zone !== "available" && index > 0) next = moveModule(value, module, zone, index - 1);
    else if (event.key === "ArrowRight" && zone !== "available" && index < list.length - 1) next = moveModule(value, module, zone, index + 2);
    else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const to = ORDER[ORDER.indexOf(zone) + (event.key === "ArrowDown" ? 1 : -1)];
      if (to) next = moveModule(value, module, to, listOf(to).length);
    }
    if (!next) return;
    event.preventDefault();
    refocus.current = module;
    onChange(next);
  }

  // A plain function, not a component: the chips must keep their identity while one is dragged.
  const renderZone = ({ zone, label, hint }: ZoneInfo) => {
    const list = listOf(zone);
    const hovered = drag !== null && target?.zone === zone ? target : null;
    return (
      <section
        key={zone}
        aria-label={`${label} modules`}
        onDragOver={(e) => {
          if (!drag) return;
          e.preventDefault();
          setTarget({ zone, index: list.length });
        }}
        onDrop={(e) => {
          e.preventDefault();
          if (drag && target) onChange(moveModule(value, drag.module, target.zone, target.index));
          endDrag();
        }}
        className={`rounded-xl p-2.5 ring-1 transition-colors ${
          zone === "available" ? "border border-dashed border-line-strong ring-0" : "bg-canvas ring-line"
        } ${hovered ? "ring-2! ring-line-hi!" : ""}`}
      >
        <h3 className="mb-2 flex items-baseline gap-2 px-1 text-[11px] font-semibold tracking-[0.1em] text-ink-3 uppercase" title={hint}>
          {label}
          <span className="font-normal tracking-normal normal-case">{list.length}</span>
        </h3>
        <ul className="flex min-h-9 flex-wrap items-center gap-y-1.5">
          {list.map((module, index) => (
            <li
              key={module}
              className="flex items-center px-[3px]"
              onDragOver={(e) => {
                if (!drag) return;
                e.preventDefault();
                e.stopPropagation();
                const box = e.currentTarget.getBoundingClientRect();
                setTarget({ zone, index: index + (e.clientX > box.left + box.width / 2 ? 1 : 0) });
              }}
            >
              {hovered?.index === index && <DropMark />}
              <Chip
                module={module}
                dragging={drag?.module === module}
                onKeyDown={(e) => onKey(e, module, zone, index)}
                onDragStart={() => setDrag({ module })}
                onDragEnd={endDrag}
              />
            </li>
          ))}
          {hovered && hovered.index >= list.length && <DropMark />}
          {list.length === 0 && !hovered && <li className="px-1 text-xs text-ink-3">{zone === "available" ? "Every module is in use" : "Empty"}</li>}
        </ul>
      </section>
    );
  };

  return (
    <div>
      <div className="grid grid-cols-3 gap-3">{PLACED.map(renderZone)}</div>
      <div className="mt-3">{renderZone(AVAILABLE)}</div>
      <p className="mt-3 text-xs text-ink-3">Drag modules between the lists. With a module focused, use the arrow keys: left and right reorder, up and down change list.</p>
    </div>
  );
}

function DropMark() {
  return <span aria-hidden className="mr-[3px] h-6 w-0.5 shrink-0 rounded-full bg-ink" />;
}

type ChipProps = {
  module: BarModule;
  dragging: boolean;
  onKeyDown: (event: KeyboardEvent) => void;
  onDragStart: () => void;
  onDragEnd: () => void;
};

function Chip({ module, dragging, onKeyDown, onDragStart, onDragEnd }: ChipProps) {
  const { label, hint, icon: Icon } = MODULE_INFO[module];
  return (
    <button
      type="button"
      draggable
      data-module={module}
      title={hint}
      onKeyDown={onKeyDown}
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", module);
        onDragStart();
      }}
      onDragEnd={onDragEnd}
      className={`flex h-8 cursor-grab items-center gap-1.5 rounded-lg bg-surface-hi px-2.5 text-[12.5px] font-medium text-ink ring-1 ring-line-strong transition hover:ring-line-hi active:cursor-grabbing ${
        dragging ? "opacity-35" : ""
      }`}
    >
      <Icon className="size-3.5 text-ink-2" aria-hidden />
      {label}
    </button>
  );
}
