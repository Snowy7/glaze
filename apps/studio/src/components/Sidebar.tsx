import { Pause, Play, Search } from "lucide-react";

import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { PAGES } from "../lib/pages";
import { Keycap, StatusDot } from "./ui";

/** The app's navigation: search, the six places, and what mypaper is doing. Below 1024 px it folds into an icon rail. */
export function Sidebar({ onSearch }: { onSearch: () => void }) {
  const { route, go } = useNav();
  const main = PAGES.filter((p) => p.page !== "settings");
  const settings = PAGES.find((p) => p.page === "settings");
  const link = (item: (typeof PAGES)[number]) => {
    const current = route.page === item.page;
    return (
      <button
        key={item.page}
        type="button"
        aria-current={current ? "page" : undefined}
        title={item.label}
        onClick={() => go(item.route)}
        className={`flex h-9 cursor-default items-center gap-3 rounded-lg px-2.5 text-[13px] font-medium transition-colors duration-150 max-lg:justify-center max-lg:px-0 ${
          current ? "bg-surface-top text-ink shadow-[inset_0_0_0_1px_var(--line-strong)]" : "text-ink-3 hover:bg-surface-hi hover:text-ink"
        }`}
      >
        <item.icon className="size-4 shrink-0" aria-hidden />
        <span className="max-lg:sr-only">{item.label}</span>
      </button>
    );
  };

  return (
    <aside className="flex w-[3.75rem] shrink-0 flex-col bg-sidebar px-2 pt-4 pb-3 lg:w-56 lg:px-3">
      <div className="mb-5 flex items-center gap-2.5 max-lg:justify-center lg:px-1.5">
        <Mark />
        <div className="leading-none max-lg:hidden">
          <div className="font-display text-[15px] font-semibold tracking-tight text-ink">mypaper</div>
          <div className="mt-1 text-[11px] font-medium text-ink-3">Studio</div>
        </div>
      </div>

      <button
        type="button"
        onClick={onSearch}
        title="Search"
        className="mb-4 flex h-9 cursor-default items-center gap-2.5 rounded-lg bg-canvas px-2.5 text-[13px] text-ink-3 ring-1 ring-line-strong transition-colors ring-inset hover:text-ink-2 hover:ring-line-hi max-lg:justify-center max-lg:px-0"
      >
        <Search className="size-4 shrink-0" aria-hidden />
        <span className="flex-1 text-left max-lg:sr-only">Search</span>
        <span className="flex gap-0.5 max-lg:hidden" aria-hidden>
          <Keycap>Ctrl</Keycap>
          <Keycap>K</Keycap>
        </span>
      </button>

      <nav aria-label="Main" className="flex min-h-0 flex-1 flex-col gap-0.5">
        {main.map(link)}
        <div className="mt-auto flex flex-col gap-0.5">{settings && link(settings)}</div>
      </nav>

      <EngineChip />
    </aside>
  );
}

/** What mypaper is doing and the theme on screen; pause and resume in one click, Settings in another. */
function EngineChip() {
  const { engine, config, themes, updateConfig } = useStudio();
  const { go } = useNav();
  const state = !engine ? "unknown" : !engine.running ? "stopped" : config?.paused ? "paused" : "running";
  const label = { unknown: "Checking", stopped: "mypaper is off", paused: "Paused", running: "Playing" }[state];
  const active = themes.find((t) => t.active);
  const canPause = state === "running" || state === "paused";

  return (
    <div className="mt-3 flex min-h-11 items-center gap-1 rounded-lg bg-surface py-1 pr-1 pl-3 ring-1 ring-line-strong max-lg:justify-center max-lg:p-1">
      <button type="button" onClick={() => go({ page: "settings" })} title={`${label}. Settings`} className="flex min-w-0 flex-1 cursor-default items-center gap-2.5 text-left max-lg:hidden">
        {state === "paused" ? <Pause className="size-2.5 shrink-0 fill-current text-ink" aria-hidden /> : <StatusDot state={state === "running" ? "on" : state === "stopped" ? "off" : "unknown"} />}
        <span className="min-w-0">
          <span className="block truncate text-[12px] font-medium text-ink">{label}</span>
          {active && state !== "stopped" && <span className="block truncate text-[12px] text-ink-3">{active.name}</span>}
        </span>
      </button>
      {canPause && config && (
        <button
          type="button"
          aria-label={config.paused ? "Resume the wallpaper" : "Pause the wallpaper"}
          title={config.paused ? "Resume" : "Pause"}
          onClick={() => void updateConfig({ paused: !config.paused }, { label: config.paused ? "Resume wallpaper" : "Pause wallpaper" })}
          className="grid size-8 shrink-0 cursor-default place-items-center rounded-md text-ink-2 transition-colors hover:bg-surface-hi hover:text-ink"
        >
          {config.paused ? <Play className="size-3.5" aria-hidden /> : <Pause className="size-3.5" aria-hidden />}
        </button>
      )}
    </div>
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
