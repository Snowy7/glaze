import { LayoutDashboard, MousePointerClick, Trash2 } from "lucide-react";
import { useState } from "react";

import { AppearanceEditor } from "../components/AppearanceEditor";
import { Banner, Button, ConfirmButton, EmptyState, Group, PageHeader, Row, SaveStatus, Segmented, Skeleton, ToggleRow } from "../components/ui";
import { setCount } from "../lib/appearance";
import { api, errorText, type Layout, type PlacedWidget, type WidgetInfo } from "../lib/api";
import { useActiveSkins, useInstalledWidgets } from "../lib/hooks";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";

type Tab = "installed" | "placed" | "look";

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
  const overlays = (installed ?? []).filter((w) => w.preset === "overlay");
  const tabs: readonly { value: Tab; label: string }[] = [
    { value: "installed", label: installed ? `Installed · ${installed.length}` : "Installed" },
    { value: "placed", label: `On the desktop · ${placed.length}` },
    { value: "look", label: "Look" },
  ];

  return (
    <>
      <PageHeader
        title="Widgets"
        subtitle="Desktop widgets sit above your wallpaper and below your windows."
        actions={
          <>
            <SaveStatus saving={layout.saving} />
            <Button variant="primary" icon={MousePointerClick} onClick={() => void toggleEditMode()}>
              Edit layout on desktop
            </Button>
          </>
        }
      />

      {layout.error && (
        <Banner>
          <span>widgets.json can't be read: {layout.error}. Fix it in a text editor; Studio won't overwrite it.</span>
        </Banner>
      )}

      <div className="max-w-4xl">
        <Segmented<Tab> label="Section" hideLabel className="mb-6" value={tab} options={tabs} onChange={setTab} />

        {tab === "installed" &&
          (installed === null ? (
            <Skeleton className="h-40 rounded-2xl" />
          ) : installed.length === 0 ? (
            <EmptyState icon={LayoutDashboard} title="No widgets installed">
              Put widget folders in %APPDATA%\mypaper\widgets, or in a widgets folder beside Studio.
            </EmptyState>
          ) : (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-4">
              {installed.map((widget) => (
                <WidgetCard key={widget.id} widget={widget} count={placed.filter((p) => p.widget === widget.id).length} />
              ))}
            </ul>
          ))}

        {tab === "placed" &&
          (placed.length === 0 ? (
            <EmptyState
              icon={LayoutDashboard}
              title="Nothing on the desktop yet"
              action={
                <Button variant="primary" icon={MousePointerClick} onClick={() => void toggleEditMode()}>
                  Edit layout on desktop
                </Button>
              }
            >
              Edit mode shows a grid on your desktop with an Add widget palette. Drag widgets where you want them; Esc leaves.
            </EmptyState>
          ) : (
            <ul className="divide-y divide-line overflow-hidden rounded-2xl bg-surface shadow-card ring-1 ring-line">
              {placed.map((item) => (
                <PlacedRow
                  key={item.instance}
                  item={item}
                  widget={installed?.find((w) => w.id === item.widget)}
                  onReset={() => void change(() => api.resetInstanceLook(item.instance))}
                  onRemove={() => void change(() => api.removeInstance(item.instance))}
                />
              ))}
            </ul>
          ))}

        {tab === "look" &&
          (layout.value ? (
            <div className="flex flex-col gap-6">
            <Group title="Overlay widgets">
              <ToggleRow
                title="Show overlay widgets"
                detail="Widgets whose preset is overlay stay on top of your windows in a click-through layer: only the widget itself takes the mouse, and it never takes focus. Off hides them everywhere except in edit mode. Hotkey Ctrl+Alt+Shift+O."
                checked={layout.value.overlay}
                onChange={(on) => void change(() => api.setWidgetsOverlay(on))}
              />
              <Row
                title="Overlay widgets installed"
                detail={overlays.length === 0 ? "None. A widget opts in with \"preset\": \"overlay\" in its widget.json." : overlays.map((w) => w.name).join(", ")}
              />
            </Group>
            <div className="rounded-2xl bg-surface p-5 shadow-card ring-1 ring-line">
              <p className="mb-4 text-[13px] text-ink-3">
                Applies to every widget. A single widget can still override it from its menu in edit mode.
              </p>
              <AppearanceEditor
                target="widgets"
                value={layout.value.appearance}
                onChange={(appearance) => layout.edit((current) => ({ ...current, appearance }))}
                below={[skins.widgets]}
                fallback="the theme"
              />
            </div>
            </div>
          ) : (
            !layout.error && <Skeleton className="h-64 rounded-2xl" />
          ))}
      </div>
    </>
  );
}

