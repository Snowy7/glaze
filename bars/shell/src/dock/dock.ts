// The dock, in two styles (docs/bars.md, "Dock"): `floating`, a pill of app icons with hover
// magnify; and `taskbar`, a full-width Windows-style bar with `left` modules, task buttons (titles
// optional) and `right` modules. Both have indicators, count badges, attention, launch bounce,
// previews, context menus, drag to reorder / pin / unpin, extras (show desktop, trash) and
// auto-hide. Geometry comes from `config.dock`; pure logic is in model.ts, drag.ts, magnify.ts
// and preview.ts.

import type { Bar, BarContext } from "../bar";
import { button, glyph, glyphLetter, h, layoutBox, prefersReducedMotion, rectOf, surface, viewport } from "../dom";
import { clamp, hoverExtent, inflate, placePopover, type Rect, type Side } from "../geometry";
import { createScope, fire } from "../host";
import { icons } from "../icons";
import { closeMenu, openMenu, type MenuEntry } from "../menu";
import { layerRects } from "../overlay";
import type { BarsConfig, DockItem } from "../protocol";
import { matchApp, iconUrl, isMono } from "../style/icon-pack";
import { leafDuration } from "../style/motion";
import { groupsOf } from "../top/build";
import { mount } from "../top/top";
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
import { applyDrop, dividerIndex, indicator, itemTitle, orderItems, splitItems, wantsAttention, type Shown } from "./model";
import { createPreviews } from "./preview";

/** Room above (or beside) a magnified icon for the name label. */
const LABEL_SPACE = 36;
const HIDE_DELAY = 1000;
/** How long a launched app may take to show a window before the bounce stops. */
const BOUNCE_LIMIT = 4000;
/** A task button with a title, DIPs. */
const TITLE_WIDTH = 168;
/** Room around a taskbar's icon inside its button. */
const TASK_PAD = 8;

interface Slot {
  item: Shown;
  el: HTMLButtonElement;
  /** What magnify scales: the picture and its badge. */
  icon: HTMLElement;
  pic: HTMLElement;
  count: HTMLElement;
  title: HTMLElement | undefined;
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
  const dock = config.dock;
  const { size, position, autohide, margin, spacing } = dock;
  const taskbar = dock.style === "taskbar";
  const horizontal = position === "bottom" || position === "top";
  const titles = taskbar && dock.titles && horizontal;
  const slotLength = taskbar ? (titles ? TITLE_WIDTH : size + 2 * TASK_PAD) : size;
  const pitch = slotLength + spacing;
  const scope = createScope();
  const magnifyOn = () => !taskbar && dock.magnify.enabled && !prefersReducedMotion();
  const maxScale = () => (magnifyOn() ? dock.magnify.scale : 1);

  document.documentElement.setAttribute("data-position", position);
  const wrap = h("div", `dock ${dock.style}${autohide ? " autohide" : ""}`);
  wrap.style.setProperty("--size", `${size}px`);
  wrap.style.setProperty("--slot", `${slotLength}px`);
  wrap.style.setProperty("--pad", `${dock.padding}px`);
  wrap.style.setProperty("--edge-gap", `${margin}px`);
  wrap.style.setProperty("--gap", `${spacing}px`);
  const pill = h("div", "pill");
  pill.dataset.band = "";
  const face = surface("pill-surface");
  const track = h("div", "items");
  const divider = h("div", "divider");
  const label = h("div", "dock-label");
  const extras = h("div", "extras");
  if (taskbar) {
    const seen = new Set<string>();
    const zone = (entries: BarsConfig["dock"]["left"], name: string) => {
      const el = h("div", `tb-zone ${name}`);
      for (const group of groupsOf(entries, seen)) {
        if (el.childElementCount > 0) el.append(h("span", "sep"));
        el.append(...group.map((module) => mount(module, config, scope)));
      }
      return el;
    };
    const start = zone(dock.left, "tb-start");
    const end = zone(dock.right, "tb-end");
    const group = h("div", "tb-group", start, track);
    pill.dataset.align = dock.align;
    pill.append(face, group, end, extras);
  } else {
    pill.append(face, track, extras);
  }
  wrap.append(pill, label);
  root.append(wrap);

