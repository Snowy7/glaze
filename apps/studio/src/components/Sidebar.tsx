import { Blocks, LayoutGrid, PanelTop, Pause, Plus, SlidersHorizontal, type LucideIcon } from "lucide-react";

import { useStudio } from "../lib/studio";
import { StatusDot } from "./ui";

export type View = "gallery" | "widgets" | "bars" | "new" | "settings";

const ITEMS: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "gallery", label: "Gallery", icon: LayoutGrid },
  { view: "widgets", label: "Widgets", icon: Blocks },
  { view: "bars", label: "Bars", icon: PanelTop },
  { view: "new", label: "New theme", icon: Plus },
  { view: "settings", label: "Settings", icon: SlidersHorizontal },
];

export function Sidebar({ view, onNavigate }: { view: View; onNavigate: (view: View) => void }) {
  return (
    <aside className="flex w-56 shrink-0 flex-col bg-sidebar px-3 pt-5 pb-4">
      <div className="mb-7 flex items-center gap-2.5 px-2">
        <Mark />
        <div className="leading-none">
          <div className="font-display text-[17px] font-semibold tracking-tight text-ink">mypaper</div>
          <div className="mt-1 text-[11px] font-medium tracking-[0.14em] text-ink-3 uppercase">Studio</div>
        </div>
      </div>

      <nav aria-label="Main" className="flex flex-col gap-1">
        {ITEMS.map(({ view: target, label, icon: Icon }) => {
          const current = view === target;
          return (
            <button
              key={target}
              type="button"
              aria-current={current ? "page" : undefined}
              onClick={() => onNavigate(target)}
              className={`flex h-10 cursor-default items-center gap-3 rounded-lg px-3 text-[13.5px] font-medium transition-colors duration-150 ${
                current ? "bg-solid text-solid-ink" : "text-ink-2 hover:bg-surface-hi hover:text-ink"
              }`}
            >
              <Icon className="size-[18px]" aria-hidden />
              {label}
            </button>
          );
        })}
      </nav>

      <EngineChip onOpen={() => onNavigate("settings")} />
    </aside>
  );
}

function EngineChip({ onOpen }: { onOpen: () => void }) {
  const { engine, config } = useStudio();
  const state = !engine ? "unknown" : !engine.running ? "stopped" : config?.paused ? "paused" : "running";
  const label = {
    unknown: "Checking engine",
    stopped: "Engine stopped",
    paused: "Paused",
    running: "Wallpaper running",
  }[state];

  return (
    <button
      type="button"
      onClick={onOpen}
      title="Engine settings"
      className="mt-auto flex h-10 cursor-default items-center gap-3 rounded-lg px-3 text-left text-[12.5px] text-ink-2 transition-colors duration-150 hover:bg-surface-hi hover:text-ink"
    >
      {state === "paused" ? (
        <Pause className="size-2.5 shrink-0 fill-current" aria-hidden />
      ) : (
        <StatusDot state={state === "running" ? "on" : state === "stopped" ? "off" : "unknown"} />
      )}
      {label}
    </button>
  );
}

/** Two aurora ribbons, echoing the app icon, in grays. */
/** The mypaper mark (assets/logo/mark-small.svg), drawn in the theme's inverse colors. */
function Mark() {
  const sheet = "M6.5 8H20.5L26 13.5V22.5Q26 24 24.5 24H6.5Q5 24 5 22.5V9.5Q5 8 6.5 8Z";
  return (
    <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
      <defs>
        <clipPath id="mark-sheet">
          <path d={sheet} />
        </clipPath>
      </defs>
      <rect width="32" height="32" rx="7.5" className="fill-solid" />
      <path d={sheet} className="fill-solid-ink" />
      <g clipPath="url(#mark-sheet)" className="fill-solid">
        <circle cx="13" cy="13" r="2.2" />
        <path d="M5 20C9 16.5 13 16.5 16.5 18.5S22.5 20.5 26 17V24H5Z" />
      </g>
      <path d="M20.5 8V12.2Q20.5 13.5 21.8 13.5H26Z" className="fill-solid" opacity="0.55" />
    </svg>
  );
}
