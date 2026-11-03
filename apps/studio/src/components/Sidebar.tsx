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
function Mark() {
  return (
    <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
      <rect width="32" height="32" rx="9" className="fill-solid" />
      <path d="M3 21c4-7 7-7 10-3s6 3 10-4 6-2 6-2" fill="none" className="stroke-solid-ink" strokeWidth="2.6" strokeLinecap="round" />
      <path d="M3 15c4-6 7-6 10-2.5s6 2.5 9.5-3 6.5-1 6.5-1" fill="none" className="stroke-solid-ink" strokeWidth="2.2" strokeLinecap="round" opacity="0.5" />
    </svg>
  );
}
