import { Monitor } from "lucide-react";
import type { ReactNode } from "react";

import type { Appearance, ThemeEntry } from "../../lib/api";
import type { Desktop } from "../../lib/desktop";
import { displayName } from "../../lib/desktopGrid";
import { useWallpaperDataUrl } from "../../lib/wallpaperImage";
import type { Target } from "../../lib/nav";
import { ThemePreview } from "../ThemeCard";
import { Segmented } from "../ui";
import { BarsLayer } from "./BarsLayer";
import { Stage, StageTools, WallpaperLayer } from "./Stage";
import { WidgetsLayer } from "./WidgetsLayer";

type Props = {
  desktop: Desktop;
  /** The wallpaper to show; the active theme by default. */
  theme?: ThemeEntry | null;
  /** The layers under each widget's own look; the theme's then the look of all widgets by default. */
  widgetLook?: readonly Appearance[];
  selection: Target | null;
  onSelect: (target: Target | null) => void;
  /** Which layers can be selected (the others are shown as they are). */
  editable: { wallpaper?: boolean; widgets?: boolean; bars?: boolean };
  /** Drawn over everything else (the Windows samples). */
  overlay?: ReactNode;
  footer?: ReactNode;
  /** More tools beside the monitor switcher. */
  tools?: ReactNode;
  showGrid?: boolean;
};

/** The desktop on stage: the wallpaper, the widgets (live), the bars, and whatever the page adds on top. */
export function DesktopScene({ desktop, theme, widgetLook, selection, onSelect, editable, overlay, footer, tools, showGrid }: Props) {
  const { grid, model, installed, skins, bars } = desktop;
  const wallpaper = theme === undefined ? desktop.active : theme;
  const wallpaperImage = useWallpaperDataUrl(wallpaper?.preview ?? null);
  const select = (target: Target) => onSelect(target);
  const widgetSelection = selection?.kind === "widget" ? selection.instance : null;

  return (
    <Stage
      label="Your desktop"
      screen={grid.screen}
      onBackground={() => onSelect(null)}
      footer={footer}
      tools={displayTools(desktop, () => onSelect(null), tools)}
    >
      <WallpaperLayer selected={selection?.kind === "wallpaper"} onSelect={editable.wallpaper ? () => select({ kind: "wallpaper" }) : undefined}>
        {wallpaper ? <ThemePreview name={wallpaper.name} kind={wallpaper.kind} preview={wallpaper.preview} /> : <div className="size-full bg-[radial-gradient(80%_80%_at_30%_20%,#3a3a3a,#111)]" />}
      </WallpaperLayer>
      <WidgetsLayer
        grid={grid}
        items={desktop.items}
        installed={installed}
        below={widgetLook ?? [skins.widgets, model.layout?.appearance ?? {}]}
        theme={wallpaper?.name ?? ""}
        selected={widgetSelection}
        onSelect={editable.widgets ? (instance) => select({ kind: "widget", instance }) : null}
        onPlace={editable.widgets ? (instance, cell, size) => model.put({ instance, cell, size }, "Arrange widget") : undefined}
        showGrid={showGrid}
      />
      {bars && (
        <BarsLayer
          doc={bars.doc}
          config={bars.config}
          catalog={bars.catalog}
          wallpaper={wallpaperImage}
          mode={editable.bars ? "bars" : "show"}
          selection={selection?.kind === "bar" ? { bar: selection.bar } : null}
          onSelect={(pick) => select({ kind: "bar", bar: pick.bar })}
        />
      )}
      {overlay}
    </Stage>
  );
}

/**
 * What goes over the stage: the display switcher (with more than one display; `onChange` after a
 * switch), then the page's own tools. Null when there's neither, so the stage has no tool row.
 */
export function displayTools(desktop: Desktop, onChange: () => void, tools?: ReactNode): ReactNode {
  const { monitors, monitor } = desktop;
  if (monitors.length < 2 && !tools) return null;
  return (
    <>
      {monitors.length > 1 ? (
        <StageTools>
          <Monitor className="ml-1.5 size-3.5 text-ink-3" aria-hidden />
          <Segmented<string>
            label="Display"
            hideLabel
            size="sm"
            value={monitor.device}
            options={monitors.map((m) => ({ value: m.device, label: displayName(m.device) }))}
            onChange={(device) => {
              desktop.setDevice(device);
              onChange();
            }}
          />
        </StageTools>
      ) : (
        <span />
      )}
      {tools}
    </>
  );
}
