import { useEffect, useMemo, useState } from "react";

import { BarPreview } from "../components/bars/BarPreview";
import { TaskbarPanel, TopBarPanel } from "../components/bars/BarPanels";
import { DesignPicker } from "../components/bars/DesignPicker";
import { DockPanel } from "../components/bars/DockPanel";
import { IconsPanel } from "../components/bars/IconsPanel";
import { LookPanel } from "../components/bars/LookPanel";
import { MenusPanel } from "../components/bars/MenusPanel";
import { ModulesPanel } from "../components/bars/ModulesPanel";
import { MotionPanel } from "../components/bars/MotionPanel";
import { PresetsPanel } from "../components/bars/PresetsPanel";
import { Banner, Button, PageHeader, SaveStatus, Segmented, Skeleton } from "../components/ui";
import { api, errorText, type BarsCatalog, type BarsState } from "../lib/api";
import { getIn, resolve, setIn, type Path } from "../lib/barsDoc";
import { BarsContext, forBar, type BarsEditor } from "../lib/barsContext";
import type { BarKind } from "../lib/barsSchema";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";

type Tab = "presets" | "top" | "dock" | "menus" | "icons" | "taskbar";

const TABS: readonly { value: Tab; label: string }[] = [
  { value: "presets", label: "Presets" },
  { value: "top", label: "Top bar" },
  { value: "dock", label: "Bottom bar" },
  { value: "menus", label: "Menus" },
  { value: "icons", label: "Icons" },
  { value: "taskbar", label: "Windows taskbar" },
];

/** What each bar's editor has: its design, its layout, modules, its own look and its own motion. */
type Section = "design" | "layout" | "modules" | "look" | "motion";
const SECTIONS: readonly { value: Section; label: string }[] = [
  { value: "design", label: "Design" },
  { value: "layout", label: "Layout" },
  { value: "modules", label: "Modules" },
  { value: "look", label: "Look" },
  { value: "motion", label: "Motion" },
];

/** One bar's editor; look and motion edit that bar's own (`top.look`, `dock.animation`...). */
function BarEditor({ bar, editor }: { bar: BarKind; editor: BarsEditor }) {
  const [section, setSection] = useState<Section>("design");
  const scoped = useMemo(() => forBar(editor, bar), [editor, bar]);
  return (
    <>
      <Segmented<Section> label="Part" hideLabel className="mb-6" value={section} options={SECTIONS} onChange={setSection} />
      {section !== "design" && (
        // The page's top padding scrolls under a sticky box, so it reaches up over it.
        <div className="sticky -top-9 z-10 -mx-1 -mt-6 mb-5 bg-canvas px-1 pt-9 pb-2">
          <BarPreview config={editor.resolved} catalog={editor.catalog} vars={editor.doc.vars} appearance={editor.doc.appearance} width={360} />
        </div>
      )}
      {section === "design" && <DesignPicker bar={bar} />}
      {section === "layout" && (bar === "top" ? <TopBarPanel /> : <DockPanel />)}
      {section === "modules" && <ModulesPanel />}
      <BarsContext value={scoped}>
        {section === "look" && <LookPanel />}
        {section === "motion" && <MotionPanel />}
      </BarsContext>
    </>
  );
}

/** The top bar, the dock and the Windows taskbar: edits `bars.json`, which `mypaper-bars` watches. */
export function Bars() {
  const { components, notify } = useStudio();
  const [tab, setTab] = useState<Tab>("presets");
  const [catalog, setCatalog] = useState<BarsCatalog | null>(null);
  const file = useAutosaved<BarsState>({
    load: api.getBars,
    save: (state) => api.setBars(state.doc),
    concerns: (changes) => changes.bars,
  });

  useEffect(() => {
    let current = true;
    api.barsCatalog().then(
      (loaded) => current && setCatalog(loaded),
      (e: unknown) => current && notify(errorText(e), "error"),
    );
    return () => {
      current = false;
    };
  }, [notify]);

  const state = file.value;
  const editor = useMemo<BarsEditor | null>(() => {
    if (!state || !catalog) return null;
    const { doc } = state;
    return {
      doc,
      resolved: resolve(catalog, doc),
      catalog,
      set: (path: Path, value: unknown) => file.edit((current) => ({ ...current, doc: setIn(current.doc, path, value) })),
      update: (change) => file.edit((current) => ({ ...current, doc: change(current.doc) })),
      has: (path: Path) => getIn(doc, path) !== undefined,
    };
  }, [state, catalog, file.edit]);

  const running = components?.find((c) => c.id === "bars")?.running;
  const problem = state?.problem ?? null;

  async function reset() {
    try {
      await api.resetBars();
      await file.reload();
      notify("bars.json reset to the defaults");
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  return (
    <>
      <PageHeader
        title="Bars"
        subtitle={state ? (state.exists ? "Changes apply to the running bars as you make them." : "No bars.json yet: these are the defaults. Your first change creates it.") : "Loading"}
        actions={<SaveStatus saving={file.saving} />}
      />

      {file.error && (
        <Banner>
          <span>bars.json can't be read: {file.error}</span>
          <Button onClick={() => void reset()}>Reset to defaults</Button>
        </Banner>
      )}
      {problem && (
        <Banner>
          <span>
            The bars ignore bars.json as it is: {problem}. Studio won't change a file the bars can't read; fix it in a text editor, or start over.
          </span>
          <Button onClick={() => void reset()}>Reset to defaults</Button>
        </Banner>
      )}
      {state && running === false && !problem && (
        <p className="mb-5 text-[13px] text-ink-3">mypaper-bars isn't running. Your choices are saved, and apply when it starts.</p>
      )}

      {editor && !problem ? (
        <BarsContext value={editor}>
          <div className="max-w-3xl">
            <Segmented<Tab> label="Section" hideLabel className="mb-6" value={tab} options={TABS} onChange={setTab} />
            {tab === "presets" && <PresetsPanel />}
            {(tab === "top" || tab === "dock") && <BarEditor key={tab} bar={tab} editor={editor} />}
            {tab === "menus" && <MenusPanel />}
            {tab === "icons" && <IconsPanel />}
            {tab === "taskbar" && <TaskbarPanel />}
          </div>
        </BarsContext>
      ) : (
        !file.error && !problem && <Skeleton className="h-64 max-w-3xl rounded-2xl" />
      )}
    </>
  );
}
