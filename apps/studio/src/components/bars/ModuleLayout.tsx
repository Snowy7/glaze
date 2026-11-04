import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { BREAK, BUILTIN_MODULES, type ModuleRef } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";

/** Module lists by zone: the top bar's `left`, `center`, `right`; a taskbar dock's `left`, `right`. */
export type Zones = Record<string, ModuleRef[]>;
export type ZoneInfo = { zone: string; label: string; hint: string };

const AVAILABLE = "available";
type Drag = { ref: ModuleRef; zone: string; index: number };
type Target = { zone: string; index: number };

/**
 * `zones` with the item at (`from`, `index`) taken out and put at `to`/`at`. From the available pool
 * (`from` is "available") a module is placed, and a break is copied; dropped on the pool, an item
 * is removed.
 */
export function moveItem(zones: Zones, drag: Drag, to: string, at: number): Zones {
  const next: Zones = Object.fromEntries(Object.entries(zones).map(([zone, list]) => [zone, [...list]]));
  let target = at;
  if (drag.zone !== AVAILABLE) {
    next[drag.zone]?.splice(drag.index, 1);
    // Taking the item out of this same list first shifts everything after it.
    if (drag.zone === to && drag.index < at) target = at - 1;
  }
  if (to !== AVAILABLE) next[to]?.splice(target, 0, drag.ref);
  return next;
}

/** Every module a layout can offer that isn't placed yet: built-ins, then the custom ones defined. */
function unplaced(zones: Zones, customs: readonly ModuleRef[]): ModuleRef[] {
  const placed = new Set(Object.values(zones).flat());
  return [...BUILTIN_MODULES, ...customs].filter((ref) => !placed.has(ref));
}

type Props = {
  zones: Zones;
  info: readonly ZoneInfo[];
  /** `custom:<id>` of every custom module the file defines. */
  customs: readonly ModuleRef[];
  selected: ModuleRef | null;
  onSelect: (ref: ModuleRef | null) => void;
  onChange: (next: Zones) => void;
};

/**
 * Lists of modules, separators included. Drag a module between them, or focus one and use the
 * arrow keys (left/right reorder, up/down change list). Clicking a module selects it, to edit its
 * options. The pool holds what isn't placed; its break chip adds a new one wherever it is dropped.
 */
