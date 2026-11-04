import { Folder, Globe, Music, Settings as SettingsIcon, Terminal } from "lucide-react";
import { useState, type CSSProperties, type ReactNode } from "react";

import { cssColor, fillShowsWallpaper, gradientCss, paintCss, surfaceBase, type ColorContext } from "../../lib/barLook";
import { dockGeometry, magnifyOf, topGeometry, type Resolved } from "../../lib/barsDoc";
import type { BarsCatalog, Appearance } from "../../lib/api";
import type { Border, ItemState, Json, Look, ModuleRef, State } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";
import { Segmented } from "../ui";

// A sketch of the bars for a resolved `bars.json`: the top bar and the dock on a stand-in
// wallpaper, drawn from the same values the host merges (preset, file, defaults). It is a
// 1280x720 DIP screen at half size, scaled to `width`; shapes, colors, materials and the
// layout read right, text is only suggested.

const K = 2 / 3;
const STAGE = { width: 640, height: 360 } as const;
const DOCK_APPS = [Folder, Globe, Terminal, Music, SettingsIcon];

type Props = {
  config: Resolved;
  catalog: BarsCatalog;
  /** The file's `vars`, for `var(--name)` colors. */
  vars?: Record<string, Json>;
  appearance?: Appearance;
  /** Pixel width of the whole preview. */
  width: number;
  /** Fixed sample wallpaper; without it the preview offers a dark/light switch. */
  wallpaper?: "dark" | "light";
  /** Icons only (for small cards). */
  dense?: boolean;
};

export function BarPreview({ config, catalog, vars = {}, appearance, width, wallpaper, dense = false }: Props) {
  const [chosen, setChosen] = useState<"dark" | "light">("dark");
  const tone = wallpaper ?? chosen;
  const base = surfaceBase(appearance);
  const scheme = config.top.look.scheme ?? config.dock.look.scheme ?? "auto";
  const text = scheme === "light" ? "#141414" : scheme === "dark" ? "#ffffff" : base.text === "light" ? "#ffffff" : "#141414";
  const ctx: ColorContext = { text, accent: base.accent, vars };
  const scale = width / STAGE.width;
  const bandTop = config.top.enabled ? topGeometry(config, catalog) : null;
  const topBand = bandTop ? (config.top.height + 2 * bandTop.margin) * K : 0;

  return (
    <div className="relative overflow-hidden rounded-xl ring-1 ring-line" style={{ width, height: STAGE.height * scale }} role="img" aria-label="Preview of the bars">
      <div
        className="ap-stage absolute top-0 left-0 origin-top-left overflow-hidden"
        data-wallpaper={tone}
        style={{ width: STAGE.width, height: STAGE.height, transform: `scale(${scale})`, color: text, fontSize: 8, lineHeight: 1 }}
        aria-hidden
      >
        {config.top.enabled && <TopBar config={config} catalog={catalog} base={base} ctx={ctx} dense={dense} />}
        {config.dock.enabled && <Dock config={config} catalog={catalog} base={base} ctx={ctx} dense={dense} topBand={config.top.position === config.dock.position ? topBand : 0} />}
        {!config.top.enabled && !config.dock.enabled && (
          <div className="absolute inset-0 grid place-items-center text-[10px] opacity-70">Both bars are off</div>
        )}
      </div>
      {wallpaper === undefined && (
        <div className="absolute right-2 bottom-2">
          <Segmented
            label="Sample wallpaper"
            hideLabel
            value={chosen}
            onChange={setChosen}
            options={[
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
            ]}
            className="text-[11px] [&_label]:px-2 [&_label]:py-0.5"
          />
        </div>
      )}
    </div>
  );
}

type Shared = { config: Resolved; catalog: BarsCatalog; base: ReturnType<typeof surfaceBase>; ctx: ColorContext; dense: boolean };

// --- Surfaces ---

type SurfaceProps = {
  look: Look;
  base: ReturnType<typeof surfaceBase>;
  ctx: ColorContext;
  radius: number;
  square?: boolean;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
};

