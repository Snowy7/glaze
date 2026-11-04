import { useEffect, useRef, useState, type CSSProperties } from "react";

import { readGeometry, union, type BarGeometry } from "../../lib/barsGeometry";
import type { BarKind, BarsDoc } from "../../lib/barsSchema";

// The real bars page (bars/shell, copied into Studio as bars-shell/), live, in a same-origin frame.
// Its own sample host (`?embed`) draws one bar with sample data; Studio posts the bars.json to
// show, paints the wallpaper behind its glass, and reads where its bands and modules are. The
// page never takes the pointer: the stage handles selecting and dragging.

export const BARS_SHELL = "bars-shell/index.html";

let available: Promise<boolean> | null = null;

/** Whether this build of Studio has the bars page (vite.config.ts copies it when it's built). */
export function barsShellAvailable(): Promise<boolean> {
  available ??= fetch(BARS_SHELL, { method: "HEAD" }).then((r) => r.ok, () => false);
  return available;
}

/** True once known to be there, false when it isn't, null while checking. */
export function useBarsShell(): boolean | null {
  const [state, setState] = useState<boolean | null>(null);
  useEffect(() => {
    let current = true;
    void barsShellAvailable().then((ok) => current && setState(ok));
    return () => {
      current = false;
    };
  }, []);
  return state;
}

type Props = {
  bar: BarKind;
  /** The bars.json to show; the page resolves it like the host (designs, preset, the file). */
  doc: BarsDoc;
  /** The frame in DIPs: the whole screen on the stage, a strip for a design thumbnail. */
  width: number;
  height: number;
  /** The wallpaper behind the glass, as a data URL (the page only loads its own files and data). */
  wallpaper: string | null;
  /** Clipped to its bars, so a full-screen frame doesn't cover what's under it. */
  clip?: boolean;
  /** Called with the page's layout whenever it changes. */
  onGeometry?: (geometry: BarGeometry) => void;
  className?: string;
  style?: CSSProperties;
};

const STYLE_ID = "studio-preview";
/** Room around the bars' surfaces for their shadows and glow. */
const CLIP_PAD = 28;
/** How far the clipped frame fades out at its edges (inside CLIP_PAD, so the bars themselves never fade). */
const EDGE_FADE = 16;

/**
 * Fades a clipped frame out at each edge that's inside the screen. A hard edge of a frame on a
 * scaled stage draws a dark one-pixel seam; faded, its edge pixels are transparent. An edge on the
 * screen's own edge (a strip bar's top) stays hard.
 */
function edgeFade(box: { x: number; y: number; w: number; h: number }, width: number, height: number): CSSProperties {
  const along = (direction: string, start: boolean, end: boolean) =>
    `linear-gradient(${direction}, ${start ? `transparent, #000 ${EDGE_FADE}px` : "#000"}, ${end ? `#000 calc(100% - ${EDGE_FADE}px), transparent` : "#000"})`;
  const mask = `${along("to bottom", box.y > 0, box.y + box.h < height)}, ${along("to right", box.x > 0, box.x + box.w < width)}`;
  return { maskImage: mask, WebkitMaskImage: mask, maskComposite: "intersect", WebkitMaskComposite: "source-in" };
}
const READY_TRIES = 100;

/** The page's look over its own: the wallpaper behind the glass, and no pointer. */
function paint(doc: Document, wallpaper: string | null) {
  let style = doc.getElementById(STYLE_ID);
  if (!style) {
    style = doc.createElement("style");
    style.id = STYLE_ID;
    doc.head.append(style);
  }
  const picture = wallpaper ? `url("${wallpaper}")` : "none";
  style.textContent =
    `html { background: center / cover no-repeat ${picture}, #101012 !important; --backdrop: ${picture} !important; --backdrop-size: cover !important; --backdrop-pos: center !important; }` +
    `html, body, * { pointer-events: none !important; cursor: default !important; }`;
}

const post = (frame: HTMLIFrameElement, doc: BarsDoc) => frame.contentWindow?.postMessage({ type: "mypaper-bars", bars: doc }, location.origin);

export function BarFrame({ bar, doc, width, height, wallpaper, clip = false, onGeometry, className = "", style }: Props) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [ready, setReady] = useState(false);
  const [bands, setBands] = useState<string | null>(null);
  const latest = useRef({ doc, wallpaper, onGeometry });
  latest.current = { doc, wallpaper, onGeometry };
  // The first draw uses the query (close to the doc), so the page doesn't flash another design.
  const [src] = useState(() => {
    const q = new URLSearchParams({ bar, embed: "", ...(doc.preset && { preset: doc.preset }), ...(doc.top?.design && { top: doc.top.design }), ...(doc.dock?.design && { dock: doc.dock.design }) });
    return `${BARS_SHELL}?${q.toString()}`;
  });

  /** Reads the layout, a few times while the bar's show animation settles. */
  const measure = useRef(() => {});
  measure.current = () => {
    const page = frame.current?.contentDocument;
    if (!page) return;
    const geometry = readGeometry(page, bar);
    const box = union(geometry.bands, CLIP_PAD);
    setBands(box ? `${box.x},${box.y},${box.width},${box.height}` : null);
    latest.current.onGeometry?.(geometry);
  };

  // Wait for the page's first draw (its host is up), then take it over.
  function onLoad() {
    let tries = 0;
    const timer = window.setInterval(() => {
      const page = frame.current?.contentDocument;
      const drawn = page?.querySelector("[data-band]");
      if (!page || (!drawn && ++tries < READY_TRIES)) return;
      window.clearInterval(timer);
      paint(page, latest.current.wallpaper);
      if (frame.current) post(frame.current, latest.current.doc);
      setReady(true);
    }, 50);
  }

  const key = JSON.stringify(doc);
  useEffect(() => {
    if (!ready || !frame.current) return;
    post(frame.current, latest.current.doc);
    const timers = [60, 300, 800].map((ms) => window.setTimeout(() => measure.current(), ms));
    return () => timers.forEach((t) => window.clearTimeout(t));
  }, [ready, key]);

  useEffect(() => {
    const page = frame.current?.contentDocument;
    if (ready && page) paint(page, wallpaper);
  }, [ready, wallpaper]);

  // Modules change size as their data changes (the clock, media titles): follow them.
  useEffect(() => {
    const page = frame.current?.contentDocument;
    if (!ready || !page) return;
    let pending = 0;
    const observer = new MutationObserver(() => {
      if (pending) return;
      pending = window.requestAnimationFrame(() => {
        pending = 0;
        measure.current();
      });
    });
    observer.observe(page.body, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(pending);
    };
  }, [ready]);

  const [x = 0, y = 0, w = 0, h = 0] = bands?.split(",").map((n) => Math.round(Number(n))) ?? [];
  const page = (
    <iframe
      ref={frame}
      src={src}
      title={bar === "top" ? "Top bar" : "Bottom bar"}
      tabIndex={-1}
      aria-hidden
      onLoad={onLoad}
      className={`pointer-events-none absolute top-0 left-0 border-0 transition-opacity duration-200 ${ready ? "opacity-100" : "opacity-0"} ${className}`}
      style={{ width, height, colorScheme: "normal", ...(clip && { left: -x, top: -y }), ...style }}
    />
  );
  if (!clip) return page;
  // Clipped to a box around its bars, faded at its edges.
  return (
    <div aria-hidden className="pointer-events-none absolute overflow-hidden" style={{ left: x, top: y, width: w, height: h, ...edgeFade({ x, y, w, h }, width, height) }}>
      {page}
    </div>
  );
}
