// Live blur, the page's side (docs/bars.md, "Live blur"). Windows blurs what's behind the window
// (crates/bars/src/blur.rs); the page says where: every element whose computed `--glass` is 1
// (style.css sets it on eligible glass surfaces and flyouts while live blur is on) is a
// pane, with its box on screen (transforms included), corner radius, opacity, and its material
// from `--glass-sat`, `--glass-bright` and `--glass-blur`. Panes go to the host whenever they
// change: on layout changes, on resizes, and every frame while an animation or transition runs
// (a flyout popping in, the dock's pill stretching as it magnifies), so the blur moves with them.

import type { Pane } from "./protocol";

const round = (v: number) => Math.round(v * 100) / 100;

/** What can be a pane: bar surfaces, overlay layers (flyouts, menus), the dock's label, and anything marked. */
const CANDIDATES = ".surface, .layer, .dock-label, [data-glass]";

/** Squircle corners bulge past a circle of the same radius; this circle hugs them closely. */
const SQUIRCLE = 0.62;

/** The element's opacity on screen: its own times its ancestors'. */
function opacityOf(el: Element): number {
  let opacity = 1;
  for (let node: Element | null = el; node && opacity > 0; node = node.parentElement) {
    opacity *= Number.parseFloat(getComputedStyle(node).opacity) || 0;
  }
  return opacity;
}

/** Every pane in the document now. Pure apart from reading layout. */
export function collectPanes(root: ParentNode = document): Pane[] {
  const panes: Pane[] = [];
  for (const el of root.querySelectorAll<HTMLElement>(CANDIDATES)) {
    const style = getComputedStyle(el);
    if (style.getPropertyValue("--glass").trim() !== "1" || style.display === "none" || style.visibility === "hidden") continue;
    const box = el.getBoundingClientRect();
    if (box.width < 1 || box.height < 1) continue;
    if (box.right < 0 || box.bottom < 0 || box.left > window.innerWidth || box.top > window.innerHeight) continue;
    const opacity = opacityOf(el);
    if (opacity <= 0.01) continue;
    // A scaled element (a flyout popping in) has its corners scaled too.
    const scale = el.offsetWidth > 0 ? box.width / el.offsetWidth : 1;
    const squircle = style.getPropertyValue("corner-shape").includes("squircle");
    const radius = (Number.parseFloat(style.borderTopLeftRadius) || 0) * scale * (squircle ? SQUIRCLE : 1);
    const number = (name: string, fallback: number) => {
      const value = Number.parseFloat(style.getPropertyValue(name));
      return Number.isFinite(value) ? value : fallback;
    };
    panes.push({
      rect: [round(box.left), round(box.top), round(box.width), round(box.height)],
      radius: round(radius),
      opacity: round(Math.min(1, opacity)),
      saturation: Math.min(3, Math.max(0, number("--glass-sat", 1))),
      brightness: Math.min(2, Math.max(0, number("--glass-bright", 1))),
      blur: Math.min(64, Math.max(0, number("--glass-blur", 0))),
    });
  }
  return panes;
}

export interface Glass {
  /** Call when layout may have changed (the region did, a flyout opened...). */
  invalidate(): void;
}

/** Reports panes through `send` whenever they change. */
export function createGlass(send: (panes: Pane[]) => void): Glass {
  let last = "";
  let frame = 0;
  const observed = new WeakSet<Element>();
  const resize = new ResizeObserver(() => invalidate());

  // An animation moves glass when it runs on a pane or on something holding one (a spinner inside
  // a flyout doesn't).
  const moving = () =>
    document.getAnimations().some((a) => {
      const target = a.effect instanceof KeyframeEffect ? a.effect.target : null;
      return a.playState === "running" && !!target && (target.matches(CANDIDATES) || !!target.querySelector(CANDIDATES));
    });
  const flush = () => {
    frame = 0;
    const panes = collectPanes();
    for (const el of document.querySelectorAll(CANDIDATES)) {
      if (!observed.has(el)) {
        observed.add(el);
        resize.observe(el);
      }
    }
    const key = JSON.stringify(panes);
    if (key !== last) {
      last = key;
      send(panes);
    }
    // Follow running animations frame by frame; stop the moment they're done.
    if (moving()) invalidate();
  };
  function invalidate() {
    if (!frame) frame = requestAnimationFrame(flush);
  }
  for (const type of ["animationstart", "transitionrun", "animationend", "transitionend", "transitioncancel"]) {
    document.addEventListener(type, invalidate, { capture: true, passive: true });
  }
  window.addEventListener("resize", invalidate);
  return { invalidate };
}
