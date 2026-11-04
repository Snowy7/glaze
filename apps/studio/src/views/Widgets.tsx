import { LayoutDashboard, MonitorUp, MousePointerClick, Search, SearchX, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";

import { AppearanceEditor } from "../components/AppearanceEditor";
import { AppearancePreview } from "../components/AppearancePreview";
import { Badge, Banner, Button, Card, ConfirmButton, EmptyState, Group, Page, Row, SaveStatus, Skeleton, Tabs, TextInput, ToggleRow } from "../components/ui";
import { BUILT_IN, setCount } from "../lib/appearance";
import { api, errorText, type Layout, type PlacedWidget, type WidgetInfo } from "../lib/api";
import { useActiveSkins, useInstalledWidgets } from "../lib/hooks";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";
import { widgetIcon } from "../lib/widgetIcons";

type Tab = "installed" | "placed" | "look";

const CARD_GRID = "grid grid-cols-[repeat(auto-fill,minmax(19rem,1fr))] gap-4";

/** Widgets: what's installed, what's on the desktop, and how all of them look. Edits `widgets.json`. */
export function Widgets() {
  const { notify } = useStudio();
  const [tab, setTab] = useState<Tab>("installed");
  const installed = useInstalledWidgets();
  const skins = useActiveSkins();
  const layout = useAutosaved<Layout>({
    load: api.getLayout,
    save: (current) => api.setWidgetsAppearance(current.appearance),
    concerns: (changes) => changes.widgets,
  });

  async function toggleEditMode() {
    try {
      await api.toggleWidgetEdit();
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  /** Runs an edit on the file itself (not a draft), then shows what's now on disk. */
  async function change(action: () => Promise<Layout>) {
    try {
      await action();
      await layout.reload();
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  const placed = layout.value?.instances ?? [];
  const editButton = (
    <Button variant="primary" icon={MousePointerClick} onClick={() => void toggleEditMode()}>
      Edit layout on desktop
    </Button>
  );

  return (
    <Page
      title="Widgets"
      subtitle="Desktop widgets sit above your wallpaper and below your windows."
      actions={
        <>
          <SaveStatus saving={layout.saving} />
          {editButton}
        </>
      }
      tabs={
        <Tabs<Tab>
          label="Section"
          value={tab}
          onChange={setTab}
          options={[
            { value: "installed", label: "Installed", count: installed?.length },
            { value: "placed", label: "On the desktop", count: layout.value ? placed.length : undefined },
            { value: "look", label: "Look" },
          ]}
        />
      }
    >
      {layout.error && (
        <Banner>
          <span>widgets.json can't be read: {layout.error}. Fix it in a text editor; Studio won't overwrite it.</span>
        </Banner>
      )}

      {tab === "installed" && <Installed installed={installed} placed={placed} />}

      {tab === "placed" &&
        (!layout.value ? (
          !layout.error && <Skeleton className="h-40 rounded-xl" />
        ) : placed.length === 0 ? (
          <EmptyState icon={LayoutDashboard} title="Nothing on the desktop yet" action={editButton}>
            Edit mode shows a grid on your desktop with an Add widget palette. Drag widgets where you want them; Esc leaves.
          </EmptyState>
        ) : (
          <Placed placed={placed} installed={installed} onReset={(item) => void change(() => api.resetInstanceLook(item.instance))} onRemove={(item) => void change(() => api.removeInstance(item.instance))} />
        ))}

      {tab === "look" &&
        (layout.value ? (
          <div className="@container">
            <div className="grid items-start gap-x-10 gap-y-6 @[62rem]:grid-cols-[minmax(0,46rem)_minmax(0,1fr)]">
              <div className="flex min-w-0 flex-col gap-8">
                <Group title="Overlay widgets" description="Widgets that float above your windows.">
                  <ToggleRow
                    title="Show overlay widgets"
                    detail="They stay on top of your windows in a click-through layer: only the widget itself takes the mouse, and it never takes focus. Off hides them everywhere except in edit mode. Hotkey Ctrl+Alt+Shift+O."
                    checked={layout.value.overlay}
                    onChange={(on) => void change(() => api.setWidgetsOverlay(on))}
                  />
                  <Row title="Installed overlay widgets" detail={overlayNames(installed)} />
                </Group>

                <section aria-labelledby="look-heading">
                  <div className="mb-2.5 px-0.5">
                    <h2 id="look-heading" className="text-[13.5px] font-semibold text-ink">
                      Appearance
                    </h2>
                    <p className="mt-0.5 text-[12.5px] text-ink-3">Applies to every widget. A single widget can still override it from its menu in edit mode.</p>
                  </div>
                  <Card flush className="px-5 py-2">
                    <AppearanceEditor
                      target="widgets"
                      hidePreview
                      value={layout.value.appearance}
                      onChange={(appearance) => layout.edit((current) => ({ ...current, appearance }))}
                      below={[skins.widgets]}
                      fallback="the theme"
                    />
                  </Card>
                </section>
              </div>

              {/* Above the fields in a narrow window; beside them, and following the scroll, in a wide one. */}
              <aside aria-label="Live preview" className="sticky top-(--header-h) z-10 order-first -mx-1 bg-canvas px-1 pb-2 @[62rem]:top-[calc(var(--header-h)+1.5rem)] @[62rem]:order-none @[62rem]:mx-0 @[62rem]:px-0 @[62rem]:pt-8">
                {/* Two of them: the tall one only fits beside the fields. */}
                <div className="@[62rem]:hidden">
                  <AppearancePreview target="widgets" base={BUILT_IN.widgets} layers={[skins.widgets, layout.value.appearance]} />
                </div>
                <div className="hidden @[62rem]:block">
                  <AppearancePreview target="widgets" base={BUILT_IN.widgets} layers={[skins.widgets, layout.value.appearance]} tall />
                </div>
                <p className="mt-3 hidden text-[12.5px] text-ink-3 @[62rem]:block">A sample clock and media card, drawn with the same styles your widgets get. Changes save as you make them.</p>
              </aside>
            </div>
          </div>
        ) : (
          !layout.error && <Skeleton className="h-64 rounded-xl" />
        ))}
    </Page>
  );
}

function overlayNames(installed: WidgetInfo[] | null): string {
  const overlays = (installed ?? []).filter((w) => w.preset === "overlay");
  return overlays.length === 0 ? 'None. A widget opts in with "preset": "overlay" in its widget.json.' : overlays.map((w) => w.name).join(", ");
}

const cells = ([columns, rows]: readonly [number, number]) => `${columns}×${rows}`;

/** The installed widgets, searchable by name, id and description. */
function Installed({ installed, placed }: { installed: WidgetInfo[] | null; placed: PlacedWidget[] }) {
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (installed ?? []).filter((w) => needle === "" || `${w.name} ${w.id} ${w.description ?? ""}`.toLowerCase().includes(needle));
  }, [installed, query]);

  if (installed === null)
    return (
      <div aria-hidden className={CARD_GRID}>
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-40 rounded-xl" />
        ))}
      </div>
    );
  if (installed.length === 0)
    return (
      <EmptyState icon={LayoutDashboard} title="No widgets installed">
        Put widget folders in %APPDATA%\mypaper\widgets, or in a widgets folder beside Studio.
      </EmptyState>
    );

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative w-full max-w-72">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-ink-3" aria-hidden />
          <TextInput compact type="search" aria-label="Search widgets" placeholder="Search widgets" value={query} onChange={(e) => setQuery(e.currentTarget.value)} className="w-full pl-8" />
        </div>
        {query !== "" && <p className="text-[12.5px] text-ink-3 tabular-nums">{shown.length === installed.length ? "All match" : `${shown.length} of ${installed.length}`}</p>}
      </div>
      {shown.length === 0 ? (
        <EmptyState compact icon={SearchX} title="No widget matches">
          Nothing called “{query.trim()}”. Try part of a name, like clock or media.
        </EmptyState>
      ) : (
        <ul className={CARD_GRID}>
          {shown.map((widget, index) => (
            <WidgetCard key={widget.id} widget={widget} index={index} count={placed.filter((p) => p.widget === widget.id).length} />
          ))}
        </ul>
      )}
    </>
  );
}

