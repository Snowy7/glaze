// The stage: a monitor-shaped frame showing the desktop at its real geometry. Layers are laid out
// in DIPs (the units the hosts use) inside a box the size of the screen, and the whole box is
// scaled to fit, so a widget 3 cells wide is exactly where and as big as it is on the desktop.

import { createContext, use, useCallback, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import type { Box } from "../../lib/desktopGrid";

/** The stage's scale (screen DIPs to page pixels), for outlines and labels that keep their size. */
const ScaleContext = createContext(1);

export const useStageScale = () => use(ScaleContext);

/** Scrolls a zoomed stage so a box (in DIPs) is in view (its start, when it's wider than the view). */
const RevealContext = createContext<(box: Box) => void>(() => {});

export const useStageReveal = () => use(RevealContext);

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
  /**
   * The least scale to draw at. When the screen doesn't fit at it, the stage keeps its size and
   * scrolls (see `useStageReveal`), so small things stay big enough to point at.
   */
  zoom?: number;
};

export function Stage({ screen, children, onBackground, tools, footer, label, zoom }: StageProps) {
  const area = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const view = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState({ width: 0, height: 0, footer: 0 });

  useLayoutEffect(() => {
    const element = area.current;
    if (!element) return;
    const measure = () => setFit({ width: element.clientWidth, height: element.clientHeight, footer: strip.current?.offsetHeight ?? 0 });
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    if (strip.current) observer.observe(strip.current);
    return () => observer.disconnect();
  }, []);

  // The screen and the strip under it are one column, centered, as wide as the screen.
  const room = fit.height - (footer ? fit.footer + FOOTER_GAP : 0);
  const fitted = Math.max(0.05, Math.min(fit.width / screen.width, room / screen.height));
  const k = zoom !== undefined && zoom > fitted ? zoom : fitted;
  const zoomed = k > fitted;
  const size: CSSProperties = { width: screen.width * fitted, height: screen.height * fitted };
  const inner: CSSProperties & Record<"--k", string> = { width: screen.width, height: screen.height, transform: `scale(${k})`, "--k": String(k) };

  const scale = useRef(k);
  scale.current = k;
  const reveal = useCallback((box: Box) => {
    const element = view.current;
    if (!element) return;
    const s = scale.current;
    const clamp = (value: number, max: number) => Math.max(0, Math.min(max, value));
    element.scrollTo({
      left: clamp(box.width * s > element.clientWidth ? box.x * s - 16 : (box.x + box.width / 2) * s - element.clientWidth / 2, element.scrollWidth - element.clientWidth),
      top: clamp((box.y + box.height / 2) * s - element.clientHeight / 2, element.scrollHeight - element.clientHeight),
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth",
    });
  }, []);

  return (
    <div className="stage-backdrop relative flex min-h-0 flex-1 flex-col">
      {tools && <div className="flex shrink-0 items-center justify-between gap-2 px-(--gutter) pt-3">{tools}</div>}
      <div ref={area} className="mx-(--gutter) my-4 min-h-0 flex-1">
        <div className="flex size-full flex-col items-center justify-center" style={{ gap: FOOTER_GAP }}>
          <div
            ref={view}
            role="group"
            aria-label={label}
            className={`relative shrink-0 rounded-lg bg-black shadow-[0_0_0_1px_var(--line-hi),0_16px_40px_-16px_rgb(0_0_0/0.8)] ${zoomed ? "overflow-auto [scrollbar-width:thin]" : "overflow-hidden"}`}
            style={size}
            onPointerDown={(event) => {
              const target = event.target;
              if (target === event.currentTarget || (target instanceof HTMLElement && target.dataset.stageBackground !== undefined)) onBackground?.();
            }}
          >
            <ScaleContext value={k}>
              <RevealContext value={reveal}>
                {/* As big as the scaled screen, so a zoomed stage scrolls over all of it. */}
                <div className="relative" style={{ width: screen.width * k, height: screen.height * k }}>
                  <div className="absolute top-0 left-0 origin-top-left" style={inner}>
                    {children}
                  </div>
                </div>
              </RevealContext>
            </ScaleContext>
          </div>
          {footer && (
            <div ref={strip} className="min-w-0 shrink-0" style={{ width: size.width }}>
              {footer}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Between the screen and the strip under it. */
const FOOTER_GAP = 16;

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
  end = false,
}: {
  box: Box | null;
  selected: boolean;
  hovered?: boolean;
  label?: string;
  radius?: number;
  inset?: boolean;
  /** The name tag under the frame instead of over it (for things at the screen's top edge). */
  below?: boolean;
  /** The name tag at the frame's right end (for things near the screen's right edge). */
  end?: boolean;
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
          className={`absolute ${end ? "right-0" : "left-0"} rounded bg-white font-sans font-semibold whitespace-nowrap text-black shadow`}
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
