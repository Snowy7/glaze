import { CornerDownLeft, Image as ImageIcon, MousePointerClick, Pause, Play, Plus, Power, Search, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { api, errorText } from "../lib/api";
import { useNav, type Route } from "../lib/nav";
import { PAGES } from "../lib/pages";
import { matches, SETTINGS } from "../lib/settingsIndex";
import { useStudio } from "../lib/studio";

type Item = { id: string; title: string; section: string; icon: LucideIcon; run: () => void; keywords?: string };
type Group = { title: string; items: Item[] };

/** Search across every page, setting, theme and action. Opens with Ctrl+K or the sidebar's search field. */
export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const { go } = useNav();
  const { themes, applyTheme, config, updateConfig, engine, startEngine, stopEngine, notify } = useStudio();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (open && !element.open) {
      setQuery("");
      setActive(0);
      element.showModal();
    }
    if (!open && element.open) element.close();
  }, [open]);

  const groups = useMemo<Group[]>(() => {
    const to = (route: Route) => () => go(route);
    const pages: Item[] = PAGES.map(({ page, route, label, icon }) => ({ id: `page:${page}`, title: label, section: "Page", icon, run: to(route) }));
    const actions: Item[] = [
      ...(config
        ? [{ id: "pause", title: config.paused ? "Resume the wallpaper" : "Pause the wallpaper", section: "Wallpaper", icon: config.paused ? Play : Pause, keywords: "stop play", run: () => void updateConfig({ paused: !config.paused }, { label: config.paused ? "Resume wallpaper" : "Pause wallpaper" }) }]
        : []),
      { id: "new-theme", title: "New theme", section: "Themes", icon: Plus, keywords: "create make import picture video", run: to({ page: "themes", view: "new" }) },
      { id: "add-widget", title: "Add a widget", section: "Widgets", icon: Plus, keywords: "new place", run: to({ page: "widgets", adding: true }) },
      { id: "arrange", title: "Arrange widgets on the desktop", section: "Widgets", icon: MousePointerClick, keywords: "edit mode move", run: () => void api.toggleWidgetEdit().catch((e: unknown) => notify(errorText(e), "error")) },
      ...(engine ? [{ id: "engine", title: engine.running ? "Stop mypaper" : "Start mypaper", section: "Settings", icon: Power, keywords: "engine quit run", run: () => void (engine.running ? stopEngine() : startEngine()) }] : []),
    ];
    const settings: Item[] = SETTINGS.map((entry) => ({ id: entry.id, title: entry.title, section: entry.section, icon: SlidersHorizontal, keywords: entry.keywords, run: to({ ...entry.route, focus: entry.id }) }));
    const themeItems: Item[] = themes
      .filter((t) => !t.active && t.error === null)
      .map((t) => ({ id: `theme:${t.id}`, title: `Use ${t.name}`, section: "Themes", icon: ImageIcon, keywords: `theme wallpaper apply ${t.kind ?? ""}`, run: () => void applyTheme(t) }));

    const needle = query.trim();
    if (needle === "") return [
      { title: "Go to", items: pages },
      { title: "Actions", items: actions },
    ];
    const filter = (items: Item[]) => items.filter((item) => matches(needle, item.title, item.section, item.keywords));
    return [
      { title: "Settings", items: filter(settings).slice(0, 12) },
      { title: "Actions", items: filter(actions) },
      { title: "Themes", items: filter(themeItems).slice(0, 6) },
      { title: "Go to", items: filter(pages) },
    ].filter((group) => group.items.length > 0);
  }, [query, themes, config, engine, go, updateConfig, applyTheme, startEngine, stopEngine, notify]);

  const flat = groups.flatMap((group) => group.items);
  const current = flat[Math.min(active, flat.length - 1)];

  function run(item: Item | undefined) {
    if (!item) return;
    onClose();
    item.run();
  }

  function onKeyDown(event: KeyboardEvent) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (step !== undefined) {
      event.preventDefault();
      const next = (active + step + flat.length) % Math.max(1, flat.length);
      setActive(next);
      list.current?.querySelector(`[data-index="${next}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      run(current);
    }
  }

  let index = -1;
  return (
    <dialog
      ref={dialog}
      aria-label="Search"
      onClose={onClose}
      onClick={(event) => event.target === event.currentTarget && onClose()}
      className="m-0 mx-auto mt-[12vh] w-[min(38rem,calc(100vw-2rem))] max-w-none overflow-hidden rounded-xl bg-surface p-0 text-ink-2 shadow-pop ring-1 ring-line-hi backdrop:bg-scrim open:animate-rise"
    >
      <div className="flex items-center gap-3 border-b border-line px-4">
        <Search className="size-[18px] shrink-0 text-ink-3" aria-hidden />
        <input
          autoFocus
          role="combobox"
          aria-expanded
          aria-controls="palette-results"
          aria-activedescendant={current ? `palette-${current.id}` : undefined}
          value={query}
          placeholder="Search settings, themes and actions"
          spellCheck={false}
          onChange={(e) => {
            setQuery(e.currentTarget.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
          className="selectable h-[3.25rem] min-w-0 flex-1 bg-transparent text-[15px] text-ink outline-none placeholder:text-ink-4"
        />
      </div>
      <div ref={list} id="palette-results" role="listbox" aria-label="Results" className="max-h-[min(26rem,60vh)] overflow-y-auto p-1.5">
        {groups.length === 0 && <p className="px-3 py-8 text-center text-[13px] text-ink-3">Nothing called “{query.trim()}”.</p>}
        {groups.map((group) => (
          <div key={group.title} role="group" aria-label={group.title} className="pb-1">
            <div className="ui-eyebrow px-2.5 pt-2.5 pb-1">{group.title}</div>
            {group.items.map((item) => {
              index += 1;
              const at = index;
              const selected = item === current;
              return (
                <div
                  key={item.id}
                  id={`palette-${item.id}`}
                  role="option"
                  aria-selected={selected}
                  data-index={at}
                  onMouseMove={() => setActive(at)}
                  onClick={() => run(item)}
                  className={`flex h-10 cursor-default items-center gap-3 rounded-lg px-2.5 ${selected ? "bg-surface-top text-ink" : "text-ink-2"}`}
                >
                  <item.icon className="size-4 shrink-0 text-ink-3" aria-hidden />
                  <span className="min-w-0 flex-1 truncate text-[13px]">{item.title}</span>
                  <span className="shrink-0 truncate text-[12px] text-ink-3">{item.section}</span>
                  {selected && <CornerDownLeft className="size-3.5 shrink-0 text-ink-3" aria-hidden />}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </dialog>
  );
}
