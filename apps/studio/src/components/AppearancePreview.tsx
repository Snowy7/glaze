import { Folder, Globe, Music, Settings as SettingsIcon, Terminal } from "lucide-react";
import { useState } from "react";

import { appearanceData, appearanceVars, DOCK_RADIUS, resolve, type Resolved } from "../lib/appearance";
import type { Appearance, SkinKind } from "../lib/api";
import type { ModuleRef } from "../lib/barsSchema";
import { describeModule } from "../lib/modules";
import { Segmented } from "./ui";

export type BarLayout = { left: readonly ModuleRef[]; center: readonly ModuleRef[]; right: readonly ModuleRef[] };

const SAMPLE_LAYOUT: BarLayout = {
  left: ["start", "workspaces", "app"],
  center: ["clock"],
  right: ["network", "volume", "battery"],
};

type Props = {
  target: SkinKind;
  /** The built-in layer for this target. */
  base: Resolved;
  /** Layers over `base`, lowest first. */
  layers: readonly Appearance[];
  /** The top bar's modules; a sample set when absent. */
  layout?: BarLayout;
};

/**
 * A sample widget card, or a sample top bar and dock, painted with the same CSS
 * variables and data attributes the real pages get (docs/widgets.md, "Appearance").
 */
export function AppearancePreview({ target, base, layers, layout = SAMPLE_LAYOUT }: Props) {
  const [wallpaper, setWallpaper] = useState<"dark" | "light">("dark");
  const look = resolve(base, ...layers);
  return (
    <div className="relative overflow-hidden rounded-xl ring-1 ring-line" aria-label="Preview">
      <div className="ap-stage relative h-44" data-wallpaper={wallpaper} aria-hidden>
        {target === "widgets" ? <WidgetSample look={look} /> : <BarsSample look={look} layers={layers} base={base} layout={layout} />}
      </div>
      <div className="absolute right-2 bottom-2">
        <Segmented
          label="Sample wallpaper"
          hideLabel
          value={wallpaper}
          onChange={setWallpaper}
          options={[
            { value: "dark", label: "Dark" },
            { value: "light", label: "Light" },
          ]}
          className="text-[11px] [&_label]:px-2 [&_label]:py-0.5"
        />
      </div>
    </div>
  );
}

function WidgetSample({ look }: { look: Resolved }) {
  return (
    <div className="ap-root absolute inset-0 flex items-center gap-4 px-8" {...appearanceData(look)} style={appearanceVars(look)}>
      <div className="ap-surface w-52 p-4" style={{ fontSize: "calc(13px * var(--scale))" }}>
        <div className="text-[3.1em] leading-none font-light tracking-tight tabular-nums">10:42</div>
        <div className="mt-1.5 text-[1em]" style={{ color: "var(--t1)" }}>
          Tuesday 4 November
        </div>
        <div className="mt-3 h-1.5 overflow-hidden rounded-full" style={{ background: "var(--fill-2)" }}>
          <div className="h-full w-3/5 rounded-full" style={{ background: "var(--accent)" }} />
        </div>
        <div className="mt-2.5 flex items-center justify-between text-[0.85em]" style={{ color: "var(--t2)" }}>
          <span>CPU 12%</span>
          <span style={{ color: "var(--accent)" }}>RAM 41%</span>
        </div>
      </div>
      <div className="ap-surface w-28 self-center p-3" style={{ fontSize: "calc(13px * var(--scale))" }}>
        <div className="text-[0.85em]" style={{ color: "var(--t2)" }}>
          Now playing
        </div>
        <div className="mt-1 text-[1em] font-medium">Slow Light</div>
        <div className="mt-2 grid size-7 place-items-center rounded-full" style={{ background: "var(--fill-3)" }}>
          <Music className="size-3.5" style={{ color: "var(--accent)" }} />
        </div>
      </div>
    </div>
  );
}

const DOCK_ICONS = [Folder, Globe, Terminal, Music, SettingsIcon];

function BarsSample({ look, layers, base, layout }: { look: Resolved } & Pick<Props, "layers" | "base"> & { layout: BarLayout }) {
  // The dock's own default radius differs from the top bar's; a chosen radius applies to both.
  const dock = resolve({ ...base, radius: DOCK_RADIUS }, ...layers);
  return (
    <>
      <div className="ap-root absolute inset-x-0 top-0" {...appearanceData(look)} style={appearanceVars(look)}>
        <div className="ap-surface flex h-8 items-center justify-between gap-2 px-3 text-[12px]" style={{ fontSize: "calc(12px * var(--scale))" }}>
          <ModuleRun modules={layout.left} />
          <ModuleRun modules={layout.center} />
          <ModuleRun modules={layout.right} />
        </div>
      </div>
      <div className="ap-root absolute inset-x-0 bottom-3 flex justify-center" {...appearanceData(dock)} style={appearanceVars(dock)}>
        <div className="ap-surface flex items-end gap-2 p-1.5">
          {DOCK_ICONS.map((Icon, i) => (
            <div key={i} className="flex flex-col items-center gap-0.5">
              <span className="grid size-8 place-items-center" style={{ background: "var(--fill-2)", borderRadius: "calc(var(--radius) * 0.5)" }}>
                <Icon className="size-4" style={{ color: i === 0 ? "var(--accent)" : "var(--t1)" }} />
              </span>
              <span className="size-[3px] rounded-full" style={{ background: i < 3 ? "var(--t1)" : "transparent" }} />
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

function ModuleRun({ modules }: { modules: readonly ModuleRef[] }) {
  return (
    <div className="flex min-w-0 items-center gap-2.5">
      {modules.map((module) => {
        const { icon: Icon, sample } = describeModule(module);
        return (
          <span key={module} className="flex items-center gap-1 whitespace-nowrap">
            <Icon className="size-3.5 shrink-0" style={{ color: module === "start" ? "var(--accent)" : "var(--t1)" }} />
            {sample && <span style={{ color: module === "clock" ? "var(--t0)" : "var(--t1)" }}>{sample}</span>}
          </span>
        );
      })}
    </div>
  );
}