/** The look's material: background fill, tint, grain, ring border, glow and shadow, around `children`. */
function Surface({ look, base, ctx, radius, square = false, className = "", style, children }: SurfaceProps) {
  const fill = look.background;
  const glass = fillShowsWallpaper(fill);
  const scheme = look.scheme ?? "auto";
  const lightScheme = scheme === "light" || (scheme === "auto" && base.text === "dark");
  // A tint made for the other scheme gives way to a neutral of this one (docs/bars.md, "Look").
  const tintColor = scheme === "light" ? "#f2f2f2" : scheme === "dark" && base.tint.toLowerCase() > "#808080" ? "#0e0e0e" : lightScheme && scheme === "auto" ? "#f2f2f2" : base.tint;
  const tint = fill === undefined ? base.opacity : (look.tint ?? (fill.type === "wallpaper" ? base.opacity : fill.type === "image" ? 0.25 : 0));
  const blur = (fill?.type === "wallpaper" ? 14 : base.blur * 0.5) * K + 3;
  const brightness = fill?.type === "wallpaper" || fill?.type === "image" ? (fill.brightness ?? 1) : 1;

  const background = (() => {
    switch (fill?.type) {
      case "solid":
        return cssColor(fill.color, ctx);
      case "gradient":
        return gradientCss(fill, ctx);
      case "image":
        return "repeating-linear-gradient(45deg, #5a5a5a 0 4px, #474747 4px 8px)";
      default:
        return undefined;
    }
  })();

  const border = look.border?.rest;
  const ringWidth = Math.max(border?.width ?? (fill === undefined ? 1 : 0), 0) * K;
  const ring = paintCss(border?.color, ctx) ?? "rgb(255 255 255 / 0.12)";
  const shadow = look.shadow?.rest;
  const glow = look.glow?.rest;
  const shadows = [
    shadow && `${(shadow.x ?? 0) * K}px ${(shadow.y ?? 0) * K}px ${(shadow.blur ?? 0) * K}px ${(shadow.spread ?? 0) * K}px ${cssColor(shadow.color, ctx) ?? "rgb(0 0 0 / 0.4)"}`,
    glow && `0 0 ${(glow.size ?? 0) * K}px color-mix(in srgb, ${cssColor(glow.color, ctx) ?? ctx.accent} ${(glow.opacity ?? 0.5) * 100}%, transparent)`,
  ].filter((s): s is string => Boolean(s));

  return (
    <div
      className={`bp-surface relative ${className}`}
      data-corners={look.corners ?? "round"}
      style={{ borderRadius: square ? 0 : radius * K, boxShadow: shadows.join(", ") || undefined, ...style }}
    >
      <div className="absolute inset-0 overflow-hidden" style={{ borderRadius: "inherit" }}>
        <div className="absolute inset-0" style={{ background, backdropFilter: glass ? `blur(${blur}px) saturate(${look.saturation ?? 1.2})` : undefined }} />
        {tint > 0 && fill?.type !== "solid" && <div className="absolute inset-0" style={{ background: tintColor, opacity: tint }} />}
        {brightness !== 1 && <div className="absolute inset-0" style={{ background: brightness < 1 ? "#000" : "#fff", opacity: Math.abs(1 - brightness) * 0.5 }} />}
        {(look.noise ?? 0) > 0 && <div className="grain absolute inset-0" style={{ opacity: Math.min(1, (look.noise ?? 0) * 4) }} />}
      </div>
      {ringWidth > 0 && (
        <div
          className="pointer-events-none absolute inset-0"
          style={{
            borderRadius: "inherit",
            padding: Math.max(ringWidth, 0.6),
            background: ring,
            mask: "linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)",
            maskComposite: "exclude",
          }}
        />
      )}
      <div className="relative h-full w-full">{children}</div>
    </div>
  );
}

/** One module button or task icon in a state, drawn from `look.items`. */
function Item({ look, ctx, state = "rest", radius, className = "", children }: { look: Look; ctx: ColorContext; state?: State; radius: number; className?: string; children: ReactNode }) {
  const items = look.items ?? {};
  const merged: ItemState = { ...items.rest, ...(state === "rest" ? {} : items[state]) };
  const border: Border | undefined = merged.border;
  const glow = merged.glow;
  const shadows = [
    border?.width && typeof border.color === "string" && `inset 0 0 0 ${Math.max(border.width * K, 0.6)}px ${cssColor(border.color, ctx)}`,
    glow && `0 0 ${(glow.size ?? 0) * K}px color-mix(in srgb, ${cssColor(glow.color, ctx) ?? ctx.accent} ${(glow.opacity ?? 0.5) * 100}%, transparent)`,
  ].filter((s): s is string => Boolean(s));
  return (
    <span
      className={`inline-flex items-center justify-center ${className}`}
      style={{
        borderRadius: radius * K,
        background: paintCss(merged.background, ctx),
        color: cssColor(merged.color, ctx),
        boxShadow: shadows.join(", ") || undefined,
        transform: merged.scale !== undefined && merged.scale !== 1 ? `scale(${merged.scale})` : undefined,
      }}
    >
      {children}
    </span>
  );
}

// --- Top bar ---

function Module({ item, dense, accent }: { item: ModuleRef; dense: boolean; accent?: boolean }) {
  const { icon: Icon, sample } = describeModule(item);
  return (
    <span className="flex items-center gap-[2px] px-[3px] py-[2px] whitespace-nowrap">
      <Icon style={{ width: 8, height: 8, color: accent ? "var(--accent-sample, currentColor)" : undefined }} strokeWidth={2} />
      {!dense && sample && <span style={{ opacity: 0.85 }}>{sample}</span>}
    </span>
  );
}

