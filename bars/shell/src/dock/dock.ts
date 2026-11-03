// The dock: a pill of app icons with hover magnify, context menus, drag to reorder / pin / unpin,
// and auto-hide. Geometry follows protocol.ts (DOCK_PAD, DOCK_GAP); pure logic is in
// model.ts, drag.ts and magnify.ts.

import type { Bar, BarContext } from "../bar";
import { glyphLetter, h, layoutBox, prefersReducedMotion, rectOf, viewport } from "../dom";
import { clamp, hoverExtent, inflate, placePopover, type Rect, type Side } from "../geometry";
import { createScope, fire } from "../host";
import { closeMenu, openMenu, type MenuEntry } from "../menu";
import { layerRects } from "../overlay";
import type { BarsConfig, DockItem } from "../protocol";
import {
  distanceOutside,
  DRAG_THRESHOLD,
  dropIndex,
  exceedsThreshold,
  FAR_DISTANCE,
  resolveDrop,
  slotShifts,
  type DropAction,
} from "./drag";
import { magnify } from "./magnify";
import { applyDrop, dividerIndex, indicator, orderItems } from "./model";

const ITEM_GAP = 6;
const MAGNIFY_MAX = 1.5;
/** Room above (or beside) a magnified icon for the name label. */
const LABEL_SPACE = 36;
const HIDE_DELAY = 1000;
const SLIDE_MS = 220;
/** Region margin around the pill, for its shadow. */
const PILL_SHADOW = 8;

interface Slot {
  item: DockItem;
  el: HTMLButtonElement;
  icon: HTMLElement;
  /** The icon URL currently drawn (so an unchanged icon isn't rebuilt). */
  iconSrc: string | undefined;
}

interface Drag {
  slot: Slot;
  from: number;
  pointerId: number;
  x0: number;
  y0: number;
  active: boolean;
  /** Centers of every item but the dragged one, and where the pinned group ends. */
  others: number[];
  boundary: number;
  to: number | null;
  far: boolean;
}