export function ModuleLayout({ zones, info, customs, selected, onSelect, onChange }: Props) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const refocus = useRef<string | null>(null);
  const order = [...info.map((z) => z.zone), AVAILABLE];
  const pool = [BREAK, ...unplaced(zones, customs)];

  // A module that changed list was re-created, so give it focus back.
  useEffect(() => {
    if (!refocus.current) return;
    document.querySelector<HTMLElement>(`[data-chip="${CSS.escape(refocus.current)}"]`)?.focus();
    refocus.current = null;
  }, [zones]);

  const listOf = (zone: string): ModuleRef[] => (zone === AVAILABLE ? pool : (zones[zone] ?? []));

  function endDrag() {
    setDrag(null);
    setTarget(null);
  }

  function onKey(event: KeyboardEvent, ref: ModuleRef, zone: string, index: number) {
    const list = listOf(zone);
    let next: Zones | null = null;
    let landed: Target | null = null;
    if (event.key === "ArrowLeft" && zone !== AVAILABLE && index > 0) {
      next = moveItem(zones, { ref, zone, index }, zone, index - 1);
      landed = { zone, index: index - 1 };
    } else if (event.key === "ArrowRight" && zone !== AVAILABLE && index < list.length - 1) {
      next = moveItem(zones, { ref, zone, index }, zone, index + 2);
      landed = { zone, index: index + 1 };
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const to = order[order.indexOf(zone) + (event.key === "ArrowDown" ? 1 : -1)];
      if (to) {
        next = moveItem(zones, { ref, zone, index }, to, to === AVAILABLE ? 0 : listOf(to).length);
        landed = { zone: to, index: to === AVAILABLE ? 0 : listOf(to).length };
      }
    }
    if (!next || !landed) return;
    event.preventDefault();
    refocus.current = `${landed.zone}:${landed.index}`;
    onChange(next);
  }

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
          if (drag && target) onChange(moveItem(zones, drag, target.zone, target.index));
          endDrag();
        }}
        className={`rounded-xl p-2.5 ring-1 transition-colors ${zone === AVAILABLE ? "border border-dashed border-line-strong ring-0" : "bg-canvas ring-line"} ${hovered ? "ring-2! ring-line-hi!" : ""}`}
      >
        <h3 className="mb-2 flex items-baseline gap-2 px-1 text-[11px] font-semibold tracking-[0.1em] text-ink-3 uppercase" title={hint}>
          {label}
          {zone !== AVAILABLE && <span className="font-normal tracking-normal normal-case">{list.length}</span>}
        </h3>
        <ul className="flex min-h-9 flex-wrap items-center gap-y-1.5">
          {list.map((ref, index) => (
            <li
              key={`${ref}:${index}`}
              className="flex items-center px-[3px]"
              onDragOver={(e) => {
                if (!drag) return;
                e.preventDefault();
                e.stopPropagation();
                const box = e.currentTarget.getBoundingClientRect();
                setTarget({ zone, index: index + (e.clientX > box.left + box.width / 2 ? 1 : 0) });
              }}
            >
              {hovered?.index === index && zone !== AVAILABLE && <DropMark />}
              <Chip
                chip={`${zone}:${index}`}
                moduleRef={ref}
                selected={selected === ref && ref !== BREAK}
                dragging={drag?.zone === zone && drag.index === index}
                onSelect={() => onSelect(ref === BREAK ? null : ref)}
                onKeyDown={(e) => onKey(e, ref, zone, index)}
                onDragStart={() => setDrag({ ref, zone, index })}
                onDragEnd={endDrag}
              />
            </li>
          ))}
          {hovered && zone !== AVAILABLE && hovered.index >= list.length && <DropMark />}
          {list.length === 0 && !hovered && <li className="px-1 text-xs text-ink-3">Empty</li>}
        </ul>
      </section>
    );
  };

  const placedZones = info.slice();
  return (
    <div>
      <div className={`grid gap-3 ${placedZones.length === 3 ? "grid-cols-3" : "grid-cols-2"}`}>{placedZones.map(renderZone)}</div>
      <div className="mt-3">{renderZone({ zone: AVAILABLE, label: "Available", hint: "Not shown. Drop a module here to remove it. The break chip adds a new separator." })}</div>
      <p className="mt-3 text-xs text-ink-3">Drag modules between the lists, or focus one and use the arrow keys: left and right reorder, up and down change list. Click a module to edit its options.</p>
    </div>
  );
}

function DropMark() {
  return <span aria-hidden className="mr-[3px] h-6 w-0.5 shrink-0 rounded-full bg-ink" />;
}

type ChipProps = {
  chip: string;
  moduleRef: ModuleRef;
  selected: boolean;
  dragging: boolean;
  onSelect: () => void;
  onKeyDown: (event: KeyboardEvent) => void;
  onDragStart: () => void;
  onDragEnd: () => void;
};

function Chip({ chip, moduleRef, selected, dragging, onSelect, onKeyDown, onDragStart, onDragEnd }: ChipProps) {
  const { label, hint, icon: Icon } = describeModule(moduleRef);
  const isBreak = moduleRef === BREAK;
  return (
    <button
      type="button"
      draggable
      data-chip={chip}
      title={hint}
      aria-pressed={isBreak ? undefined : selected}
      onClick={onSelect}
      onKeyDown={onKeyDown}
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", moduleRef);
        onDragStart();
      }}
      onDragEnd={onDragEnd}
      className={`flex h-8 cursor-grab items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium ring-1 transition active:cursor-grabbing ${
        selected ? "bg-solid text-solid-ink ring-transparent" : "bg-surface-hi text-ink ring-line-strong hover:ring-line-hi"
      } ${dragging ? "opacity-35" : ""} ${isBreak ? "border-dashed" : ""}`}
    >
      {isBreak ? <span className="font-mono text-sm leading-none">|</span> : <Icon className={`size-3.5 ${selected ? "" : "text-ink-2"}`} aria-hidden />}
      {isBreak ? "Break" : label}
    </button>
  );
}
