import { AppWindow, Folder, Globe, Mail, Music, Settings as SettingsIcon, Terminal, type LucideIcon } from "lucide-react";
import { useState, type CSSProperties, type ReactNode } from "react";

import type { BarsCatalog } from "../../lib/api";
import { dockGeometry, topGeometry, type Resolved } from "../../lib/barsDoc";
import type { ModuleRef } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";
import { Outline } from "./Stage";

// The top bar and the dock as silhouettes on the stage: their real position, size and shape from
// the resolved `bars.json`, a glass surface, and each module's icon. The Bars page (phase 2) swaps
// this for the real bars page in an iframe (docs/studio-ux.md, "Bars").

export type BarId = "top" | "dock";

type Props = {
  config: Resolved;
  catalog: BarsCatalog;
  screen: { width: number; height: number };
  selected: BarId | null;
  /** Null: shown, not selectable. */
  onSelect: ((bar: BarId) => void) | null;
};

const SAMPLE_APPS: readonly LucideIcon[] = [Folder, Globe, Terminal, Mail, Music, AppWindow, SettingsIcon];

type Rect = { x: number; y: number; width: number; height: number };

function clock(): string {
  return new Date().toLocaleString("en-GB", { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).replace(",", "");
}

export function BarsLayer({ config, catalog, screen, selected, onSelect }: Props) {
  const [hovered, setHovered] = useState<BarId | null>(null);
  const light = config.look.scheme === "light";
  // A bar whose background is "none" is just its modules on the wallpaper (a macOS menu bar).
  const bare = config.look.background?.type === "none";
  const radius = config.look.radius;
  const rects = layout(config, catalog, screen);

  const frame = (bar: BarId, rect: Rect, round: number, children: ReactNode) => (
    <div
      key={bar}
      className="absolute"
      style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
      onPointerEnter={() => setHovered(bar)}
      onPointerLeave={() => setHovered(null)}
    >
      {children}
      {onSelect && (
        <button
          type="button"
          aria-label={bar === "top" ? "Top bar" : "Dock"}
          aria-pressed={selected === bar}
          className="absolute inset-0 cursor-default outline-none"
          style={{ borderRadius: round }}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => onSelect(bar)}
        />
      )}
      <Outline box={{ x: 0, y: 0, width: rect.width, height: rect.height }} selected={selected === bar} hovered={hovered === bar && !!onSelect} label={bar === "top" ? "Top bar" : "Dock"} radius={round} below={rect.y < 48} />
    </div>
  );

  const top = rects.top;
  const dock = rects.dock;
  const topRound = config.top.layout === "strip" ? 0 : (radius ?? 12);
  return (
    <div className="pointer-events-none absolute inset-0 [&_button]:pointer-events-auto" style={{ color: light ? "#151515" : "#fff" }}>
      {top &&
        frame(
          "top",
          top,
          topRound,
          config.top.layout === "islands" ? (
            <div className="absolute inset-0 flex items-stretch justify-between">
              {[config.top.left, config.top.center, config.top.right].map((modules, i) => (
                <Glass key={i} light={light} bare={bare} radius={top.height / 2} className="flex items-center gap-4 px-4">
                  <Modules refs={modules} size={top.height} />
                </Glass>
              ))}
            </div>
          ) : (
            <Glass light={light} bare={bare} radius={topRound} className="absolute inset-0 grid grid-cols-[1fr_auto_1fr] items-center gap-6 px-4">
              <Modules refs={config.top.left} size={top.height} />
              <Modules refs={config.top.center} size={top.height} />
              <Modules refs={config.top.right} size={top.height} end />
            </Glass>
          ),
        )}
      {dock &&
        frame(
          "dock",
          dock.rect,
          config.dock.style === "taskbar" ? 0 : (radius ?? 18),
          <Glass light={light} radius={config.dock.style === "taskbar" ? 0 : (radius ?? 18)} className={`absolute inset-0 flex items-center gap-2 ${dock.vertical ? "flex-col justify-center py-2" : "px-2"}`}>
            {config.dock.style === "taskbar" && <Modules refs={config.dock.left} size={dock.icon} />}
            <div className={`flex items-center ${dock.vertical ? "flex-col" : ""} ${config.dock.style === "taskbar" && config.dock.align === "center" ? "mx-auto" : ""}`} style={{ gap: dock.icon * 0.22 }}>
              {Array.from({ length: dock.apps }, (_, i) => {
                const Icon = SAMPLE_APPS[i % SAMPLE_APPS.length] ?? AppWindow;
                return (
                  <span key={i} className="grid place-items-center rounded-[22%] bg-white/14" style={{ width: dock.icon, height: dock.icon }}>
                    <Icon style={{ width: dock.icon * 0.5, height: dock.icon * 0.5 }} strokeWidth={1.6} />
                  </span>
                );
              })}
            </div>
            {config.dock.style === "taskbar" && <Modules refs={config.dock.right} size={dock.icon} end />}
          </Glass>,
        )}
    </div>
  );
}