export function createDock(root: HTMLElement, config: BarsConfig, ctx: BarContext): Bar {
  const { size, position, autohide } = config.dock;
  const horizontal = position === "bottom";
  const pitch = size + ITEM_GAP;
  const scope = createScope();
  const magnifyOn = () => config.dock.magnify && !prefersReducedMotion();

  document.documentElement.setAttribute("data-position", position);
  const wrap = h("div", autohide ? "dock autohide" : "dock");
  wrap.style.setProperty("--size", `${size}px`);
  const pill = h("div", "pill");
  const track = h("div", "items");
  const divider = h("div", "divider");
  const label = h("div", "dock-label");
  pill.append(track);
  wrap.append(pill, label);
  root.append(wrap);

  const slots = new Map<string, Slot>();
  let items: DockItem[] = [];
  let centers: number[] = [];
  let ready = false;
  let hovered = false;
  let pointerInside = false;
  let menuOpen = false;
  let revealed = !autohide;
  let regionOn = !autohide;
  let hideTimer = 0;
  let suppressClick = false;
  let drag: Drag | undefined;
  let pendingItems: DockItem[] | undefined;

  // Axis helpers: the dock runs along x (bottom) or y (left/right).
  const along = (e: { clientX: number; clientY: number }) => (horizontal ? e.clientX : e.clientY);
  const across = (e: { clientX: number; clientY: number }) => (horizontal ? e.clientY : e.clientX);
  const alongStart = (r: DOMRect) => (horizontal ? r.left : r.top);
  const shiftBy = (v: number) => (horizontal ? `translate(${v}px, 0)` : `translate(0, ${v}px)`);
  const centerOf = (el: HTMLElement) =>
    horizontal ? el.offsetLeft + el.offsetWidth / 2 : el.offsetTop + el.offsetHeight / 2;

  // --- Items ---------------------------------------------------------------------------------------

  function paint(slot: Slot, item: DockItem) {
    slot.item = item;
    slot.el.setAttribute("aria-label", item.name);
    slot.el.dataset.state = indicator(item);
    if (slot.iconSrc === item.icon && slot.icon.firstChild) return;
    slot.iconSrc = item.icon;
    if (item.icon) {
      const img = h("img");
      img.src = item.icon;
      img.alt = "";
      img.draggable = false;
      slot.icon.replaceChildren(img);
    } else {
      slot.icon.replaceChildren(h("div", "ph", glyphLetter(item.name)));
    }
  }

  function makeSlot(item: DockItem): Slot {
    const icon = h("div", "icon");
    const el = h("button", "slot", icon, h("span", "dot"));
    el.type = "button";
    const slot: Slot = { item, el, icon, iconSrc: undefined };
    el.addEventListener("click", () => {
      if (!suppressClick) fire("activate", { app: slot.item.key });
    });
    el.addEventListener("mousedown", (e) => {
      if (e.button === 1) e.preventDefault();
    });
    el.addEventListener("auxclick", (e) => {
      if (e.button !== 1) return;
      e.preventDefault();
      fire("launch", { path: slot.item.path, newInstance: true });
    });
    el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      openItemMenu(slot);
    });
    el.addEventListener("pointerdown", (e) => beginPress(slot, e));
    return slot;
  }

  function setItems(next: readonly DockItem[]) {
    items = orderItems(next);
    const at = dividerIndex(items);
    const wanted: HTMLElement[] = [];
    const seen = new Set<string>();
    items.forEach((item, i) => {
      seen.add(item.key);
      let slot = slots.get(item.key);
      if (!slot) {
        slot = makeSlot(item);
        slots.set(item.key, slot);
        if (ready) slot.el.classList.add("enter");
      }
      paint(slot, item);
      if (i === at) wanted.push(divider);
      wanted.push(slot.el);
    });
    for (const [key, slot] of slots) {
      if (seen.has(key)) continue;
      slot.el.remove();
      slots.delete(key);
    }
    const current = [...track.children];
    if (current.length !== wanted.length || wanted.some((node, i) => current[i] !== node)) {
      track.replaceChildren(...wanted);
    }
    measure();
    ctx.invalidateRegion();
  }

  function measure() {
    centers = items.map((item) => {
      const slot = slots.get(item.key);
      return slot ? centerOf(slot.el) : 0;
    });
  }

  function clearTransforms() {
    for (const slot of slots.values()) {
      slot.el.style.transform = "";
      slot.icon.style.transform = "";
    }
    divider.style.transform = "";
    pill.style.removeProperty("--ga");
    pill.style.removeProperty("--gb");
  }

  // --- Hover: magnify and label --------------------------------------------------------------------

  function hoverAt(e: PointerEvent) {
    if (!hovered) {
      hovered = true;
      measure();
      ctx.invalidateRegion();
    }
    const p = along(e) - alongStart(track.getBoundingClientRect());
    const result = magnifyOn()
      ? magnify(centers, p, { max: MAGNIFY_MAX, radius: 3 * pitch, size })
      : { scales: centers.map(() => 1), shifts: centers.map(() => 0) };
    track.classList.add("tracking");
    items.forEach((item, i) => {
      const slot = slots.get(item.key);
      if (!slot) return;
      slot.el.style.transform = shiftBy(result.shifts[i] ?? 0);
      slot.icon.style.transform = `scale(${result.scales[i] ?? 1})`;
    });
    // The pill stretches to the outermost icons' new edges.
    const last = centers.length - 1;
    const spreadStart = -((result.shifts[0] ?? 0) - (((result.scales[0] ?? 1) - 1) * size) / 2);
    const spreadEnd = (result.shifts[last] ?? 0) + (((result.scales[last] ?? 1) - 1) * size) / 2;
    pill.style.setProperty("--ga", `${Math.max(0, spreadStart)}px`);
    pill.style.setProperty("--gb", `${Math.max(0, spreadEnd)}px`);
    const nearest = centers.reduce((best, c, i) => (Math.abs(c - p) < Math.abs((centers[best] ?? 0) - p) ? i : best), 0);
    if (!menuOpen) showLabel(nearest, result.shifts[nearest] ?? 0, result.scales[nearest] ?? 1);
  }

  function showLabel(index: number, shift: number, scale: number) {
    const item = items[index];
    if (!item) return hideLabel();
    const wasShown = label.classList.contains("show");
    label.textContent = item.name;
    const r = track.getBoundingClientRect();
    const center = alongStart(r) + (centers[index] ?? 0) + shift;
    const lift = size * scale + 8;
    const win = viewport();
    let transform: string;
    if (horizontal) {
      const half = label.offsetWidth / 2;
      transform = `translate(${clamp(center, half + 4, win.w - half - 4)}px, ${r.bottom - lift}px) translate(-50%, -100%)`;
    } else {
      const half = label.offsetHeight / 2;
      const y = clamp(center, half + 4, win.h - half - 4);
      transform =
        position === "left"
          ? `translate(${r.left + lift}px, ${y}px) translate(0, -50%)`
          : `translate(${r.right - lift}px, ${y}px) translate(-100%, -50%)`;
    }
    // Appear in place; slide only between icons.
    if (!wasShown) label.style.transition = "opacity 120ms";
    label.style.transform = transform;
    if (!wasShown) {
      label.getBoundingClientRect();
      label.style.transition = "";
    }
    label.classList.add("show");
  }

  function hideLabel() {
    label.classList.remove("show");
  }

  function endHover() {
    hovered = false;
    track.classList.remove("tracking");
    hideLabel();
    clearTransforms();
    ctx.invalidateRegion();
  }

  pill.addEventListener("pointerenter", () => {
    pointerInside = true;
    clearTimeout(hideTimer);
    if (!drag?.active) {
      hovered = true;
      measure();
      ctx.invalidateRegion();
    }
  });
  pill.addEventListener("pointermove", (e) => {
    if (!drag?.active) hoverAt(e);
  });
  pill.addEventListener("pointerleave", () => {
    pointerInside = false;
    if (!drag?.active) {
      endHover();
      scheduleHide();
    }
  });

  // --- Context menu --------------------------------------------------------------------------------

  function dockMenu(item: DockItem): MenuEntry[] {
    const entries: MenuEntry[] = [{ kind: "header", label: item.name }];
    for (const w of item.windows) {
      entries.push({
        kind: "item",
        label: w.title || item.name,
        marker: w.active ? "dot" : undefined,
        detail: w.minimized ? "Minimized" : undefined,
        run: () => fire("activate", { hwnd: w.hwnd }),
      });
    }
    if (item.windows.length > 0) entries.push({ kind: "separator" });
    entries.push(
      item.pinned
        ? { kind: "item", label: "Remove from Dock", run: () => fire("unpin", { path: item.path }) }
        : { kind: "item", label: "Keep in Dock", run: () => fire("pin", { path: item.path, name: item.name }) },
      { kind: "item", label: "New window", run: () => fire("launch", { path: item.path, newInstance: true }) },
    );
    if (item.windows.length > 0) {
      entries.push({ kind: "separator" });
      entries.push({
        kind: "item",
        label: item.windows.length > 1 ? "Close all windows" : "Close window",
        run: () => item.windows.forEach((w) => fire("close", { hwnd: w.hwnd })),
      });
    }
    return entries;
  }

  function openItemMenu(slot: Slot) {
    // Anchor on the icon along the dock and on the whole pill across it, so the menu clears
    // magnified icons. Menus open toward the screen center.
    const [sx, sy, sw, sh] = rectOf(slot.el);
    const [px, py, pw, ph] = rectOf(pill);
    const anchor: Rect = horizontal ? [sx, py, sw, ph] : [px, sy, pw, sh];
    const side: Side = position === "bottom" ? "above" : position === "left" ? "right" : "left";
    const origin = position === "bottom" ? "50% 100%" : position === "left" ? "0 50%" : "100% 50%";
    // Clear the magnified icons, which stand out past the pill.
    const gap = 10 + (magnifyOn() ? (MAGNIFY_MAX - 1) * size : 0);
    clearTimeout(hideTimer);
    hideLabel();
    menuOpen = true;
    openMenu({
      anchor: slot.el,
      entries: dockMenu(slot.item),
      origin,
      place: (menuSize) => placePopover(anchor, menuSize, viewport(), side, "center", gap),
      onClose: () => {
        menuOpen = false;
        scheduleHide();
      },
    });
  }

  // --- Drag ----------------------------------------------------------------------------------------

  function beginPress(slot: Slot, e: PointerEvent) {
    if (e.button !== 0 || drag) return;
    const from = items.findIndex((item) => item.key === slot.item.key);
    if (from < 0) return;
    drag = {
      slot,
      from,
      pointerId: e.pointerId,
      x0: e.clientX,
      y0: e.clientY,
      active: false,
      others: [],
      boundary: 0,
      to: null,
      far: false,
    };
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragEnd);
    window.addEventListener("pointercancel", onDragCancel);
  }

  function activateDrag(d: Drag) {
    d.active = true;
    closeMenu();
    endHover();
    measure();
    d.others = centers.filter((_, i) => i !== d.from);
    d.boundary = divider.isConnected ? centerOf(divider) : 0;
    track.classList.add("shifting");
    d.slot.el.classList.add("dragged");
    try {
      d.slot.el.setPointerCapture(d.pointerId);
    } catch {
      // The pointer is already gone (a cancelled touch): the window listeners still end the drag.
    }
    ctx.invalidateRegion();
  }

  function onDragMove(e: PointerEvent) {
    const d = drag;
    if (!d || e.pointerId !== d.pointerId) return;
    const dx = e.clientX - d.x0;
    const dy = e.clientY - d.y0;
    if (!d.active) {
      if (!exceedsThreshold(dx, dy, DRAG_THRESHOLD)) return;
      activateDrag(d);
    }
    const p = along(e) - alongStart(track.getBoundingClientRect());
    const pillRect = pill.getBoundingClientRect();
    const [lo, hi] = horizontal ? [pillRect.top, pillRect.bottom] : [pillRect.left, pillRect.right];
    d.to = dropIndex(items, d.from, d.others, p, d.boundary);
    d.far = !!items[d.from]?.pinned && distanceOutside(across(e), lo, hi) > FAR_DISTANCE;

    d.slot.el.style.transform = `translate(${dx}px, ${dy}px) scale(1.08)`;
    d.slot.el.classList.toggle("far", d.far);
    const shifts = slotShifts(items.length, d.from, d.far ? null : d.to);
    items.forEach((item, i) => {
      const slot = slots.get(item.key);
      if (slot && slot !== d.slot) slot.el.style.transform = shiftBy((shifts[i] ?? 0) * pitch);
    });
    const dividerOpens = !items[d.from]?.pinned && d.to !== null;
    divider.style.transform = shiftBy(dividerOpens ? pitch : 0);
  }

  function stopDragListeners() {
    window.removeEventListener("pointermove", onDragMove);
    window.removeEventListener("pointerup", onDragEnd);
    window.removeEventListener("pointercancel", onDragCancel);
  }

  function onDragEnd(e: PointerEvent) {
    const d = drag;
    if (!d || e.pointerId !== d.pointerId) return;
    stopDragListeners();
    if (!d.active) {
      drag = undefined;
      return;
    }
    // The click that follows this release must not activate the app.
    suppressClick = true;
    setTimeout(() => (suppressClick = false), 0);
    settle(d, resolveDrop(items, d.from, d.far ? null : d.to, d.far));
  }

  function onDragCancel(e: PointerEvent) {
    const d = drag;
    if (!d || e.pointerId !== d.pointerId) return;
    stopDragListeners();
    if (d.active) settle(d, { type: "none" });
    else drag = undefined;
  }

  scope.add(() => stopDragListeners());
  window.addEventListener("keydown", onKey);
  scope.add(() => window.removeEventListener("keydown", onKey));
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || !drag?.active) return;
    stopDragListeners();
    settle(drag, { type: "none" });
  }

  /** Ends a drag: sends the action, shows the new order at once, and eases the icon into place. */
  function settle(d: Drag, action: DropAction) {
    const finish = () => {
      const r = rectOf(d.slot.el);
      const before = { x: r[0] + r[2] / 2, y: r[1] + r[3] / 2 };

      // Other icons are already where the new order puts them: swap the DOM with no transition.
      track.classList.add("instant");
      d.slot.el.classList.remove("dragged", "far", "poof");
      clearTransforms();
      track.classList.remove("shifting");
      if (action.type !== "none") {
        send(action);
        pendingItems = undefined;
        setItems(applyDrop(items, action));
      } else if (pendingItems) {
        setItems(pendingItems);
        pendingItems = undefined;
      }
      track.getBoundingClientRect();
      track.classList.remove("instant");

      if (d.slot.el.isConnected && !prefersReducedMotion()) {
        const [x, y, w, h2] = rectOf(d.slot.el);
        d.slot.el.style.transition = "none";
        d.slot.el.style.transform = `translate(${before.x - (x + w / 2)}px, ${before.y - (y + h2 / 2)}px)`;
        d.slot.el.getBoundingClientRect();
        d.slot.el.style.transition = "";
        d.slot.el.style.transform = "";
      }
      drag = undefined;
      ctx.invalidateRegion();
      scheduleHide();
    };
    const vanishes = action.type === "unpin" && d.slot.item.windows.length === 0;
    if (vanishes && !prefersReducedMotion()) {
      d.slot.el.classList.add("poof");
      setTimeout(finish, 240);
    } else {
      finish();
    }
  }

  function send(action: DropAction) {
    switch (action.type) {
      case "reorder":
        return fire("reorder", { paths: action.paths });
      case "pin":
        return fire("pin", { path: action.path, name: action.name, index: action.index });
      case "unpin":
        return fire("unpin", { path: action.path });
      case "none":
        return;
    }
  }

  // --- Auto-hide -----------------------------------------------------------------------------------

  function reveal() {
    if (!autohide) return;
    revealed = true;
    regionOn = true;
    wrap.classList.add("revealed");
    ctx.invalidateRegion();
    scheduleHide();
  }

  function scheduleHide() {
    if (!autohide) return;
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => {
      if (!menuOpen && !drag && !pointerInside) hide();
    }, HIDE_DELAY);
  }

  function hide() {
    revealed = false;
    wrap.classList.remove("revealed");
    endHover();
    const done = () => {
      if (revealed) return;
      regionOn = false;
      ctx.invalidateRegion();
    };
    if (prefersReducedMotion()) done();
    else setTimeout(done, SLIDE_MS + 30);
  }

  scope.on("reveal", reveal);
  window.addEventListener("blur", scheduleHide);
  scope.add(() => window.removeEventListener("blur", scheduleHide));

  scope.on("apps", (next) => {
    // A host update mid-drag would reshuffle the DOM under the pointer: apply it afterwards.
    if (drag) pendingItems = next;
    else setItems(next);
  });
  ready = true;

  // --- Bar -----------------------------------------------------------------------------------------

  return {
    rects() {
      const layers = layerRects();
      if (drag?.active) return [[0, 0, window.innerWidth, window.innerHeight], ...layers];
      if (!regionOn) return layers;
      const box = inflate(layoutBox(pill), PILL_SHADOW);
      if (!hovered) return [box, ...layers];
      const headroom = (magnifyOn() ? (MAGNIFY_MAX - 1) * size : 0) + LABEL_SPACE;
      return [hoverExtent(box, position, headroom, size), ...layers];
    },
    destroy() {
      clearTimeout(hideTimer);
      closeMenu();
      stopDragListeners();
      scope.dispose();
      wrap.remove();
    },
  };
}