/** Splits a module list at its breaks. */
const groups = (list: readonly ModuleRef[]): ModuleRef[][] =>
  list.reduce<ModuleRef[][]>((out, ref) => (ref === "|" ? [...out, []] : [...out.slice(0, -1), [...(out.at(-1) ?? []), ref]]), [[]]).filter((g) => g.length > 0);

function TopBar({ config, catalog, base, ctx, dense }: Shared) {
  const { top } = config;
  const look = top.look;
  const geometry = topGeometry(config, catalog);
  const islands = top.layout === "islands";
  const radius = look.radius ?? (top.layout === "strip" ? 0 : base.radius || 12);
  const itemRadius = look.items?.radius ?? 6;
  const hovered = top.left.find((m) => m !== "|" && m !== "start") ?? top.center[0];
  const active = top.right.findLast((m) => m !== "|");

  const zone = (list: ModuleRef[], align: "start" | "center" | "end") => {
    const parts = islands ? groups(list) : [list];
    return (
      <div className="flex items-center" style={{ gap: geometry.margin * K, justifyContent: align === "start" ? "flex-start" : align === "end" ? "flex-end" : "center" }}>
        {parts.map((part, i) => {
          const inner = (
            <div className="flex items-center" style={{ gap: geometry.spacing * K, padding: islands ? geometry.padding * K : 0 }}>
              {part.map((ref, j) =>
                ref === "|" ? (
                  <span key={j} className="h-2 w-px bg-current opacity-30" />
                ) : (
                  <Item key={`${ref}${j}`} look={look} ctx={ctx} radius={itemRadius} state={ref === active ? "active" : ref === hovered ? "hover" : "rest"}>
                    <Module item={ref} dense={dense} />
                  </Item>
                ),
              )}
            </div>
          );
          return islands ? (
            <Surface key={i} look={look} base={base} ctx={ctx} radius={radius} style={{ height: top.height * K }}>
              {inner}
            </Surface>
          ) : (
            <span key={i}>{inner}</span>
          );
        })}
      </div>
    );
  };

  const rail = (
    <div className="flex h-full items-center justify-between" style={{ padding: islands ? 0 : `0 ${geometry.padding * K}px` }}>
      {zone(top.left, "start")}
      {zone(top.center, "center")}
      {zone(top.right, "end")}
    </div>
  );

  // `width` only applies to the floating layout: full, hugging the modules, DIPs, or a share.
  const widthStyle: CSSProperties = (() => {
    if (top.layout !== "floating" || top.width === null || top.width === "full") return {};
    if (top.width === "auto") return { width: "fit-content", marginInline: "auto" };
    const percent = typeof top.width === "string" ? Number.parseFloat(top.width) : NaN;
    if (Number.isFinite(percent)) return { width: `${percent}%`, marginInline: "auto" };
    return typeof top.width === "number" ? { width: top.width * K, marginInline: "auto" } : {};
  })();
  const edge: CSSProperties = top.position === "top" ? { top: 0 } : { bottom: 0 };
  const inset = top.layout === "strip" ? 0 : geometry.margin * K;

  return (
    <div className="absolute inset-x-0" style={{ ...edge, padding: `${top.layout === "strip" ? 0 : inset}px ${inset}px` }}>
      {islands ? (
        <div style={{ height: top.height * K, padding: `0 ${inset}px` }}>{rail}</div>
      ) : (
        <Surface look={look} base={base} ctx={ctx} radius={radius} square={top.layout === "strip"} style={{ height: top.height * K, ...widthStyle }}>
          {rail}
        </Surface>
      )}
    </div>
  );
}

// --- Dock ---

function Indicator({ kind, state, ctx, look }: { kind: Resolved["dock"]["indicator"]; state: "running" | "active"; ctx: ColorContext; look: Look }) {
  if (kind === "none") return null;
  const color = cssColor(look.indicator?.[state], ctx) ?? (state === "active" ? ctx.text : "rgb(255 255 255 / 0.5)");
  const shapes: Record<Exclude<typeof kind, "none">, CSSProperties> = {
    dot: { width: state === "active" ? 4 : 3, height: state === "active" ? 4 : 3, borderRadius: 4 },
    bar: { width: 12, height: 1.5, borderRadius: 2 },
    glow: { width: 7, height: 3, borderRadius: 4, boxShadow: `0 0 4px 1px ${color}` },
    pill: { width: state === "active" ? 10 : 5, height: 2.5, borderRadius: 3 },
  };
  return <span className="block" style={{ background: color, ...shapes[kind] }} />;
}

