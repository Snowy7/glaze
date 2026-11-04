import { AppWindow, Blocks, Dock, Image as ImageIcon, LayoutGrid, MousePointerClick, Pause, PanelTop, Pencil, Play } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, type ThemeEntry } from "../../lib/api";
import type { Resolved } from "../../lib/barsDoc";
import type { Desktop } from "../../lib/desktop";
import { displayName } from "../../lib/desktopGrid";
import { useHistory } from "../../lib/history";
import { KINDS } from "../../lib/kinds";
import { useNav, type Target } from "../../lib/nav";
import { useStudio } from "../../lib/studio";
import { ThemePreview } from "../ThemeCard";
import { Button, InspectorHeader, InspectorSection, NavRow, Prop, Switch } from "../ui";

const PRESET_NAMES: Record<Resolved["preset"], string> = {
  monochrome: "Monochrome",
  macos: "macOS",
  windows11: "Windows 11",
  islands: "Islands",
  "neon-glass": "Neon glass",
  floating: "Floating",
};

/** Nothing selected on the Desktop page: what's on the desktop, each a step from its editor. */
export function DesktopOverview({ desktop, onSelect }: { desktop: Desktop; onSelect: (target: Target) => void }) {
  const { go } = useNav();
  const { config } = useStudio();
  const theme = desktop.active;
  const bars = desktop.bars?.config;
  return (
    <>
      <InspectorHeader icon={LayoutGrid} title="Your desktop" detail={displayName(desktop.monitor.device)} />
      {theme && (
        <InspectorSection title="Wallpaper">
          <button type="button" onClick={() => onSelect({ kind: "wallpaper" })} className="group flex cursor-default items-center gap-3 rounded-lg text-left">
            <span className="aspect-[16/10] w-24 shrink-0 overflow-hidden rounded-md ring-1 ring-line-strong transition group-hover:ring-line-hi">
              <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} />
            </span>
            <span className="min-w-0">
              <span className="block truncate text-[14px] font-semibold text-ink">{theme.name}</span>
              <span className="block text-[12px] text-ink-3">
                {theme.kind ? KINDS[theme.kind].label : "Theme"}
                {config?.paused ? " · Paused" : ""}
              </span>
            </span>
          </button>
          <div className="flex gap-2">
            <Button size="sm" className="flex-1" onClick={() => go({ page: "themes" })}>
              Change theme
            </Button>
            <PauseButton />
          </div>
        </InspectorSection>
      )}
      <InspectorSection title="On this desktop">
        <div className="flex flex-col">
          <NavRow icon={Blocks} title="Widgets" detail={desktop.items.length === 0 ? "None yet" : `${desktop.items.length} on this display`} onClick={() => go({ page: "widgets" })} />
          <NavRow icon={PanelTop} title="Top bar" detail={bars ? (bars.top.enabled ? PRESET_NAMES[bars.preset] : "Off") : "Loading"} onClick={() => onSelect({ kind: "bar", bar: "top" })} />
          <NavRow icon={Dock} title="Dock" detail={bars ? (bars.dock.enabled ? (bars.dock.style === "taskbar" ? "Taskbar" : "Floating") : "Off") : "Loading"} onClick={() => onSelect({ kind: "bar", bar: "dock" })} />
          <NavRow icon={AppWindow} title="Windows" detail="Accent, corners, pointer" onClick={() => go({ page: "windows" })} />
          <NavRow icon={MousePointerClick} title="Right-click menu" detail={config?.desktop_menu.enabled ? "mypaper's menu" : "Windows' menu"} onClick={() => go({ page: "windows", select: "menu" })} />
        </div>
      </InspectorSection>
      <InspectorSection>
        <p className="text-[12px] leading-relaxed text-ink-3">Click anything on the desktop to change it. Drag widgets to move them.</p>
      </InspectorSection>
    </>
  );
}

export function PauseButton({ size = "sm" }: { size?: "sm" | "md" }) {
  const { config, updateConfig } = useStudio();
  if (!config) return null;
  const paused = config.paused;
  return (
    <Button size={size} icon={paused ? Play : Pause} aria-pressed={paused} onClick={() => void updateConfig({ paused: !paused }, { label: paused ? "Resume wallpaper" : "Pause wallpaper" })}>
      {paused ? "Resume" : "Pause"}
    </Button>
  );
}

/**
 * The wallpaper: the theme, pausing, and (for web pages and 3D scenes) the mouse. On the Desktop
 * page it leads back to the overview (`onBack`) and on to the theme editor; inside the editor it
 * has neither.
 */
