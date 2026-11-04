// Liquid Glass (docs/bars.md, "Liquid glass"): what's behind a pane, bent by a thick convex lens
// along its rim and nearly clear in the middle, with chromatic fringing where it bends and one thin
// specular edge.
//
// The host copies what's behind (the `capture` request: the screen without our windows, so the
// wallpaper's current frame over the desktop, or the windows behind a flyout). Each pane (an
// element whose computed `--liquid` is 1: style.css sets it on surfaces and flyouts while
// `data-liquid` is on) gets a `.lens` layer showing that copy aligned with the screen, through
// an SVG filter: a displacement map made from the pane's rounded-rect distance field (outward at
// the rim, neutral inside the bezel), applied per color channel at slightly different strengths.
// A `.shine` layer on top draws the rim light.
//
// Cost: a map is drawn when a pane's size changes; a capture when it appears, moves, or the host
// says what's behind changed (`behind`). Between those nothing runs: Chromium keeps the filtered
// layer as it is.

import { request } from "./host";

/** What can be a pane (the same elements as live blur, glass.ts). */
const CANDIDATES = ".surface, .layer";
const SVG_NS = "http://www.w3.org/2000/svg";
/** Chromatic fringing: green and blue bend this much more than red. */
const FRINGE = [1, 1.045, 1.09] as const;
/** Map pixels at most; a bigger pane's map is drawn smaller and stretched (it's smooth). */
const MAP_PIXELS = 160_000;

export interface LensShape {
  /** The pane's size (DIPs). */
  width: number;
  height: number;
  radius: number;
  /** How far the lens reaches in from the rim (DIPs). */
  bezel: number;
  /** Edges flush with the screen: no rim there (a strip along the top, a taskbar along the bottom). */
  flush: { top: boolean; right: boolean; bottom: boolean; left: boolean };
  /** Room around the pane in the map, so displaced pixels come from outside it (DIPs). */
  margin: number;
}

/**
 * The displacement map for `shape` at `scale` (map pixels per DIP): RGBA, red and green the x
 * and y offset (128 is none), pointing out of the pane, strongest at the rim and fading to none
 * across the bezel with a convex lens's profile. Pure, so it can be tested.
 */
export function lensMap(shape: LensShape, scale: number): { width: number; height: number; data: Uint8ClampedArray<ArrayBuffer> } {
  const { width, height, radius, bezel, flush, margin } = shape;
  const mw = Math.max(1, Math.round((width + 2 * margin) * scale));
  const mh = Math.max(1, Math.round((height + 2 * margin) * scale));
  const data = new Uint8ClampedArray(new ArrayBuffer(mw * mh * 4));
  // A flush edge is pushed far out, so its distance never counts.
  const far = 1e5;
  const left = flush.left ? -far : 0;
  const right = flush.right ? width + far : width;
  const top = flush.top ? -far : 0;
  const bottom = flush.bottom ? height + far : height;
  const hw = (right - left) / 2;
  const hh = (bottom - top) / 2;
  const cx = (left + right) / 2;
  const cy = (top + bottom) / 2;
  const r = Math.min(radius, width / 2, height / 2);
  for (let j = 0; j < mh; j++) {
    for (let i = 0; i < mw; i++) {
      const x = (i + 0.5) / scale - margin;
      const y = (j + 0.5) / scale - margin;
      let dx = 0;
      let dy = 0;
      if (x >= 0 && y >= 0 && x <= width && y <= height) {
        // Signed distance to the rounded rect, and the outward normal.
        const px = x - cx;
        const py = y - cy;
        const qx = Math.abs(px) - (hw - r);
        const qy = Math.abs(py) - (hh - r);
        let distance: number;
        let nx: number;
        let ny: number;
        if (qx > 0 && qy > 0) {
          const len = Math.hypot(qx, qy) || 1;
          distance = r - len;
          nx = (qx / len) * Math.sign(px);
          ny = (qy / len) * Math.sign(py);
        } else if (qx > qy) {
          distance = r - qx;
          nx = Math.sign(px);
          ny = 0;
        } else {
          distance = r - qy;
          nx = 0;
          ny = Math.sign(py);
        }
        const t = Math.min(1, Math.max(0, distance / bezel));
        // A convex bezel: steepest at the rim, flat by the time it reaches the clear middle.
        const strength = (1 - t) ** 1.5;
        dx = nx * strength;
        dy = ny * strength;
      }
      const at = (j * mw + i) * 4;
      data[at] = 128 + dx * 127;
      data[at + 1] = 128 + dy * 127;
      data[at + 2] = 128;
      data[at + 3] = 255;
    }
  }
  return { width: mw, height: mh, data };
}