const cells = ([columns, rows]: readonly [number, number]) => `${columns}×${rows}`;

function WidgetCard({ widget, count }: { widget: WidgetInfo; count: number }) {
  const { size } = widget;
  return (
    <li className="flex flex-col rounded-2xl bg-surface p-4 shadow-card ring-1 ring-line">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="truncate font-display text-[15px] font-semibold tracking-tight text-ink">{widget.name}</h3>
          <p className="selectable truncate font-mono text-[11.5px] text-ink-3" title={widget.dir}>
            {widget.id}
            {widget.version && ` · ${widget.version}`}
          </p>
        </div>
        <span className="flex shrink-0 gap-1.5">
          {widget.preset !== "desktop" && (
            <span className="rounded-full bg-surface-hi px-2 py-0.5 text-[11px] font-medium text-ink-2" title={widget.preset === "overlay" ? "Always on top, click-through except the widget" : "Opened from a bar module"}>
              {widget.preset === "overlay" ? "Overlay" : "Popup"}
            </span>
          )}
          <span className="rounded-full bg-surface-hi px-2 py-0.5 text-[11px] font-medium text-ink-2">{widget.origin === "user" ? "Yours" : "Built in"}</span>
        </span>
      </div>

      {widget.error ? (
        <p className="selectable mt-3 line-clamp-3 text-xs text-ink" title={widget.error}>
          {widget.error}
        </p>
      ) : (
        <>
          {widget.description && <p className="mt-2 text-[13px] text-ink-2">{widget.description}</p>}
          <dl className="mt-3 space-y-1.5 text-xs">
            <div className="flex gap-2">
              <dt className="w-16 shrink-0 text-ink-3">Size</dt>
              <dd className="text-ink-2">
                {widget.resizable ? (
                  <>
                    {cells(size.default)} default, {cells(size.min)} to {size.max ? cells(size.max) : "any size"}
                  </>
                ) : (
                  <>{cells(size.default)}, fixed</>
                )}
              </dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-16 shrink-0 text-ink-3">Can use</dt>
              <dd className="flex flex-wrap gap-1">
                {widget.permissions.length === 0 ? (
                  <span className="text-ink-2">Nothing beyond its own box</span>
                ) : (
                  widget.permissions.map((permission) => (
                    <code key={permission} className="rounded-md bg-surface-hi px-1.5 py-0.5 font-mono text-[11px] text-ink-2">
                      {permission}
                    </code>
                  ))
                )}
              </dd>
            </div>
          </dl>
        </>
      )}

      <p className="mt-auto pt-3 text-xs text-ink-3">{count === 0 ? "Not on the desktop" : `On the desktop ×${count}`}</p>
    </li>
  );
}

/** `\\.\DISPLAY2` -> `Display 2`. */
function monitorName(device: string | null): string {
  const number = device && /DISPLAY(\d+)/i.exec(device)?.[1];
  return number ? `Display ${number}` : (device ?? "Primary display");
}

function PlacedRow({
  item,
  widget,
  onReset,
  onRemove,
}: {
  item: PlacedWidget;
  widget: WidgetInfo | undefined;
  onReset: () => void;
  onRemove: () => void;
}) {
  const own = setCount(item.appearance);
  return (
    <li className="flex items-center gap-4 px-5 py-3.5">
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[14px] font-medium text-ink">{widget?.name ?? item.widget}</span>
          <span className="selectable font-mono text-[11.5px] text-ink-3">{item.instance}</span>
          {!widget && <span className="text-xs text-ink-3" title="Kept in widgets.json, but not shown">not installed</span>}
        </div>
        <p className="mt-0.5 text-xs text-ink-3">
          {monitorName(item.monitor)}
          {item.cell && item.size && ` · cell ${item.cell[0]}, ${item.cell[1]} · ${cells(item.size)}`}
          {own > 0 && ` · own look (${own} ${own === 1 ? "field" : "fields"})`}
        </p>
      </div>
      <Button disabled={own === 0} onClick={onReset} title="Make this widget follow the global look again">
        Reset look
      </Button>
      <ConfirmButton icon={Trash2} label={`Remove ${item.instance}`} onConfirm={onRemove}>
        Remove
      </ConfirmButton>
    </li>
  );
}