  const slots = new Map<string, Slot>();
  let items: Shown[] = [];
  let centers: number[] = [];
  let ready = false;
  let hovered = false;
  let pointerInside = false;
  let menuOpen = false;
  let revealed = !autohide;
  let regionOn = !autohide;
  let hideTimer = 0;
  let drag: Drag | undefined;
  let pendingItems: DockItem[] | undefined;
  /** Items launched from the dock that have no window yet: they bounce. */
  const launching = new Map<string, number>();

  // Axis helpers: the dock runs along x (bottom, top) or y (left, right).
  const along = (e: { clientX: number; clientY: number }) => (horizontal ? e.clientX : e.clientY);
  const across = (e: { clientX: number; clientY: number }) => (horizontal ? e.clientY : e.clientX);
  const alongStart = (r: DOMRect) => (horizontal ? r.left : r.top);
  const shiftBy = (v: number) => (horizontal ? `translate(${v}px, 0)` : `translate(0, ${v}px)`);
  const centerOf = (el: HTMLElement) =>
    horizontal ? el.offsetLeft + el.offsetWidth / 2 : el.offsetTop + el.offsetHeight / 2;
  /** Toward the middle of the screen, where popovers and labels go. */
  const inward: Side =
    position === "bottom" ? "above" : position === "top" ? "below" : position === "left" ? "right" : "left";
  const origin =
    position === "bottom" ? "50% 100%" : position === "top" ? "50% 0" : position === "left" ? "0 50%" : "100% 50%";

  // --- Items ---------------------------------------------------------------------------------------

  /** The item's picture: the icon pack's when it has one, else the app's own. */
  function iconFor(item: DockItem): { src: string; mono: boolean } | undefined {
    const pack = ctx.iconPack;
    const entry = pack ? matchApp(pack, { aumid: item.aumid, exe: item.exe, path: item.path }) : undefined;
    if (pack && entry) return { src: iconUrl(pack, entry), mono: isMono(pack, entry) };
    return item.icon ? { src: item.icon, mono: false } : undefined;
  }

  function paint(slot: Slot, item: Shown) {
    slot.item = item;
    const el = slot.el;
    el.setAttribute("aria-label", item.name);
    el.dataset.state = indicator(item);
    el.dataset.name = item.name; // for theme CSS: per-app styling and icon swaps
    el.dataset.pinned = String(item.pinned);
    el.dataset.attention = String(wantsAttention(item));
    const count = item.windows.length;
    slot.count.textContent = dock.counter && count > 1 ? String(Math.min(count, 99)) : "";
    if (slot.title) slot.title.textContent = itemTitle(item);
    if (count > 0 && launching.has(item.key)) stopBounce(item.key);
    const icon = iconFor(item);
    const src = icon ? `${icon.mono ? "mono:" : ""}${icon.src}` : undefined;
    if (slot.iconSrc === src && slot.pic.firstChild) return;
    slot.iconSrc = src;
    if (icon?.mono) {
      const mask = h("div", "mask");
      mask.style.setProperty("--mask", `url("${icon.src}")`);
      slot.pic.replaceChildren(mask);
    } else if (icon) {
      const img = h("img");
      img.src = icon.src;
      img.alt = "";
      img.draggable = false;
      slot.pic.replaceChildren(img);
    } else {
      slot.pic.replaceChildren(h("div", "ph", glyphLetter(item.name)));
    }
  }