export function WallpaperInspector({ theme, onBack }: { theme: ThemeEntry; onBack?: () => void }) {
  const { go } = useNav();
  return (
    <>
      <InspectorHeader icon={ImageIcon} title={theme.name} detail={theme.kind ? `${KINDS[theme.kind].label} wallpaper` : "Wallpaper"} back={onBack && { label: "Your desktop", onClick: onBack }} />
      {onBack && (
        <InspectorSection>
          <div className="aspect-[16/10] overflow-hidden rounded-lg ring-1 ring-line-strong">
            <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} />
          </div>
          <div className="flex gap-2">
            <Button size="sm" className="flex-1" onClick={() => go({ page: "themes" })}>
              Change theme
            </Button>
            <PauseButton />
          </div>
        </InspectorSection>
      )}
      {(theme.kind === "web" || theme.kind === "scene") && <InteractiveSetting theme={theme} />}
      {theme.kind !== "web" && theme.kind !== "scene" && !onBack && (
        <InspectorSection>
          <p className="text-[12.5px] leading-snug text-ink-3">{theme.kind ? `${KINDS[theme.kind].hint}.` : ""} The picture and its options come from the theme's source.</p>
        </InspectorSection>
      )}
      {onBack && (
        <InspectorSection>
          <NavRow icon={Pencil} title="Edit theme" detail="Windows style, widget and bar looks" onClick={() => go({ page: "themes", view: "edit", theme: theme.id })} />
        </InspectorSection>
      )}
    </>
  );
}

/** Whether the mouse reaches a web or 3D wallpaper (a theme setting, so only on the user's own). */
function InteractiveSetting({ theme }: { theme: ThemeEntry }) {
  const { notify } = useStudio();
  const history = useHistory();
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    let current = true;
    void api.getInteractive(theme.dir).then((value) => current && setOn(value === true), () => undefined);
    return () => {
      current = false;
    };
  }, [theme.dir]);

  const write = (value: boolean) => {
    setOn(value);
    api.setInteractive(theme.dir, value).catch((e: unknown) => notify(errorText(e), "error"));
  };

  const readOnly = theme.origin !== "user";
  return (
    <InspectorSection title="Mouse">
      <Prop
        id="wallpaper.interactive"
        label="React to the mouse"
        hint={readOnly ? "Copy this theme to your own to change it." : theme.kind === "scene" ? "Drag on the bare desktop to turn the camera." : "The page gets moves, clicks and the wheel over the bare desktop."}
      >
        <Switch
          label="React to the mouse"
          checked={on === true}
          disabled={on === null || readOnly}
          onChange={(value) => {
            history.record({ label: value ? "Let the wallpaper react to the mouse" : "Stop the wallpaper reacting to the mouse", undo: () => write(!value), redo: () => write(value) });
            write(value);
          }}
        />
      </Prop>
    </InspectorSection>
  );
}

/** A bar on the Desktop page: what it is, and the way to its editor. */
export function BarSummary({ bar, config, onBack }: { bar: "top" | "dock"; config: Resolved | undefined; onBack: () => void }) {
  const { go } = useNav();
  const on = config ? (bar === "top" ? config.top.enabled : config.dock.enabled) : false;
  const facts: [string, string][] = config
    ? bar === "top"
      ? [
          ["Preset", PRESET_NAMES[config.preset]],
          ["Shape", { strip: "Strip", floating: "Floating", islands: "Islands" }[config.top.layout]],
          ["Edge", config.top.position === "top" ? "Top" : "Bottom"],
        ]
      : [
          ["Preset", PRESET_NAMES[config.preset]],
          ["Style", config.dock.style === "taskbar" ? "Taskbar" : "Floating"],
          ["Edge", config.dock.position[0]?.toUpperCase() + config.dock.position.slice(1)],
        ]
    : [];
  return (
    <>
      <InspectorHeader icon={bar === "top" ? PanelTop : Dock} title={bar === "top" ? "Top bar" : "Dock"} detail={on ? "Shown" : "Off"} back={{ label: "Your desktop", onClick: onBack }} />
      <InspectorSection>
        <dl className="divide-y divide-line rounded-lg bg-canvas ring-1 ring-line-strong ring-inset">
          {facts.map(([label, value]) => (
            <div key={label} className="flex items-center justify-between px-3 py-2">
              <dt className="text-[12.5px] text-ink-3">{label}</dt>
              <dd className="text-[13px] font-medium text-ink">{value}</dd>
            </div>
          ))}
        </dl>
        <Button variant="primary" className="w-full" onClick={() => go({ page: "bars" })}>
          Edit {bar === "top" ? "the top bar" : "the dock"}
        </Button>
      </InspectorSection>
    </>
  );
}
