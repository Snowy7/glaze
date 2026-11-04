import { Blocks, LayoutGrid, PanelTop, Pause, Plus, Settings, type LucideIcon } from "lucide-react";

import { useStudio } from "../lib/studio";
import { StatusDot } from "./ui";

export type View = "gallery" | "widgets" | "bars" | "new" | "settings";

type Item = { view: View; label: string; icon: LucideIcon };

const SECTIONS: readonly { title: string; items: readonly Item[] }[] = [
  {
    title: "Themes",
    items: [
      { view: "gallery", label: "Gallery", icon: LayoutGrid },
      { view: "new", label: "New theme", icon: Plus },
    ],
  },
  {
    title: "Desktop",
    items: [
      { view: "widgets", label: "Widgets", icon: Blocks },
      { view: "bars", label: "Bars", icon: PanelTop },
    ],
  },
];

const SETTINGS: Item = { view: "settings", label: "Settings", icon: Settings };

/** The app's navigation. Below 1024 px wide it folds into an icon rail (labels stay for screen readers and tooltips). */
export function Sidebar({ view, onNavigate }: { view: View; onNavigate: (view: View) => void }) {
  const link = (item: Item) => <NavLink key={item.view} item={item} current={view === item.view} onNavigate={onNavigate} />;
  return (
    <aside className="flex w-[3.75rem] shrink-0 flex-col bg-sidebar px-2 pt-4 pb-3 lg:w-56 lg:px-3">
      <div className="mb-6 flex items-center gap-2.5 max-lg:justify-center lg:px-1.5">
        <Mark />
        <div className="leading-none max-lg:hidden">
          <div className="font-display text-[15px] font-semibold tracking-tight text-ink">mypaper</div>
          <div className="mt-1 text-[11px] font-medium text-ink-3">Studio</div>
        </div>
      </div>

      <nav aria-label="Main" className="flex min-h-0 flex-1 flex-col">
        {SECTIONS.map(({ title, items }) => (
          <div key={title} className="mb-4 max-lg:mb-3 max-lg:border-b max-lg:border-line max-lg:pb-3 max-lg:last:border-0">
            <div className="mb-1 px-2.5 text-[11.5px] font-medium text-ink-4 max-lg:sr-only">{title}</div>
            <div className="flex flex-col gap-0.5">{items.map(link)}</div>
          </div>
        ))}
        <div className="mt-auto flex flex-col gap-0.5">{link(SETTINGS)}</div>
      </nav>

      <EngineChip onOpen={() => onNavigate("settings")} />
    </aside>
  );
}

function NavLink({ item: { view, label, icon: Icon }, current, onNavigate }: { item: Item; current: boolean; onNavigate: (view: View) => void }) {
  return (
    <button
      type="button"
      aria-current={current ? "page" : undefined}
      title={label}
      onClick={() => onNavigate(view)}
      className={`flex h-9 cursor-default items-center gap-3 rounded-lg px-2.5 text-[13px] font-medium transition-colors duration-150 max-lg:justify-center max-lg:px-0 ${
        current ? "bg-surface-top text-ink shadow-[inset_0_0_0_1px_var(--line-strong)]" : "text-ink-3 hover:bg-surface-hi hover:text-ink"
      }`}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="max-lg:sr-only">{label}</span>
    </button>
  );
}

/** What the engine is doing, and the theme on screen. Opens Settings. */
function EngineChip({ onOpen }: { onOpen: () => void }) {
  const { engine, config, themes } = useStudio();
  const state = !engine ? "unknown" : !engine.running ? "stopped" : config?.paused ? "paused" : "running";
  const label = {
    unknown: "Checking engine",
    stopped: "Engine stopped",
    paused: "Wallpaper paused",
    running: "Wallpaper running",
  }[state];
  const active = themes.find((t) => t.active);

  return (
    <button
      type="button"
      onClick={onOpen}
      title={`${label}. Engine settings`}
      className="mt-3 flex min-h-11 cursor-default items-center gap-2.5 rounded-lg bg-surface px-3 py-2 text-left ring-1 ring-line-strong transition-colors duration-150 hover:bg-surface-hi hover:ring-line-hi max-lg:justify-center max-lg:px-0"
    >
      {state === "paused" ? (
        <Pause className="size-2.5 shrink-0 fill-current text-ink" aria-hidden />
      ) : (
        <StatusDot state={state === "running" ? "on" : state === "stopped" ? "off" : "unknown"} />
      )}
      <span className="min-w-0 max-lg:sr-only">
        <span className="block truncate text-[12.5px] font-medium text-ink">{label}</span>
        {active && state !== "stopped" && <span className="block truncate text-[11.5px] text-ink-3">{active.name}</span>}
      </span>
    </button>
  );
}

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
