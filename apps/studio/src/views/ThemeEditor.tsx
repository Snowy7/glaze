import { FileCode, Image as ImageIcon, X } from "lucide-react";
import { useState } from "react";

import { WallpaperInspector } from "../components/inspectors/DesktopInspectors";
import { LookFields } from "../components/inspectors/LookFields";
import { MakeCopy } from "../components/inspectors/MakeCopy";
import { NO_STYLE, WindowsStyleFields } from "../components/inspectors/WindowsStyleFields";
import { DesktopScene } from "../components/stage/DesktopScene";
import { WindowsLayer } from "../components/stage/WindowsLayer";
import { Button, EditorLayout, EmptyState, IconButton, Inspector, More, PageBar, SaveState, Segmented } from "../components/ui";
import { setCount } from "../lib/appearance";
import { api, errorText, pickStyleSheet, type SkinEdit, type SkinKind, type ThemeEntry, type WindowsStyle } from "../lib/api";
import { useDesktop } from "../lib/desktop";
import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { useAutosaved, type Autosaved } from "../lib/useAutosaved";

type Tab = "wallpaper" | "windows" | "widgets" | "bars";

const EMPTY_SKIN: SkinEdit = { appearance: {}, css: null, glass: null };

/** One theme, edited in place: its wallpaper options, Windows style, and the looks it gives widgets and bars. */
export function ThemeEditor({ themeId }: { themeId: string }) {
  const { themes, themesLoaded } = useStudio();
  const { go } = useNav();
  const theme = themes.find((t) => t.id === themeId);
  if (!theme)
    return (
      <>
        <PageBar title="Theme" back={{ label: "Themes", onClick: () => go({ page: "themes" }) }} />
        {themesLoaded && <EmptyState icon={ImageIcon} title="This theme is gone">It was removed or renamed outside Studio.</EmptyState>}
      </>
    );
  return <Editor key={theme.id} theme={theme} />;
}

function useSkin(theme: ThemeEntry, kind: SkinKind) {
  return useAutosaved<SkinEdit>({
    load: () => api.getSkin(theme.dir, kind),
    save: (skin) => api.setSkin(theme.dir, kind, skin),
    concerns: (changes) => changes.themes,
    label: kind === "widgets" ? "Change the theme's widget look" : "Change the theme's bar look",
  });
}

function Editor({ theme }: { theme: ThemeEntry }) {
  const { go } = useNav();
  const { applyTheme } = useStudio();
  const desktop = useDesktop();
  const [tab, setTab] = useState<Tab>("windows");
  const editable = theme.origin === "user";
  const style = useAutosaved<WindowsStyle>({ load: () => api.getStyle(theme.dir), save: (next) => api.setStyle(theme.dir, next), concerns: (c) => c.themes, label: "Change window style" });
  const widgets = useSkin(theme, "widgets");
  const bars = useSkin(theme, "bars");
  const userLook = desktop.model.layout?.appearance ?? {};
  const saving = style.saving || widgets.saving || bars.saving;

  return (
    <>
      <PageBar title={theme.name} back={{ label: "Themes", onClick: () => go({ page: "themes" }) }}>
        <SaveState saving={saving} />
        {!theme.active && (
          <Button variant="primary" onClick={() => void applyTheme(theme)}>
            Use this theme
          </Button>
        )}
      </PageBar>
      <EditorLayout
        stage={
          <DesktopScene
            desktop={desktop}
            theme={theme}
            widgetLook={[widgets.value?.appearance ?? {}, userLook]}
            selection={null}
            onSelect={() => undefined}
            editable={{}}
            overlay={tab === "windows" && <WindowsLayer screen={desktop.grid.screen} style={style.value ?? NO_STYLE} menu={null} selected="style" onSelect={null} />}
          />
        }
        inspector={
          <Inspector label="Theme">
            <div className="border-b border-line px-4 py-3">
              <Segmented<Tab>
                label="Part of the theme"
                hideLabel
                className="w-full"
                size="sm"
                value={tab}
                onChange={setTab}
                options={[
                  { value: "wallpaper", label: "Wallpaper" },
                  { value: "windows", label: "Windows" },
                  { value: "widgets", label: "Widgets" },
                  { value: "bars", label: "Bars" },
                ]}
              />
            </div>
            {!editable && <MakeCopy theme={theme} apply={theme.active} onCopied={(copy) => go({ page: "themes", view: "edit", theme: copy.id })} />}
            {tab === "wallpaper" && <WallpaperInspector theme={theme} />}
            {tab === "windows" && style.value && <WindowsStyleFields style={style.value} disabled={!editable} onChange={(next, label) => style.edit(() => next, label)} />}
            {tab === "widgets" && <SkinFields skin={widgets} kind="widgets" theme={theme} editable={editable} userSets={setCount(userLook)} />}
            {tab === "bars" && <SkinFields skin={bars} kind="bars" theme={theme} editable={editable} userSets={0} />}
          </Inspector>
        }
      />
    </>
  );
}

/** The look a theme gives widgets or the bars, and its optional style sheet. */
function SkinFields({ skin, kind, theme, editable, userSets }: { skin: Autosaved<SkinEdit>; kind: SkinKind; theme: ThemeEntry; editable: boolean; userSets: number }) {
  const { notify } = useStudio();
  const value = skin.value ?? EMPTY_SKIN;

  async function chooseCss() {
    const picked = await pickStyleSheet();
    if (!picked) return;
    try {
      const css = await api.importSkinCss(theme.dir, kind, picked);
      skin.edit((current) => ({ ...current, css }), "Change the style sheet");
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  return (
    <>
      {userSets > 0 && (
        <p className="mx-4 mt-4 rounded-lg bg-surface px-3 py-2.5 text-[12px] leading-snug text-ink-3 ring-1 ring-line-strong">
          Your own widget look changes {userSets} of these on your desktop, and wins over the theme.
        </p>
      )}
      {skin.value && (
        <LookFields
          target={kind}
          prefix={`theme.${kind}`}
          value={value.appearance}
          belowLabel="Default"
          disabled={!editable}
          onChange={(appearance, label) => skin.edit((current) => ({ ...current, appearance }), label)}
          glass={kind === "widgets" ? { value: value.glass, onChange: (glass, label) => skin.edit((current) => ({ ...current, glass }), label) } : undefined}
        />
      )}
      <More label="Style sheet">
        <div data-setting={`theme.${kind}.css`} className="setting-anchor flex items-center gap-3">
          <FileCode className="size-4 shrink-0 text-ink-3" aria-hidden />
          <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-ink-2" title={value.css ?? undefined}>
            {value.css ?? "None"}
          </span>
          <Button size="sm" disabled={!editable} onClick={() => void chooseCss()}>
            {value.css ? "Replace" : "Choose"}
          </Button>
          {value.css && <IconButton icon={X} size="sm" variant="ghost" label="Remove the style sheet" disabled={!editable} onClick={() => skin.edit((current) => ({ ...current, css: null }), "Remove the style sheet")} />}
        </div>
        <p className="text-[12px] leading-snug text-ink-3">A CSS file for fine-tuning, loaded after the built-in styles.</p>
      </More>
    </>
  );
}