/** The material knobs, read from the pane's computed style (style/look.ts sets them). */
interface Material {
  intensity: number;
  refraction: number;
  blur: number;
}

interface Pane {
  el: HTMLElement;
  lens: HTMLElement;
  shine: HTMLElement;
  filter: SVGFilterElement;
  /** What the map was drawn for. */
  key: string;
  /** The pane's layout box when captured (window DIPs), and the capture. */
  captured?: { box: [number, number, number, number]; url: string; rect: [number, number, number, number] };
  capturing: boolean;
  /** A capture was asked for while one was in flight. */
  again: boolean;
  margin: number;
}

const number = (style: CSSStyleDeclaration, name: string, fallback: number): number => {
  const value = Number.parseFloat(style.getPropertyValue(name));
  return Number.isFinite(value) ? value : fallback;
};

/** The element's layout box in window DIPs, ignoring transforms (its own and its ancestors'). */
function layoutBox(el: HTMLElement): [number, number, number, number] {
  let x = 0;
  let y = 0;
  for (let node: HTMLElement | null = el; node; node = node.offsetParent as HTMLElement | null) {
    x += node.offsetLeft;
    y += node.offsetTop;
  }
  return [x, y, el.offsetWidth, el.offsetHeight];
}

export interface Liquid {
  /** Layout may have changed: look for new, resized, moved and gone panes. */
  invalidate(): void;
  /** What's behind may have changed (the host's `behind`): capture every pane again. */
  recapture(): void;
}

