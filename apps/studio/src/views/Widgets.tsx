import { Blocks, MousePointerClick, Plus, RotateCcw } from "lucide-react";
import { useState } from "react";

import { LookFields } from "../components/inspectors/LookFields";
import { WidgetCatalog } from "../components/inspectors/WidgetCatalog";
import { WidgetInspector } from "../components/inspectors/WidgetInspector";
import { DesktopScene } from "../components/stage/DesktopScene";
import { Banner, Button, EditorLayout, IconButton, Inspector, InspectorHeader, InspectorSection, More, NavRow, PageBar, Prop, SaveState, Switch } from "../components/ui";
import { setCount } from "../lib/appearance";
import { api, errorText, type WidgetInfo } from "../lib/api";
import { useDesktop, type Desktop } from "../lib/desktop";
import { displayName, firstFree, nextInstanceId } from "../lib/desktopGrid";
import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { widgetIcon } from "../lib/widgetIcons";

type Mode = { kind: "all" } | { kind: "add" } | { kind: "widget"; instance: string };

/** Your widgets, live on the desktop: add, move, resize, set their options and how they look. */
export function Widgets() {
  const { route } = useNav();
  const { notify } = useStudio();
  const desktop = useDesktop();
  const { model, grid, items, placements, installed } = desktop;
  const [mode, setMode] = useState<Mode>(() => (route.page === "widgets" && route.select ? { kind: "widget", instance: route.select } : route.page === "widgets" && route.adding ? { kind: "add" } : { kind: "all" }));
  const all = () => setMode({ kind: "all" });

  const editOnDesktop = () => void api.toggleWidgetEdit().catch((e: unknown) => notify(errorText(e), "error"));

  function add(widget: WidgetInfo) {
    const size = widget.size.default;
    const cell = firstFree(grid, size, placements);
    if (!cell) return notify(`There's no free space for ${widget.name} on this display. Remove or shrink a widget first.`);
    const instance = nextInstanceId(widget.id, model.layout?.instances.map((i) => i.instance) ?? []);
    model.put({ instance, widget: widget.id, monitor: desktop.monitor.device, cell, size }, `Add ${widget.name}`);
    setMode({ kind: "widget", instance });
  }

  const selected = mode.kind === "widget" ? items.find((i) => i.instance === mode.instance) : undefined;
  const inspector = (() => {
    if (mode.kind === "add") {
      const counts = new Map<string, number>();
      for (const item of model.layout?.instances ?? []) counts.set(item.widget, (counts.get(item.widget) ?? 0) + 1);
      return <WidgetCatalog installed={installed} counts={counts} onAdd={add} onBack={all} />;
    }
    if (selected)
      return (
        <WidgetInspector
          key={selected.instance}
          item={selected}
          widget={installed.find((w) => w.id === selected.widget)}
          grid={grid}
          placements={placements}
          model={model}
          themeLook={desktop.skins.widgets}
          onBack={all}
          onSelect={(instance) => setMode({ kind: "widget", instance })}
          onEditOnDesktop={editOnDesktop}
        />
      );
    return <AllWidgets desktop={desktop} onSelect={(instance) => setMode({ kind: "widget", instance })} onAdd={() => setMode({ kind: "add" })} />;
  })();

  return (
    <>
      <PageBar title="Widgets" detail={`${items.length} on ${displayName(desktop.monitor.device)}`}>
        <SaveState saving={model.saving} />
        <Button icon={MousePointerClick} onClick={editOnDesktop} title="Shows the grid on your real desktop, to arrange widgets there">
          Arrange on desktop
        </Button>
        <Button variant="primary" icon={Plus} onClick={() => setMode({ kind: "add" })}>
          Add widget
        </Button>
      </PageBar>
      {model.error && (
        <div className="px-(--gutter) pt-4">
          <Banner>
            <span>Your widget layout can't be read: {model.error}. Studio won't change it until it's fixed.</span>
          </Banner>
        </div>
      )}
      <EditorLayout
        stage={
          <DesktopScene
            desktop={desktop}
            selection={selected ? { kind: "widget", instance: selected.instance } : null}
            onSelect={(target) => setMode(target?.kind === "widget" ? { kind: "widget", instance: target.instance } : { kind: "all" })}
            editable={{ widgets: true }}
            showGrid={mode.kind === "add"}
          />
        }
        inspector={<Inspector label="Inspector">{inspector}</Inspector>}
      />
    </>
  );
}

/** Nothing selected: the look every widget starts from, what's on this display, and the always-on-top layer. */
function AllWidgets({ desktop, onSelect, onAdd }: { desktop: Desktop; onSelect: (instance: string) => void; onAdd: () => void }) {
  const { model, items, installed, skins } = desktop;
  const look = model.layout?.appearance ?? {};
  return (
    <>
      <InspectorHeader
        icon={Blocks}
        title="All widgets"
        detail="The look every widget starts from"
        actions={<IconButton icon={RotateCcw} variant="ghost" size="sm" label="Reset to the theme's look" disabled={setCount(look) === 0} onClick={() => model.setLook({}, "Reset the look of all widgets")} />}
      />
      {model.layout && <LookFields target="widgets" prefix="widgets.look" value={look} below={[skins.widgets]} belowLabel="Theme" onChange={(next, label) => model.setLook(next, label)} />}
      <InspectorSection title="On this display">
        {items.length === 0 ? (
          <Button icon={Plus} onClick={onAdd} className="self-start">
            Add your first widget
          </Button>
        ) : (
          <div className="flex flex-col">
            {items.map((item) => {
              const widget = installed.find((w) => w.id === item.widget);
              return <NavRow key={item.instance} icon={widgetIcon(item.widget)} title={widget?.name ?? item.widget} detail={widget ? (setCount(item.appearance) > 0 ? "Its own look" : undefined) : "Not installed"} onClick={() => onSelect(item.instance)} />;
            })}
          </div>
        )}
      </InspectorSection>
      <More>
        <Prop id="widgets.overlay" label="Always-on-top widgets" hint="Widgets made to float over your windows, like the media pill. Off hides them until you turn this back on.">
          <Switch label="Always-on-top widgets" checked={model.layout?.overlay ?? true} disabled={!model.layout} onChange={model.setOverlay} />
        </Prop>
      </More>
    </>
  );
}
