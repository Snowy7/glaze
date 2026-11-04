import { AppWindow, MousePointerClick, RotateCcw } from "lucide-react";
import { useState } from "react";

import { MakeCopy } from "../components/inspectors/MakeCopy";
import { MenuFields } from "../components/inspectors/MenuFields";
import { NO_STYLE, WindowsStyleFields } from "../components/inspectors/WindowsStyleFields";
import { DesktopScene } from "../components/stage/DesktopScene";
import { WindowsLayer } from "../components/stage/WindowsLayer";
import { EditorLayout, IconButton, Inspector, InspectorHeader, InspectorSection, NavRow, PageBar, SaveState } from "../components/ui";
import { api, type ThemeEntry, type WindowsStyle } from "../lib/api";
import { useDesktop } from "../lib/desktop";
import { useNav, type WindowsTarget } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";

/** How windows, the pointer and the desktop's right-click menu look. Window styling is saved with the theme on screen. */
export function Windows() {
  const { themes } = useStudio();
  const active = themes.find((t) => t.active);
  // A theme change (or a copy made here) starts over with that theme's values.
  return active ? <WindowsEditor key={active.id} theme={active} /> : <WindowsEditor theme={null} />;
}

const same = (a: WindowsStyle, b: WindowsStyle) => JSON.stringify(a) === JSON.stringify(b);

function WindowsEditor({ theme }: { theme: ThemeEntry | null }) {
  const { route } = useNav();
  const { config } = useStudio();
  const desktop = useDesktop();
  const [selected, setSelected] = useState<WindowsTarget>(route.page === "windows" && route.select ? route.select : "style");
  const style = useAutosaved<WindowsStyle>({
    load: () => (theme ? api.getStyle(theme.dir) : Promise.resolve(NO_STYLE)),
    save: (next) => (theme ? api.setStyle(theme.dir, next) : Promise.resolve(next)),
    concerns: (changes) => changes.themes,
    label: "Change window style",
  });
  const editable = theme?.origin === "user";
  const current = style.value ?? NO_STYLE;

  return (
    <>
      <PageBar title="Windows" detail={theme ? `Saved with ${theme.name}` : undefined}>
        <SaveState saving={style.saving} />
      </PageBar>
      <EditorLayout
        stage={
          <DesktopScene
            desktop={desktop}
            selection={null}
            onSelect={() => setSelected("style")}
            editable={{}}
            overlay={<WindowsLayer screen={desktop.grid.screen} style={current} menu={config?.desktop_menu.enabled ? config.desktop_menu.items : null} selected={selected} onSelect={setSelected} />}
          />
        }
        inspector={
          <Inspector label="Inspector">
            {selected === "menu" ? (
              <>
                <InspectorHeader icon={MousePointerClick} title="Right-click menu" detail="On the bare desktop" back={{ label: "Windows", onClick: () => setSelected("style") }} />
                {config && <MenuFields config={config} />}
              </>
            ) : (
              <>
                <InspectorHeader
                  icon={AppWindow}
                  title="Windows"
                  detail="Colors, corners, material and pointer"
                  actions={editable && <IconButton icon={RotateCcw} variant="ghost" size="sm" label="Leave everything as Windows has it" disabled={same(current, NO_STYLE)} onClick={() => style.edit(() => NO_STYLE, "Reset window style")} />}
                />
                {theme && !editable && <MakeCopy theme={theme} apply />}
                {style.value && <WindowsStyleFields style={current} disabled={!editable} onChange={(next, label) => style.edit(() => next, label)} />}
                <InspectorSection>
                  <NavRow icon={MousePointerClick} title="Right-click menu" detail={config?.desktop_menu.enabled ? "mypaper's menu" : "Windows' menu"} onClick={() => setSelected("menu")} />
                </InspectorSection>
              </>
            )}
          </Inspector>
        }
      />
    </>
  );
}