/** Where each bar sits on the screen, in DIPs. */
function layout(config: Resolved, catalog: BarsCatalog, screen: { width: number; height: number }) {
  let top: Rect | null = null;
  if (config.top.enabled) {
    const { margin } = topGeometry(config, catalog);
    const height = config.top.height;
    const width = config.top.layout === "floating" ? widthOf(config.top.width, screen.width - 2 * margin) : screen.width - 2 * margin;
    const y = config.top.position === "top" ? margin : screen.height - height - margin;
    top = { x: (screen.width - width) / 2, y, width, height };
  }
  let dock: { rect: Rect; icon: number; apps: number; vertical: boolean } | null = null;
  if (config.dock.enabled) {
    const geometry = dockGeometry(config, catalog);
    const icon = Math.max(16, geometry.size * (config.dock.style === "taskbar" ? 0.62 : 0.78));
    const thickness = geometry.size + 2 * geometry.padding;
    const apps = Math.min(9, Math.max(4, config.dock.pinned?.length ?? 7));
    const vertical = config.dock.position === "left" || config.dock.position === "right";
    const along = config.dock.style === "taskbar" ? (vertical ? screen.height : screen.width) : apps * icon * 1.22 + 2 * geometry.padding + icon * 0.4;
    const m = config.dock.style === "taskbar" ? 0 : geometry.margin;
    const rect: Rect = vertical
      ? { x: config.dock.position === "left" ? m : screen.width - thickness - m, y: (screen.height - along) / 2, width: thickness, height: along }
      : { x: (screen.width - along) / 2, y: config.dock.position === "top" ? m : screen.height - thickness - m, width: along, height: thickness };
    dock = { rect, icon, apps, vertical };
  }
  return { top, dock };
}

/** A floating bar's width: the whole edge, hugging its modules (about half), a share, or DIPs. */
function widthOf(width: Resolved["top"]["width"], full: number): number {
  if (typeof width === "number") return Math.min(full, width);
  if (typeof width === "string" && width.endsWith("%")) return (full * (Number.parseFloat(width) || 100)) / 100;
  return width === "auto" ? full * 0.55 : full;
}

function Glass({ light, radius, bare = false, className = "", children }: { light: boolean; radius: number; bare?: boolean; className?: string; children: ReactNode }) {
  const style: CSSProperties = bare ? { borderRadius: radius } : {
    borderRadius: radius,
    background: light ? "rgb(246 246 248 / 0.55)" : "rgb(22 24 30 / 0.42)",
    backdropFilter: "blur(20px) saturate(1.2)",
    boxShadow: light ? "inset 0 0 0 1px rgb(0 0 0 / 0.08)" : "inset 0 0 0 1px rgb(255 255 255 / 0.1)",
  };
  return (
    <div className={className} style={style}>
      {children}
    </div>
  );
}

function Modules({ refs, size, end = false }: { refs: readonly ModuleRef[]; size: number; end?: boolean }) {
  const font = Math.max(10, size * 0.4);
  return (
    <div className={`flex min-w-0 items-center ${end ? "justify-end" : ""}`} style={{ gap: size * 0.45, fontSize: font }}>
      {refs.map((ref, i) =>
        ref === "|" ? (
          <span key={i} className="h-[45%] w-px bg-current opacity-30" />
        ) : (
          <Module key={`${ref}:${i}`} moduleRef={ref} size={size} />
        ),
      )}
    </div>
  );
}

function Module({ moduleRef, size }: { moduleRef: ModuleRef; size: number }) {
  const { icon: Icon, sample } = describeModule(moduleRef);
  const text = moduleRef === "clock" ? clock() : sample;
  return (
    <span className="flex shrink-0 items-center gap-[0.35em] font-medium whitespace-nowrap opacity-90">
      {moduleRef !== "clock" && <Icon style={{ width: size * 0.46, height: size * 0.46 }} strokeWidth={1.8} />}
      {text && moduleRef !== "tray" && <span className={moduleRef === "clock" ? "" : "opacity-80"}>{text}</span>}
    </span>
  );
}
