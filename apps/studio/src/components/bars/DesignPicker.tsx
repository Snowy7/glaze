import { Check } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";

import { useBars } from "../../lib/barsContext";
import { DOCK_DESIGNS, TOP_DESIGNS } from "../../lib/barsNames";
import { DOCK_DESIGNS as DOCK_IDS, TOP_DESIGNS as TOP_IDS, type BarKind, type BarsDoc } from "../../lib/barsSchema";
import { BarFrame } from "./BarFrame";

/** Each thumbnail is the real bar page in a frame this many DIPs wide, scaled to the tile. */
const FRAME_WIDTH = 1000;
const FRAME_HEIGHT: Record<BarKind, number> = { top: 76, dock: 128 };

/**
 * One bar's designs, each drawn by the real bars page with that design (the file's other settings
 * still on top, on your wallpaper). Choosing one writes `<bar>.design`.
 */
export function DesignPicker({ bar, wallpaper }: { bar: BarKind; wallpaper: string | null }) {
  const { doc, resolved, set } = useBars();
  const current = resolved[bar].design;
  const ids: readonly string[] = bar === "top" ? TOP_IDS : DOCK_IDS;
  const names: Record<string, { name: string; blurb: string }> = bar === "top" ? TOP_DESIGNS : DOCK_DESIGNS;

  return (
    <div role="radiogroup" aria-label={bar === "top" ? "Top bar design" : "Bottom bar design"} data-setting={`bars.${bar}.design`} className="setting-anchor flex flex-col gap-2">
      {ids.map((id) => {
        const on = id === current;
        const info = names[id];
        return (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={on}
            title={info?.blurb}
            onClick={() => !on && set([bar, "design"], id, `Use the ${info?.name ?? id} design`)}
            className={`group cursor-default rounded-[10px] p-1 text-left transition-[box-shadow,background-color] duration-150 ${
              on ? "bg-surface-top ring-2 ring-ink" : "bg-surface ring-1 ring-line-strong hover:bg-surface-hi hover:ring-line-hi"
            }`}
          >
            <Thumbnail bar={bar} doc={withDesign(doc, bar, id)} wallpaper={wallpaper} />
            <span className="flex items-center gap-1.5 px-1 pt-1.5 pb-0.5">
              <span className={`min-w-0 flex-1 truncate text-[12px] font-medium ${on ? "text-ink" : "text-ink-2"}`}>{info?.name ?? id}</span>
              {on && <Check className="size-3.5 shrink-0 text-ink" aria-hidden />}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** The file with this bar shown in design `id`, on its usual edge, so the designs compare side by side. */
function withDesign(doc: BarsDoc, bar: BarKind, id: string): BarsDoc {
  if (bar === "top") return { ...doc, top: { ...doc.top, design: TOP_IDS.find((d) => d === id), enabled: true, position: "top" } };
  return { ...doc, dock: { ...doc.dock, design: DOCK_IDS.find((d) => d === id), enabled: true, position: "bottom", autohide: false } };
}

/** A live, scaled-down bar, mounted once it scrolls into view. */
function Thumbnail({ bar, doc, wallpaper }: { bar: BarKind; doc: BarsDoc; wallpaper: string | null }) {
  const box = useRef<HTMLSpanElement>(null);
  const [width, setWidth] = useState(0);
  const [seen, setSeen] = useState(false);

  useEffect(() => {
    const element = box.current;
    if (!element) return;
    const resize = new ResizeObserver(([entry]) => setWidth(entry?.contentRect.width ?? 0));
    const visible = new IntersectionObserver(([entry]) => entry?.isIntersecting && setSeen(true), { rootMargin: "200px" });
    resize.observe(element);
    visible.observe(element);
    return () => {
      resize.disconnect();
      visible.disconnect();
    };
  }, []);

  const scale = width / FRAME_WIDTH;
  const height = FRAME_HEIGHT[bar];
  const inner: CSSProperties = { width: FRAME_WIDTH, height, transform: `scale(${scale})` };
  return (
    <span ref={box} className="relative block overflow-hidden rounded-md bg-black" style={{ height: height * scale }} aria-hidden>
      <span className="absolute inset-0 bg-cover bg-center" style={wallpaper ? { backgroundImage: `url("${wallpaper}")` } : undefined} />
      {seen && width > 0 && (
        <span className="absolute top-0 left-0 origin-top-left" style={inner}>
          <BarFrame bar={bar} doc={doc} width={FRAME_WIDTH} height={height} wallpaper={wallpaper} />
        </span>
      )}
    </span>
  );
}