  function makeSlot(item: Shown): Slot {
    const pic = h("div", "pic");
    const count = h("span", "count");
    const icon = h("div", "icon", pic, count);
    const title = titles ? h("span", "title") : undefined;
    const el = h("button", "slot", h("span", "tile"), icon, ...(title ? [title] : []), h("span", "dot"));
    el.type = "button";
    const slot: Slot = { item, el, icon, pic, count, title, iconSrc: undefined };
    // Pointer clicks activate on release (see onDragEnd): magnify slides icons while the pointer
    // arrives, so press and release can land on different elements and no `click` fires. Here
    // only the keyboard's click (Enter/Space, detail 0) remains.
    el.addEventListener("click", (e) => {
      if (e.detail === 0) activate(slot.item);
    });
    el.addEventListener("mousedown", (e) => {
      if (e.button === 1) e.preventDefault();
    });
    el.addEventListener("auxclick", (e) => {
      if (e.button !== 1) return;
      e.preventDefault();
      launch(slot.item, true);
    });
    el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      openItemMenu(slot);
    });
    el.addEventListener("pointerdown", (e) => beginPress(slot, e));
    el.addEventListener("pointerenter", () => {
      if (dock.preview.enabled && !drag?.active && !menuOpen) previews.hover(slot.item, el);
    });
    el.addEventListener("pointerleave", () => previews.leave());
    return slot;
  }

  /** What launches an item again (a split-off window's app). */
  const pathOf = (item: Shown): string => item.of?.path ?? item.path;

  function activate(item: Shown) {
    previews.close();
    const hwnd = item.of ? item.windows[0]?.hwnd : undefined;
    if (hwnd !== undefined) return fire("activate", { hwnd });
    if (item.windows.length === 0) startBounce(item.key);
    fire("activate", { app: item.key });
  }

  function launch(item: Shown, newInstance: boolean) {
    startBounce(item.key);
    fire("launch", { path: pathOf(item), newInstance });
  }

  function startBounce(key: string) {
    if (!dock.bounce || prefersReducedMotion()) return;
    stopBounce(key);
    slots.get(key)?.el.classList.add("launching");
    launching.set(key, window.setTimeout(() => stopBounce(key), BOUNCE_LIMIT));
  }

  function stopBounce(key: string) {
    clearTimeout(launching.get(key));
    launching.delete(key);
    slots.get(key)?.el.classList.remove("launching");
  }

  function setItems(next: readonly DockItem[]) {
    items = dock.split ? splitItems(orderItems(next)) : orderItems(next);
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
    face.style.removeProperty("--ga");
    face.style.removeProperty("--gb");
  }

  // --- Extras: show desktop, trash -----------------------------------------------------------------

  if (dock.extras.length > 0) {
    if (!taskbar) extras.append(h("div", "divider"));
    for (const extra of dock.extras) {
      if (extra === "desktop") {
        const el = button("slot extra desktop", h("span", "tile"), h("div", "icon", glyph(icons.desktop)));
        el.title = "Show desktop";
        el.addEventListener("click", () => fire("showDesktop"));
        extras.append(el);
      } else {
        const g = glyph(icons.trash);
        const el = button("slot extra trash", h("span", "tile"), h("div", "icon", g));
        el.title = "Recycle Bin";
        el.addEventListener("click", () => fire("trash", { action: "open" }));
        el.addEventListener("contextmenu", (e) => {
          e.preventDefault();
          openExtraMenu(el, [
            { kind: "header", label: "Recycle Bin" },
            { kind: "item", label: "Open", run: () => fire("trash", { action: "open" }) },
            { kind: "item", label: "Empty Recycle Bin", run: () => fire("trash", { action: "empty" }) },
          ]);
        });
        scope.on("trash", ({ full }) => {
          g.innerHTML = full ? icons.trashFull : icons.trash;
          el.dataset.full = String(full);
        });
        extras.append(el);
      }
    }
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
      ? magnify(centers, p, { max: dock.magnify.scale, radius: dock.magnify.range * pitch, size })
      : { scales: centers.map(() => 1), shifts: centers.map(() => 0) };
    if (magnifyOn()) {
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
      face.style.setProperty("--ga", `${Math.max(0, spreadStart)}px`);
      face.style.setProperty("--gb", `${Math.max(0, spreadEnd)}px`);
    }
    const nearest = centers.reduce((best, c, i) => (Math.abs(c - p) < Math.abs((centers[best] ?? 0) - p) ? i : best), 0);
    const item = items[nearest];
    const previewing = dock.preview.enabled && item && item.windows.length > 0;
    if (!menuOpen && !previewing && !titles) showLabel(nearest, result.shifts[nearest] ?? 0, result.scales[nearest] ?? 1);
    else hideLabel();
  }

  function showLabel(index: number, shift: number, scale: number) {
    const item = items[index];
    if (!item) return hideLabel();
    const wasShown = label.classList.contains("show");
    label.textContent = item.name;
    const r = track.getBoundingClientRect();
    const center = alongStart(r) + (centers[index] ?? 0) + shift;
    const lift = (taskbar ? size + 2 * dock.padding - 8 : size * scale) + 8;
    const win = viewport();
    let transform: string;
    if (horizontal) {
      const half = label.offsetWidth / 2;
      const x = clamp(center, half + 4, win.w - half - 4);
      transform =
        position === "bottom"
          ? `translate(${x}px, ${r.bottom - lift}px) translate(-50%, -100%)`
          : `translate(${x}px, ${r.top + lift}px) translate(-50%, 0)`;
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

  track.addEventListener("pointerenter", () => {
    pointerInside = true;
    clearTimeout(hideTimer);
    if (!drag?.active) {
      hovered = true;
      measure();
      ctx.invalidateRegion();
    }
  });
  track.addEventListener("pointermove", (e) => {
    if (!drag?.active) hoverAt(e);
  });
  track.addEventListener("pointerleave", () => {
    if (!drag?.active) endHover();
  });
  pill.addEventListener("pointerenter", () => {
    pointerInside = true;
    clearTimeout(hideTimer);
  });
  pill.addEventListener("pointerleave", () => {
    pointerInside = false;
    if (!drag?.active) {
      endHover();
      scheduleHide();
    }
  });

  // --- Previews ------------------------------------------------------------------------------------

  /** Next to a slot along the dock, clear of the whole pill (and magnified icons) across it. */
  const anchorOf = (el: HTMLElement): { rect: Rect; side: Side; origin: string; gap: number } => {
    const [sx, sy, sw, sh] = rectOf(el);
    const [px, py, pw, ph] = rectOf(pill);
    const rect: Rect = horizontal ? [sx, py, sw, ph] : [px, sy, pw, sh];
    return { rect, side: inward, origin, gap: 10 + (magnifyOn() ? (maxScale() - 1) * size : 0) };
  };
  const previews = createPreviews(dock.preview.delay, anchorOf);
  scope.add(() => previews.close());

  // --- Context menu --------------------------------------------------------------------------------

  function dockMenu(item: Shown): MenuEntry[] {
    const app = item.of ?? item;
    const entries: MenuEntry[] = [{ kind: "header", label: app.name }];
    for (const w of item.windows) {
      entries.push({
        kind: "item",
        label: w.title || app.name,
        marker: w.active ? "dot" : undefined,
        detail: w.minimized ? "Minimized" : w.attention ? "Wants attention" : undefined,
        run: () => fire("activate", { hwnd: w.hwnd }),
      });
    }
    if (item.windows.length > 0) entries.push({ kind: "separator" });
    entries.push(
      app.pinned
        ? { kind: "item", label: "Remove from Dock", run: () => fire("unpin", { path: app.path }) }
        : { kind: "item", label: "Keep in Dock", run: () => fire("pin", { path: app.path, name: app.name }) },
      { kind: "item", label: "New window", run: () => launch(item, true) },
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

  function openExtraMenu(el: HTMLElement, entries: MenuEntry[]) {
    const { rect, side, origin: from, gap } = anchorOf(el);
    clearTimeout(hideTimer);
    previews.close();
    menuOpen = true;
    openMenu({
      anchor: el,
      entries,
      origin: from,
      place: (menuSize) => placePopover(rect, menuSize, viewport(), side, "center", gap),
      onClose: () => {
        menuOpen = false;
        scheduleHide();
      },
    });
  }

  function openItemMenu(slot: Slot) {
    hideLabel();
    openExtraMenu(slot.el, dockMenu(slot.item));
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
    previews.close();
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
      // A click: the icon that was pressed, wherever magnify moved it meanwhile.
      drag = undefined;
      activate(d.slot.item);
      return;
    }
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
      if (!menuOpen && !drag && !pointerInside && !previews.open) hide();
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
    const slide = leafDuration("show");
    if (slide === 0) done();
    else setTimeout(done, slide + 30);
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
      // Room for the pill's shadow and glow: its margin (inside the band), at least 8 DIPs.
      const box = inflate(layoutBox(pill), Math.max(8, margin));
      if (!hovered) return [box, ...layers];
      const headroom = (magnifyOn() ? (maxScale() - 1) * size : 0) + LABEL_SPACE;
      return [hoverExtent(box, position, headroom, magnifyOn() ? size : 0), ...layers];
    },
    destroy() {
      clearTimeout(hideTimer);
      for (const key of [...launching.keys()]) stopBounce(key);
      closeMenu();
      stopDragListeners();
      scope.dispose();
      wrap.remove();
    },
  };
}