export function createLiquid(): Liquid {
  const defs = document.createElementNS(SVG_NS, "svg");
  defs.setAttribute("aria-hidden", "true");
  defs.style.cssText = "position:absolute;width:0;height:0;overflow:hidden";
  document.body.append(defs);
  const panes = new Map<HTMLElement, Pane>();
  const resize = new ResizeObserver(() => invalidate());
  let next = 0;
  let frame = 0;
  let mapTimer = 0;

  const capture = (pane: Pane) => {
    if (pane.capturing) {
      pane.again = true;
      return;
    }
    const box = layoutBox(pane.el);
    const m = pane.margin;
    pane.capturing = true;
    request("capture", { rect: [box[0] - m, box[1] - m, box[2] + 2 * m, box[3] + 2 * m] })
      .then(async ({ url, rect }) => {
        const image = new Image();
        image.src = url;
        await image.decode();
        pane.captured = { box, url, rect };
        place(pane);
        pane.el.classList.add("lensed");
      })
      .catch((error: unknown) => console.warn("[bars] liquid capture failed", error))
      .finally(() => {
        pane.capturing = false;
        if (pane.again) {
          pane.again = false;
          capture(pane);
        }
      });
  };

  /** Lines the capture up with the screen inside the lens (which starts `margin` before the pane). */
  const place = (pane: Pane) => {
    const shot = pane.captured;
    if (!shot) return;
    const [bx, by] = layoutBox(pane.el);
    const [x, y, w, h] = shot.rect;
    const m = pane.margin;
    const s = pane.lens.style;
    s.backgroundImage = `url("${shot.url}")`;
    s.backgroundSize = `${w}px ${h}px`;
    s.backgroundPosition = `${x - (bx - m)}px ${y - (by - m)}px`;
  };

  /** Draws the pane's map and filter for its current size and material. */
  const draw = (pane: Pane, style: CSSStyleDeclaration) => {
    const { el } = pane;
    const width = el.offsetWidth;
    const height = el.offsetHeight;
    const material: Material = {
      intensity: number(style, "--lq-intensity", 1),
      refraction: number(style, "--lq-refraction", 0.6),
      blur: number(style, "--lq-blur", 1),
    };
    const radius = Math.min(Number.parseFloat(style.borderTopLeftRadius) || 0, width / 2, height / 2);
    const [bx, by] = layoutBox(el);
    const short = Math.min(width, height);
    const bezel = Math.max(6, Math.min(radius * 1.3 + 10, short / 2));
    const reach = material.refraction * material.intensity * bezel * 1.6;
    // An edge closer to the window's (the screen's, for a bar) than the lens reaches would bend in
    // pixels nobody has: it gets no rim, like an edge flush with the screen.
    const edges = {
      top: by < reach,
      left: bx < reach,
      right: bx + width > window.innerWidth - reach,
      bottom: by + height > window.innerHeight - reach,
    };
    const margin = Math.ceil(reach * 1.2 + material.blur * 3 + 4);
    const key = JSON.stringify([width, height, radius, edges, reach, material.blur]);
    if (key === pane.key) return;
    pane.key = key;
    pane.margin = margin;
    const shape: LensShape = { width, height, radius, bezel, flush: edges, margin };
    const total = (width + 2 * margin) * (height + 2 * margin);
    const scale = Math.min(1, Math.sqrt(MAP_PIXELS / Math.max(1, total)));
    const map = lensMap(shape, scale);
    const canvas = document.createElement("canvas");
    canvas.width = map.width;
    canvas.height = map.height;
    canvas.getContext("2d")?.putImageData(new ImageData(map.data, map.width, map.height), 0, 0);
    const href = canvas.toDataURL("image/png");
    const w = width + 2 * margin;
    const h = height + 2 * margin;
    // Displacement is `scale × (channel − 0.5)`: twice the reach at full strength.
    const scales = FRINGE.map((k) => (2 * reach * k).toFixed(2));
    const keep = ["1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0", "0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0", "0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0"];
    pane.filter.innerHTML =
      (material.blur > 0 ? `<feGaussianBlur in="SourceGraphic" stdDeviation="${material.blur}" edgeMode="duplicate" result="src"/>` : "") +
      `<feImage href="${href}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="map"/>` +
      scales
        .map(
          (s, i) =>
            `<feDisplacementMap in="${material.blur > 0 ? "src" : "SourceGraphic"}" in2="map" scale="${s}" xChannelSelector="R" yChannelSelector="G" result="d${i}"/>` +
            `<feColorMatrix in="d${i}" type="matrix" values="${keep[i]}" result="c${i}"/>`,
        )
        .join("") +
      `<feComposite in="c0" in2="c1" operator="arithmetic" k2="1" k3="1" result="c01"/>` +
      `<feComposite in="c01" in2="c2" operator="arithmetic" k2="1" k3="1"/>`;
    pane.lens.style.inset = `${-margin}px`;
    place(pane);
  };

  const adopt = (el: HTMLElement): Pane => {
    const id = `lq-${++next}`;
    const filter = document.createElementNS(SVG_NS, "filter");
    filter.id = id;
    for (const [name, value] of [
      ["x", "0"],
      ["y", "0"],
      ["width", "1"],
      ["height", "1"],
      ["filterUnits", "objectBoundingBox"],
      ["primitiveUnits", "userSpaceOnUse"],
      ["color-interpolation-filters", "sRGB"],
    ] as const) {
      filter.setAttribute(name, value);
    }
    defs.append(filter);
    const lens = document.createElement("div");
    lens.className = "lens";
    lens.style.setProperty("--lens", `url(#${id})`);
    const shine = document.createElement("div");
    shine.className = "shine";
    // Under the content, over the surface's own fill (wallpaper glass stays as the fallback).
    const bg = el.querySelector(":scope > .bg");
    if (bg) bg.after(lens);
    else el.prepend(lens);
    el.append(shine);
    resize.observe(el);
    const pane: Pane = { el, lens, shine, filter, key: "", capturing: false, again: false, margin: 0 };
    panes.set(el, pane);
    return pane;
  };

  const drop = (pane: Pane) => {
    panes.delete(pane.el);
    resize.unobserve(pane.el);
    pane.filter.remove();
    pane.lens.remove();
    pane.shine.remove();
    pane.el.classList.remove("lensed");
  };

  const flush = () => {
    frame = 0;
    const seen = new Set<HTMLElement>();
    for (const el of document.querySelectorAll<HTMLElement>(CANDIDATES)) {
      const style = getComputedStyle(el);
      if (style.getPropertyValue("--liquid").trim() !== "1" || el.offsetWidth < 2 || el.offsetHeight < 2) continue;
      seen.add(el);
      const pane = panes.get(el) ?? adopt(el);
      draw(pane, style);
      const box = layoutBox(el);
      const was = pane.captured?.box;
      // Moved or grown past what was captured: capture again; otherwise just re-align.
      const moved = !was || Math.abs(was[0] - box[0]) > 1 || Math.abs(was[1] - box[1]) > 1 || box[2] > was[2] + 1 || box[3] > was[3] + 1;
      if (moved) capture(pane);
      else place(pane);
    }
    for (const pane of [...panes.values()]) if (!seen.has(pane.el)) drop(pane);
  };
  function invalidate() {
    // Resizes come in bursts (the dock's pill stretching): settle before redrawing maps.
    clearTimeout(mapTimer);
    mapTimer = window.setTimeout(() => {
      if (!frame) frame = requestAnimationFrame(flush);
    }, 60);
  }

  window.addEventListener("resize", invalidate);

  return {
    invalidate,
    recapture: () => {
      for (const pane of panes.values()) capture(pane);
    },
  };
}
