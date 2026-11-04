// The stage: a monitor-shaped frame showing the desktop at its real geometry. Layers are laid out
// in DIPs (the units the hosts use) inside a box the size of the screen, and the whole box is
// scaled to fit, so a widget 3 cells wide is exactly where and as big as it is on the desktop.

import { createContext, use, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import type { Box } from "../../lib/desktopGrid";

/** The stage's scale (screen DIPs to page pixels), for outlines and labels that keep their size. */
const ScaleContext = createContext(1);

export const useStageScale = () => use(ScaleContext);

type StageProps = {
  /** The screen, in DIPs. */
  screen: { width: number; height: number };
  children: ReactNode;
  /** A click on nothing selectable. */
  onBackground?: () => void;
  /** Over the stage's top edge: monitor switcher and the like. */
  tools?: ReactNode;
  /** Under the screen: a strip of quick choices. */
  footer?: ReactNode;
  label: string;
};

export function Stage({ screen, children, onBackground, tools, footer, label }: StageProps) {
  const area = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const element = area.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry?.contentRect;
      if (box) setFit({ width: box.width, height: box.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const k = Math.max(0.05, Math.min(fit.width / screen.width, fit.height / screen.height));
  const size: CSSProperties = { width: screen.width * k, height: screen.height * k };
  const inner: CSSProperties & Record<"--k", string> = { width: screen.width, height: screen.height, transform: `scale(${k})`, "--k": String(k) };

  return (
    <div className="stage-backdrop relative flex min-h-0 flex-1 flex-col">
      {tools && <div className="absolute inset-x-0 top-0 z-20 flex items-center justify-between gap-2 px-(--gutter) pt-3">{tools}</div>}
      <div ref={area} className={`mx-(--gutter) min-h-0 flex-1 ${tools ? "mt-14" : "mt-5"} ${footer ? "mb-3" : "mb-5"}`}>
        <div className="grid size-full place-items-center">
          <div
            role="group"
            aria-label={label}
            className="relative overflow-hidden rounded-[10px] bg-black shadow-[0_0_0_1px_rgb(255_255_255/0.09),0_0_0_6px_rgb(0_0_0/0.55),0_0_0_7px_rgb(255_255_255/0.06),0_30px_70px_-20px_rgb(0_0_0/0.9)] transition-[width,height] duration-200"
            style={size}
            onPointerDown={(event) => {
              if (event.target === event.currentTarget || (event.target as HTMLElement).dataset.stageBackground !== undefined) onBackground?.();
            }}
          >
            <ScaleContext value={k}>
              <div className="absolute top-0 left-0 origin-top-left" style={inner}>
                {children}
              </div>
            </ScaleContext>
          </div>
        </div>
      </div>
      {footer && <div className="shrink-0 px-(--gutter) pb-4">{footer}</div>}
    </div>
  );
}

/** The wallpaper layer: the theme's picture, or its placeholder art. Clicks on it count as the background unless `onSelect` is given. */
export function WallpaperLayer({ children, selected = false, onSelect }: { children: ReactNode; selected?: boolean; onSelect?: () => void }) {
  return (
    <div className="absolute inset-0" data-stage-background={onSelect ? undefined : ""} onClick={onSelect}>
      <div className="pointer-events-none absolute inset-0">{children}</div>
      {selected && <Outline box={null} selected label="Wallpaper" inset />}
    </div>
  );
}

/**
 * The hover and selection frame around a selectable thing, drawn at a constant screen width
 * whatever the stage's scale. `box` null: the whole stage.
 */
export function Outline({
  box,
  selected,
  hovered = false,
  label,
  radius = 0,
  inset = false,
  below = false,
}: {
  box: Box | null;
  selected: boolean;
  hovered?: boolean;
  label?: string;
  radius?: number;
  inset?: boolean;
  /** The name tag under the frame instead of over it (for things at the screen's top edge). */
  below?: boolean;
}) {
  const k = useStageScale();
  if (!selected && !hovered) return null;
  const pad = inset ? 0 : 3 / k;
  const style: CSSProperties = box
    ? { left: box.x - pad, top: box.y - pad, width: box.width + 2 * pad, height: box.height + 2 * pad, borderRadius: radius + pad }
    : { inset: 0 };
  return (
    <div className="pointer-events-none absolute z-30" style={style} aria-hidden>
      <div
        className="absolute inset-0"
        style={{
          borderRadius: "inherit",
          boxShadow: selected ? `0 0 0 ${2 / k}px #fff, 0 0 0 ${3.5 / k}px rgb(0 0 0 / 0.45)` : `0 0 0 ${1.25 / k}px rgb(255 255 255 / 0.7)`,
        }}
      />
      {selected && label && (
        <span
          className="absolute left-0 rounded bg-white font-sans font-semibold whitespace-nowrap text-black shadow"
          style={{ [below ? "top" : "bottom"]: `calc(100% + ${6 / k}px)`, fontSize: 11 / k, lineHeight: 1, padding: `${4 / k}px ${6 / k}px`, borderRadius: 4 / k }}
        >
          {label}
        </span>
      )}
    </div>
  );
}

/** A small floating control group over the stage (monitor switcher, toggles). */
export function StageTools({ children }: { children: ReactNode }) {
  return <div className="flex items-center gap-1 rounded-lg bg-surface/80 p-1 shadow-pop ring-1 ring-line-strong backdrop-blur-xl">{children}</div>;
}