function Dock({ config, catalog, base, ctx, dense, topBand }: Shared & { topBand: number }) {
  const { dock } = config;
  const look = dock.look;
  const geometry = dockGeometry(config, catalog);
  const vertical = dock.position === "left" || dock.position === "right";
  const taskbar = dock.style === "taskbar";
  const magnify = magnifyOf(dock.magnify);
  const radius = look.radius ?? (base.radius || 16);
  const itemRadius = look.items?.radius ?? 8;
  const size = geometry.size * K;
  const icon = Math.max(size * 0.62, 7);
  const pointed = 2;

  const side: CSSProperties =
    dock.position === "bottom" ? { bottom: 0, left: 0, right: 0, justifyContent: "center", alignItems: "flex-end", paddingBottom: geometry.margin * K }
    : dock.position === "top" ? { top: topBand, left: 0, right: 0, justifyContent: "center", alignItems: "flex-start", paddingTop: geometry.margin * K }
    : dock.position === "left" ? { left: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "flex-start", paddingLeft: geometry.margin * K }
    : { right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "flex-end", paddingRight: geometry.margin * K };

  const tasks = (
    <div className="flex items-center" style={{ flexDirection: vertical ? "column" : "row", gap: geometry.spacing * K }}>
      {DOCK_APPS.map((App, i) => {
        // The cosine falloff of the magnify around the pointer (docs/bars.md, "Dock").
        const distance = Math.abs(i - pointed);
        const lift = !taskbar && magnify.enabled && distance < magnify.range ? 1 + (magnify.scale - 1) * Math.cos((Math.PI / 2) * (distance / magnify.range)) : 1;
        const state: State = i === 0 ? "active" : i === pointed ? "hover" : "rest";
        return (
          <div key={i} className="flex flex-col items-center" style={{ gap: 1.5, flexDirection: vertical ? "row" : "column" }}>
            <Item look={look} ctx={ctx} state={state} radius={itemRadius} className="origin-bottom">
              <span className="grid place-items-center" style={{ width: size * lift, height: size * lift, background: "rgb(128 128 128 / 0.22)", borderRadius: itemRadius * K }}>
                <App style={{ width: icon * lift, height: icon * lift }} strokeWidth={1.8} />
              </span>
              {taskbar && dock.titles && !dense && !vertical && <span className="px-1">App</span>}
            </Item>
            {i < 3 && <Indicator kind={dock.indicator} state={i === 0 ? "active" : "running"} ctx={ctx} look={look} />}
          </div>
        );
      })}
      {dock.extras.map((extra) => (
        <span key={extra} className="grid place-items-center opacity-70" style={{ width: size * 0.8, height: size * 0.8, border: "1px dashed currentColor", borderRadius: 3 }}>
          <span style={{ fontSize: 5 }}>{extra === "trash" ? "bin" : "d"}</span>
        </span>
      ))}
    </div>
  );

  if (!taskbar) {
    return (
      <div className="absolute flex" style={side}>
        <Surface look={look} base={base} ctx={ctx} radius={radius} style={{ padding: geometry.padding * K }}>
          {tasks}
        </Surface>
      </div>
    );
  }

  const cluster = (list: ModuleRef[]) => (
    <div className="flex items-center" style={{ flexDirection: vertical ? "column" : "row", gap: geometry.spacing * K }}>
      {list.map((ref, i) =>
        ref === "|" ? (
          <span key={i} className={vertical ? "h-px w-2 bg-current opacity-30" : "h-2 w-px bg-current opacity-30"} />
        ) : (
          <Item key={`${ref}${i}`} look={look} ctx={ctx} radius={itemRadius}>
            <Module item={ref} dense={dense} />
          </Item>
        ),
      )}
    </div>
  );

  // The full-width taskbar: `left` modules, task buttons (centered or at the start), `right` modules.
  return (
    <div className="absolute flex" style={{ ...side, padding: 0 }}>
      <Surface look={look} base={base} ctx={ctx} radius={0} square style={{ [vertical ? "height" : "width"]: "100%", [vertical ? "width" : "height"]: (size + 2 * geometry.padding * K) }}>
        <div className="flex h-full w-full items-center justify-between" style={{ flexDirection: vertical ? "column" : "row", padding: vertical ? `${geometry.padding * K}px 0` : `0 ${geometry.padding * K}px` }}>
          {dock.align === "center" ? (
            <>
              {cluster(dock.left)}
              {tasks}
              {cluster(dock.right)}
            </>
          ) : (
            <>
              <div className="flex items-center" style={{ flexDirection: vertical ? "column" : "row", gap: geometry.spacing * K * 2 }}>
                {cluster(dock.left)}
                {tasks}
              </div>
              {cluster(dock.right)}
            </>
          )}
        </div>
      </Surface>
    </div>
  );
}
