import { Plus, Search } from "lucide-react";
import { useMemo, useState } from "react";

import type { WidgetInfo } from "../../lib/api";
import { widgetIcon } from "../../lib/widgetIcons";
import { InspectorHeader, InspectorSection, TextInput } from "../ui";

type Props = {
  installed: readonly WidgetInfo[];
  /** How many of each are on the desktop already. */
  counts: ReadonlyMap<string, number>;
  onAdd: (widget: WidgetInfo) => void;
  onBack: () => void;
};

/** Every widget that can go on the desktop; a click adds one to the first free space. */
export function WidgetCatalog({ installed, counts, onAdd, onBack }: Props) {
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return installed.filter((w) => w.preset !== "popup" && !w.error && (needle === "" || `${w.name} ${w.id} ${w.description ?? ""}`.toLowerCase().includes(needle)));
  }, [installed, query]);

  return (
    <>
      <InspectorHeader icon={Plus} title="Add a widget" detail="Click one to put it on this display" back={{ label: "All widgets", onClick: onBack }} />
      <div className="border-b border-line px-4 py-3">
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-ink-3" aria-hidden />
          <TextInput compact autoFocus type="search" aria-label="Search widgets" placeholder="Search widgets" value={query} onChange={(e) => setQuery(e.currentTarget.value)} className="w-full pl-8" />
        </div>
      </div>
      <InspectorSection>
        {shown.length === 0 ? (
          <p className="py-6 text-center text-[13px] text-ink-3">No widget called “{query.trim()}”.</p>
        ) : (
          <ul className="-mx-2 -my-1.5 flex flex-col">
            {shown.map((widget) => {
              const Icon = widgetIcon(widget.id);
              const count = counts.get(widget.id) ?? 0;
              return (
                <li key={widget.id}>
                  <button type="button" onClick={() => onAdd(widget)} className="group flex w-full cursor-default items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors hover:bg-surface-hi">
                    <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink ring-1 ring-line-strong transition-colors group-hover:bg-surface-top">
                      <Icon className="size-[18px]" strokeWidth={1.75} aria-hidden />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-2">
                        <span className="truncate text-[13px] font-medium text-ink">{widget.name}</span>
                        {widget.preset === "overlay" && <span className="shrink-0 text-[11px] text-ink-3">Always on top</span>}
                      </span>
                      <span className="block truncate text-[12px] text-ink-3">{widget.description ?? `${widget.size.default[0]}×${widget.size.default[1]}`}</span>
                    </span>
                    {count > 0 && <span className="shrink-0 rounded-md bg-surface-hi px-1.5 text-[11px] leading-5 text-ink-2 tabular-nums">{count}</span>}
                    <Plus className="size-4 shrink-0 text-ink-4 transition-colors group-hover:text-ink" aria-hidden />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </InspectorSection>
    </>
  );
}