function WidgetCard({ widget, index, count }: { widget: WidgetInfo; index: number; count: number }) {
  const { size } = widget;
  const Icon = widgetIcon(widget.id);
  return (
    <li
      style={{ animationDelay: `${Math.min(index, 16) * 20}ms` }}
      className="flex animate-[rise_320ms_ease-out_backwards] flex-col rounded-xl bg-surface p-4 shadow-card ring-1 ring-line-strong transition-[box-shadow,background-color] duration-200 hover:bg-surface-hi/50 hover:ring-line-hi"
    >
      <div className="flex items-start gap-3">
        <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink ring-1 ring-line-strong">
          <Icon className="size-[18px]" strokeWidth={1.7} aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-[14px] font-semibold tracking-tight text-ink">{widget.name}</h3>
          <p className="selectable truncate font-mono text-[11.5px] text-ink-3" title={widget.dir}>
            {widget.id}
            {widget.version && ` · ${widget.version}`}
          </p>
        </div>
        {count > 0 && (
          <Badge tone="solid" title={`${count} on the desktop`}>
            On desktop{count > 1 && ` ×${count}`}
          </Badge>
        )}
      </div>

      {widget.error ? (
        <p className="selectable mt-3 line-clamp-3 text-xs text-ink" title={widget.error}>
          {widget.error}
        </p>
      ) : (
        widget.description && <p className="mt-3 line-clamp-2 text-[13px] leading-relaxed text-ink-2">{widget.description}</p>
      )}

      <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-4">
        <Badge title={widget.resizable ? `${cells(size.min)} to ${size.max ? cells(size.max) : "any size"}` : "Fixed size"}>
          {cells(size.default)}
          {widget.resizable ? " · resizable" : " · fixed"}
        </Badge>
        {widget.preset !== "desktop" && (
          <Badge tone="outline" title={widget.preset === "overlay" ? "Always on top, click-through except the widget" : "Opened from a bar module"}>
            {widget.preset === "overlay" ? "Overlay" : "Popup"}
          </Badge>
        )}
        {widget.origin === "user" && <Badge tone="outline">Yours</Badge>}
        {widget.permissions.map((permission) => (
          <Badge key={permission} tone="outline" title="A permission it asks for" className="font-mono">
            {permission}
          </Badge>
        ))}
      </div>
    </li>
  );
}

