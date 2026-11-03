import { LayoutGrid, Plus, SlidersHorizontal, type LucideIcon } from "lucide-react";

import { useStudio } from "../lib/studio";

export type View = "gallery" | "new" | "settings";

const ITEMS: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "gallery", label: "Gallery", icon: LayoutGrid },
  { view: "new", label: "New theme", icon: Plus },
  { view: "settings", label: "Settings", icon: SlidersHorizontal },
];

export function Sidebar({ view, onNavigate }: { view: View; onNavigate: (view: View) => void }) {
  return (
    <aside className="flex w-56 shrink-0 flex-col px-3 pt-5 pb-4">
      <div className="mb-7 flex items-center gap-2.5 px-2">
        <Mark />
        <div className="leading-none">
          <div className="font-display text-[17px] font-semibold tracking-tight">mypaper</div>
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
              className={`group relative flex h-10 cursor-default items-center gap-3 rounded-lg px-3 text-[13.5px] font-medium transition-colors duration-150 ${
                current ? "bg-surface-hi text-ink" : "text-ink-2 hover:bg-surface-hi/60 hover:text-ink"
              }`}
            >
              <span
                aria-hidden
                className={`absolute top-2.5 bottom-2.5 left-0 w-[3px] rounded-full bg-accent transition-opacity duration-150 ${
                  current ? "opacity-100" : "opacity-0"
                }`}
              />
              <Icon className={`size-[18px] ${current ? "text-accent" : ""}`} aria-hidden />
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
  const { label, dot } = {
    unknown: { label: "Checking engine", dot: "bg-ink-3" },
    stopped: { label: "Engine stopped", dot: "bg-ink-3" },
    paused: { label: "Paused", dot: "bg-warn" },
    running: { label: "Wallpaper running", dot: "bg-ok" },
  }[state];

  return (
    <button
      type="button"
      onClick={onOpen}
      title="Engine settings"
      className="mt-auto flex h-10 cursor-default items-center gap-3 rounded-lg px-3 text-left text-[12.5px] text-ink-2 transition-colors duration-150 hover:bg-surface-hi/60 hover:text-ink"
    >
      <span className="relative flex size-2.5 shrink-0" aria-hidden>
        {state === "running" && <span className={`absolute inset-0 rounded-full ${dot} animate-[pulse-ring_2.4s_ease-out_infinite]`} />}
        <span className={`relative size-2.5 rounded-full ${dot}`} />
      </span>
      {label}
    </button>
  );
}

/** Two aurora ribbons, echoing the app icon. */
function Mark() {
  return (
    <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
      <rect width="32" height="32" rx="9" fill="oklch(0.2 0.02 270)" />
      <path d="M3 21c4-7 7-7 10-3s6 3 10-4 6-2 6-2" fill="none" stroke="oklch(0.85 0.12 78)" strokeWidth="2.6" strokeLinecap="round" />
      <path d="M3 15c4-6 7-6 10-2.5s6 2.5 9.5-3 6.5-1 6.5-1" fill="none" stroke="oklch(0.8 0.1 190)" strokeWidth="2.2" strokeLinecap="round" opacity="0.85" />
    </svg>
  );
}
