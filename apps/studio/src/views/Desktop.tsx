import { useState } from "react";

import { BarSummary, DesktopOverview, PauseButton, WallpaperInspector } from "../components/inspectors/DesktopInspectors";
import { WidgetInspector } from "../components/inspectors/WidgetInspector";
import { DesktopScene } from "../components/stage/DesktopScene";
import { ThemeStrip } from "../components/ThemeStrip";
import { EditorLayout, Inspector, PageBar, SaveState } from "../components/ui";
import { api, errorText } from "../lib/api";
import { useDesktop } from "../lib/desktop";
import { useNav, type Target } from "../lib/nav";
import { useStudio } from "../lib/studio";

/** Home: your desktop, live. Click the wallpaper, a widget or a bar to change it right there. */
export function Desktop() {
  const { route } = useNav();
  const { notify } = useStudio();
  const desktop = useDesktop();
  const [selection, setSelection] = useState<Target | null>(route.page === "desktop" ? (route.select ?? null) : null);
  const back = () => setSelection(null);

  const inspector = (() => {
    if (selection?.kind === "wallpaper" && desktop.active) return <WallpaperInspector theme={desktop.active} onBack={back} />;
    if (selection?.kind === "bar") return <BarSummary bar={selection.bar} config={desktop.bars?.config} onBack={back} />;
    if (selection?.kind === "widget") {
      const item = desktop.items.find((i) => i.instance === selection.instance);
      if (item)
        return (
          <WidgetInspector
            key={item.instance}
            item={item}
            widget={desktop.installed.find((w) => w.id === item.widget)}
            grid={desktop.grid}
            placements={desktop.placements}
            model={desktop.model}
            themeLook={desktop.skins.widgets}
            onBack={back}
            onSelect={(instance) => setSelection({ kind: "widget", instance })}
            onEditOnDesktop={() => void api.toggleWidgetEdit().catch((e: unknown) => notify(errorText(e), "error"))}
          />
        );
    }
    return <DesktopOverview desktop={desktop} onSelect={setSelection} />;
  })();

  return (
    <>
      <PageBar title="Desktop" detail={desktop.active?.name}>
        <SaveState saving={desktop.model.saving} />
        <PauseButton size="md" />
      </PageBar>
      <EditorLayout
        stage={<DesktopScene desktop={desktop} selection={selection} onSelect={setSelection} editable={{ wallpaper: true, widgets: true, bars: true }} footer={<ThemeStrip />} />}
        inspector={<Inspector label="Inspector">{inspector}</Inspector>}
      />
    </>
  );
}