/** `\\.\DISPLAY2` -> `Display 2`. */
function monitorName(device: string | null): string {
  const number = device && /DISPLAY(\d+)/i.exec(device)?.[1];
  return number ? `Display ${number}` : (device ?? "Primary display");
}

/** The widgets on the desktop, by display. */
function Placed({
  placed,
  installed,
  onReset,
  onRemove,
}: {
  placed: PlacedWidget[];
  installed: WidgetInfo[] | null;
  onReset: (item: PlacedWidget) => void;
  onRemove: (item: PlacedWidget) => void;
}) {
  const byMonitor = new Map<string, PlacedWidget[]>();
  for (const item of placed) {
    const monitor = monitorName(item.monitor);
    byMonitor.set(monitor, [...(byMonitor.get(monitor) ?? []), item]);
  }
  return (
    <div className="flex flex-col gap-8">
      {[...byMonitor].map(([monitor, items]) => (
        <section key={monitor} aria-label={monitor}>
          <h2 className="mb-2.5 flex items-center gap-2 px-0.5 text-[13.5px] font-semibold text-ink">
            <MonitorUp className="size-4 text-ink-3" aria-hidden />
            {monitor}
            <span className="font-normal text-ink-3 tabular-nums">{items.length}</span>
          </h2>
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(21rem,1fr))] gap-4">
            {items.map((item) => (
              <PlacedCard key={item.instance} item={item} widget={installed?.find((w) => w.id === item.widget)} onReset={() => onReset(item)} onRemove={() => onRemove(item)} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

function PlacedCard({ item, widget, onReset, onRemove }: { item: PlacedWidget; widget: WidgetInfo | undefined; onReset: () => void; onRemove: () => void }) {
  const own = setCount(item.appearance);
  const Icon = widgetIcon(item.widget);
  const facts: [string, string][] = [
    ["Cell", item.cell ? `${item.cell[0]}, ${item.cell[1]}` : "Auto"],
    ["Size", item.size ? cells(item.size) : widget ? cells(widget.size.default) : "Default"],
    ["Look", own === 0 ? "Global" : `Own · ${own}`],
  ];
  return (
    <li className="flex flex-col rounded-xl bg-surface shadow-card ring-1 ring-line-strong">
      <div className="flex items-center gap-3 px-4 pt-4 pb-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink ring-1 ring-line-strong">
          <Icon className="size-4" strokeWidth={1.7} aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-ink">{widget?.name ?? item.widget}</div>
          <div className="selectable truncate font-mono text-[11.5px] text-ink-3">{item.instance}</div>
        </div>
        {!widget && (
          <Badge tone="outline" title="Kept in widgets.json, but not shown">
            Not installed
          </Badge>
        )}
      </div>
      <dl className="mx-4 grid grid-cols-3 divide-x divide-line rounded-lg bg-canvas ring-1 ring-line ring-inset">
        {facts.map(([label, value]) => (
          <div key={label} className="min-w-0 px-3 py-2">
            <dt className="text-[11px] text-ink-3">{label}</dt>
            <dd className="truncate text-[12.5px] font-medium text-ink tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="mt-auto flex items-center justify-end gap-2 px-4 pt-3 pb-4">
        <Button size="sm" variant="ghost" disabled={own === 0} onClick={onReset} title="Make this widget follow the global look again">
          Reset look
        </Button>
        <ConfirmButton size="sm" icon={Trash2} label={`Remove ${item.instance}`} onConfirm={onRemove}>
          Remove
        </ConfirmButton>
      </div>
    </li>
  );
}
