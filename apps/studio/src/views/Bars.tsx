import { useState } from "react";

import { AppearanceEditor } from "../components/AppearanceEditor";
import { DockPanel, TaskbarPanel, TopBarPanel, type EditBars } from "../components/bars/BarPanels";
import { Banner, Button, PageHeader, SaveStatus, Segmented, Skeleton } from "../components/ui";
import { api, type BarsState } from "../lib/api";
import { useActiveSkins } from "../lib/hooks";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";

type Tab = "top" | "dock" | "taskbar" | "look";

const TABS: readonly { value: Tab; label: string }[] = [
  { value: "top", label: "Top bar" },
  { value: "dock", label: "Dock" },
  { value: "taskbar", label: "Taskbar" },
  { value: "look", label: "Look" },
];

/** The top bar, the dock and the Windows taskbar: edits `bars.json`, which `mypaper-bars` watches. */
export function Bars() {
  const { components, notify } = useStudio();
  const [tab, setTab] = useState<Tab>("top");
  const skins = useActiveSkins();
  const file = useAutosaved<BarsState>({
    load: api.getBars,
    save: (state) => api.setBars(state.config),
    concerns: (changes) => changes.bars,
  });

  const state = file.value;
  const edit: EditBars = (change) => file.edit((current) => ({ ...current, config: change(current.config) }));
  const running = components?.find((c) => c.id === "bars")?.running;

  async function reset() {
    try {
      await api.resetBars();
      await file.reload();
      notify("bars.json reset to the defaults");
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), "error");
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
      {state?.warnings.map((warning) => (
        <Banner key={warning}>
          <span>{warning}</span>
        </Banner>
      ))}
      {state && running === false && (
        <p className="mb-5 text-[13px] text-ink-3">mypaper-bars isn't running. Your choices are saved, and apply when it starts.</p>
      )}

      {state ? (
        <div className="max-w-3xl">
          <Segmented<Tab> label="Section" hideLabel className="mb-6" value={tab} options={TABS} onChange={setTab} />
          {tab === "top" && <TopBarPanel config={state.config} edit={edit} />}
          {tab === "dock" && <DockPanel config={state.config} edit={edit} />}
          {tab === "taskbar" && <TaskbarPanel config={state.config} edit={edit} />}
          {tab === "look" && (
            <div className="rounded-2xl bg-surface p-5 shadow-card ring-1 ring-line">
              <AppearanceEditor
                target="bars"
                value={state.config.appearance}
                onChange={(appearance) => edit((config) => ({ ...config, appearance }))}
                below={[skins.bars]}
                fallback="the theme"
                layout={state.config.top}
              />
            </div>
          )}
        </div>
      ) : (
        !file.error && <Skeleton className="h-64 max-w-3xl rounded-2xl" />
      )}
    </>
  );
}
