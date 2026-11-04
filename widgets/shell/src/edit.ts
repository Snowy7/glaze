// Edit mode: scrim and grid, per-widget chrome (drag, resize handles, ⋯ menu), toolbar, palette,
// settings and appearance panels, keyboard. All geometry and the no-overlap rule live in grid.ts,
// the appearance layering in appearance.ts; this file only turns pointer and key input into calls
// to them. The host's `state` stays authoritative.

import { below, preview, type PreviewContext, type Scope } from "./appearance";
import { buildAppearanceForm } from "./appearance-form";
import { clamp, h } from "./dom";
import {
  deltaToCells,
  findFreeSpot,
  moveBy,
  rectToPx,
  resizeBy,
  resolve,
  sizeLimits,
  toolbarEdge,
  type Handle,
  type Item,
  type PxRect,
  type Rect,
  type Resolution,
  type SizeLimits,
  type Vec2,
} from "./grid";
import type { Frames, Slot } from "./frames";
import type { Layer, ShellState, ShellToHost } from "./protocol";
import { buildSettingsForm, type SettingsForm } from "./settings-form";

export interface Editor {
  /** Adds the edit-mode chrome to a new slot. */
  decorate(slot: Slot): void;
  /** Call on every host `state`, before `frames.sync`: drops optimistic placements. */
  reset(): void;
  /** Call on every host `state`, after `frames.sync`. */
  sync(state: ShellState): void;
}

interface Options {
  root: HTMLElement;
  frames: Frames;
  send: (message: ShellToHost) => void;
}

type DragKind = "move" | Handle;

interface Drag {
  slot: Slot;
  kind: DragKind;
  el: HTMLElement;
  pointerId: number;
  startX: number;
  startY: number;
  items: Item[];
  origin: Item;
  limits: SizeLimits;
  baseOverrides: Map<string, Rect>;
  active: boolean;
  result: Resolution | undefined;
  abort: AbortController;
}

interface Popover {
  key: string;
  el: HTMLElement;
  anchorEl: HTMLElement | undefined;
  onClose: (() => void) | undefined;
}

const HANDLES: Handle[] = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];
const DRAG_THRESHOLD_PX = 3;
const ARROWS = new Map<string, Vec2>([
  ["ArrowLeft", [-1, 0]],
  ["ArrowRight", [1, 0]],
  ["ArrowUp", [0, -1]],
  ["ArrowDown", [0, 1]],
]);

export function createEditor({ root, frames, send }: Options): Editor {
  let state: ShellState | undefined;
  let selected: string | undefined;
  let drag: Drag | undefined;
  let popover: Popover | undefined;
  let gridKey = "";
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  // ---- static chrome ----

  const scrim = h("div", "scrim edit-only");
  const gridmap = h("div", "gridmap edit-only");
  const ghost = h("div", "ghost");
  const addButton = h("button", "btn", "+ Add widget");
  const appearanceButton = h("button", "btn", "Appearance");
  const doneButton = h("button", "btn primary", "Done");
  const toolbar = h("div", "toolbar edit-only", addButton, appearanceButton, doneButton);
  const banner = h("div", "banner edit-only");
  const toastEl = h("div", "toast");
  for (const button of [addButton, appearanceButton, doneButton]) button.type = "button";
  root.prepend(scrim, gridmap);
  root.append(ghost, toolbar, banner, toastEl);

  // ---- helpers ----

  /** The layout as the user sees it: the host's, plus optimistic placements awaiting its answer. */
  function effectiveItems(): Item[] {
    if (!state) return [];
    return state.instances
      .filter((i) => frames.slots.has(i.instance))
      .map((i) => ({ instance: i.instance, ...(frames.overrides.get(i.instance) ?? i) }));
  }

  function toast(message: string) {
    toastEl.textContent = message;
    toastEl.classList.add("on");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toastEl.classList.remove("on"), 2200);
  }

  function shake(slot: Slot) {
    slot.el.classList.remove("shake");
    void slot.el.offsetWidth; // restart the animation
    slot.el.classList.add("shake");
  }

  function select(instance: string | undefined) {
    selected = instance;
    for (const slot of frames.slots.values()) slot.el.classList.toggle("selected", slot.instance === instance);
  }

  /** Applies a successful resolution: show it now, tell the host once. */
  function commit(result: Extract<Resolution, { ok: true }>) {
    frames.overrides.clear();
    for (const item of result.layout) frames.overrides.set(item.instance, item);
    frames.render();
    send({ type: "layout", changes: result.changes });
  }

  function renderGridMap(grid: ShellState["grid"]) {
    const key = JSON.stringify(grid);
    if (key === gridKey) return;
    gridKey = key;
    Object.assign(gridmap.style, {
      left: `${grid.origin[0]}px`,
      top: `${grid.origin[1]}px`,
      gridTemplateColumns: `repeat(${grid.cols}, ${grid.cell[0]}px)`,
      gridTemplateRows: `repeat(${grid.rows}, ${grid.cell[1]}px)`,
      gap: `${grid.gap}px`,
    });
    gridmap.replaceChildren(...Array.from({ length: grid.cols * grid.rows }, () => h("div", "gc")));
  }

  // ---- popovers (one at a time) ----

  function closePopover() {
    if (!popover) return;
    const { el, onClose } = popover;
    popover = undefined;
    el.remove();
    onClose?.();
  }

  function openPopover(
    key: string,
    el: HTMLElement,
    anchor: DOMRect,
    place: "below-start" | "below-end" | "side",
    anchorEl?: HTMLElement,
    onClose?: () => void,
  ) {
    closePopover();
    el.classList.add("popover");
    root.append(el);
    popover = { key, el, anchorEl, onClose };

    const margin = 8;
    const { width, height } = el.getBoundingClientRect();
    let left = place === "below-end" ? anchor.right - width : anchor.left;
    let top = anchor.bottom + 8;
    if (place === "side") {
      left = anchor.right + 10;
      if (left + width > window.innerWidth - margin) left = anchor.left - width - 10;
      top = anchor.top;
    }
    el.style.left = `${clamp(left, margin, window.innerWidth - width - margin)}px`;
    el.style.top = `${clamp(top, margin, window.innerHeight - height - margin)}px`;
  }

  // Clicking anywhere else closes the open popover (its own anchor button toggles it instead).
  document.addEventListener(
    "pointerdown",
    (e) => {
      if (!popover || !(e.target instanceof Node)) return;
      if (popover.el.contains(e.target) || popover.anchorEl?.contains(e.target)) return;
      closePopover();
    },
    true,
  );

  function openPalette() {
    if (!state) return;
    if (popover?.key === "palette") return closePopover();
    const list = h("div", "palette");
    list.append(h("div", "popover-title", "Add widget"));
    if (state.widgets.length === 0) list.append(h("div", "empty", "No widgets installed."));
    for (const manifest of state.widgets) {
      const [w, hgt] = manifest.size.default;
      const entry = h("button", "entry", h("span", "name", manifest.name), h("span", "dims", `${w}×${hgt}`));
      entry.type = "button";
      if (manifest.description) entry.append(h("span", "desc", manifest.description));
      entry.addEventListener("click", () => {
        closePopover();
        addWidget(manifest.id);
      });
      list.append(entry);
    }
    openPopover("palette", list, addButton.getBoundingClientRect(), "below-start", addButton);
  }

  function openSettings(slot: Slot) {
    const inst = state?.instances.find((i) => i.instance === slot.instance);
    const specs = slot.manifest.settings ?? [];    if (!inst || specs.length === 0) return;
    const form: SettingsForm = buildSettingsForm(specs, inst.settings, (key, value) =>
      send({ type: "settings", instance: slot.instance, values: { [key]: value } }),
    );
    const close = h("button", "icon-btn close");
    close.type = "button";
    close.setAttribute("aria-label", "Close");
    close.addEventListener("click", closePopover);
    const panel = h("div", "panel", h("div", "popover-title row", h("span", "", slot.manifest.name), close), form.el);
    openPopover(`settings:${slot.instance}`, panel, slot.el.getBoundingClientRect(), "side", undefined, form.flush);
  }

  // ---- appearance: the user layer (toolbar) and an instance's own layer (⋯ menu) ----

  let previewContext: { context: PreviewContext; scope: Scope; layer: Layer } | undefined;
  let previewFrame = 0;

  /** Shows `layer` on the affected widgets, at most once per frame, without saving anything. */
  function previewLayer(context: PreviewContext, scope: Scope, layer: Layer) {
    previewContext = { context, scope, layer };
    if (previewFrame) return;
    previewFrame = requestAnimationFrame(() => {
      previewFrame = 0;
      if (!previewContext) return;
      for (const [instance, look] of preview(previewContext.context, previewContext.scope, previewContext.layer)) {
        frames.looks.set(instance, look);
      }
      frames.applyLooks();
    });
  }

  function openAppearance(slot?: Slot) {
    if (!state) return;
    const key = `appearance:${slot?.instance ?? ""}`;
    if (popover?.key === key) return closePopover();
    const own = slot && state.instances.find((i) => i.instance === slot.instance)?.own;
    if (slot && !own) return;

    const scope: Scope = slot ? { kind: "instance", instance: slot.instance } : { kind: "user" };
    const { base, user, accent } = state.appearance;
    const context: PreviewContext = {
      base,
      user,
      autoAccent: accent,
      instances: state.instances.map((i) => ({ instance: i.instance, own: i.own })),
    };
    const form = buildAppearanceForm({
      below: below(scope, base, user),
      layer: own ?? user,
      autoAccent: accent,
      onPreview: (layer) => previewLayer(context, scope, layer),
      onCommit: (values) => send({ type: "appearance", ...(slot && { instance: slot.instance }), values }),
    });
    const close = h("button", "icon-btn close");
    close.type = "button";
    close.setAttribute("aria-label", "Close");
    close.addEventListener("click", closePopover);
    const title = slot ? `Appearance · ${slot.manifest.name}` : "Appearance · all widgets";
    const panel = h("div", "panel wide", h("div", "popover-title row", h("span", "", title), close), form.el);
    const anchor = slot ? slot.el.getBoundingClientRect() : appearanceButton.getBoundingClientRect();
    openPopover(key, panel, anchor, slot ? "side" : "below-start", slot ? undefined : appearanceButton, () => {
      form.flush();
      cancelAnimationFrame(previewFrame);
      previewFrame = 0;
      previewContext = undefined;
    });
  }

  function openMenu(slot: Slot, anchorEl: HTMLElement) {
    if (popover?.key === `menu:${slot.instance}`) return closePopover();
    const item = (label: string, action: () => void, disabled = false) => {
      const button = h("button", "menu-item", label);
      button.type = "button";
      button.disabled = disabled;
      button.addEventListener("click", () => {
        closePopover();
        action();
      });
      return button;
    };
    const menu = h(
      "div",
      "menu",
      item("Settings", () => openSettings(slot), (slot.manifest.settings?.length ?? 0) === 0),
      item("Appearance", () => openAppearance(slot)),
      item("Duplicate", () => duplicate(slot.instance)),
      item("Remove", () => remove(slot.instance)),
    );
    openPopover(`menu:${slot.instance}`, menu, anchorEl.getBoundingClientRect(), "below-end", anchorEl);
  }

  // ---- toolbar and menu actions ----

  function addWidget(widget: string) {
    const manifest = state?.widgets.find((w) => w.id === widget);
    if (!state || !manifest) return;
    const size = manifest.size.default;
    const cell = findFreeSpot(state.grid, effectiveItems(), size);
    if (!cell) return toast("No room on this screen");
    send({ type: "add", widget, cell, size });
  }

  function duplicate(instance: string) {
    if (!state) return;
    const items = effectiveItems();
    const original = items.find((i) => i.instance === instance);
    if (!original) return;
    const [x, y] = original.cell;
    const [w, hgt] = original.size;
    const cell = findFreeSpot(state.grid, items, original.size, [[x + w, y], [x, y + hgt]]);
    if (!cell) return toast("No room on this screen");
    send({ type: "duplicate", instance, cell });
  }

  function remove(instance: string) {
    if (selected === instance) select(undefined);
    send({ type: "remove", instance });
  }

  addButton.addEventListener("click", openPalette);
  appearanceButton.addEventListener("click", () => openAppearance());
  doneButton.addEventListener("click", () => send({ type: "edit", on: false }));
  scrim.addEventListener("pointerdown", () => select(undefined));

  // ---- dragging and resizing ----

  function showGhost(rect: PxRect, ok: boolean) {
    ghost.style.transform = `translate(${rect.x}px, ${rect.y}px)`;
    ghost.style.width = `${rect.width}px`;
    ghost.style.height = `${rect.height}px`;
    ghost.classList.toggle("bad", !ok);
    ghost.classList.add("on");
  }

  function beginDrag(e: PointerEvent, slot: Slot, kind: DragKind, el: HTMLElement) {
    if (e.button !== 0 || !state?.editing || drag) return;
    if (kind !== "move" && slot.manifest.resizable === false) return;
    const items = effectiveItems();
    const origin = items.find((i) => i.instance === slot.instance);
    if (!origin) return;

    e.preventDefault();
    select(slot.instance);
    el.setPointerCapture(e.pointerId);
    const abort = new AbortController();
    drag = {
      slot,
      kind,
      el,
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      items,
      origin,
      limits: sizeLimits(slot.manifest),
      baseOverrides: new Map(frames.overrides),
      active: false,
      result: undefined,
      abort,
    };
    const opts = { signal: abort.signal };
    el.addEventListener("pointermove", updateDrag, opts);
    el.addEventListener("pointerup", (ev) => endDrag(ev, true), opts);
    el.addEventListener("pointercancel", (ev) => endDrag(ev, false), opts);
    el.addEventListener("lostpointercapture", (ev) => endDrag(ev, false), opts);
  }

  function updateDrag(e: PointerEvent) {
    const d = drag;
    if (!d || !state || e.pointerId !== d.pointerId) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    if (!d.active) {
      if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
      d.active = true;
      d.slot.el.classList.add("dragging");
    }

    const grid = state.grid;
    const delta = deltaToCells(grid, [dx, dy]);
    const place = (snap: boolean): Rect =>
      d.kind === "move"
        ? { cell: moveBy(grid, d.origin, delta, snap), size: d.origin.size }
        : resizeBy(grid, d.limits, d.origin, d.kind, delta, snap);

    const target = place(true);
    const result = resolve(grid, d.items, { instance: d.slot.instance, ...target }, d.kind === "move" ? "move" : "resize");
    d.result = result;

    // The widget follows the pointer exactly; the ghost shows where it will land; neighbours preview.
    frames.live.set(d.slot.instance, rectToPx(grid, place(false)));
    frames.overrides.clear();
    for (const [instance, rect] of d.baseOverrides) frames.overrides.set(instance, rect);
    if (result.ok) {
      for (const change of result.changes) if (change.instance !== d.slot.instance) frames.overrides.set(change.instance, change);
    }
    showGhost(rectToPx(grid, target), result.ok);
    frames.render();
  }

  /** Drops all drag state and visuals, restoring the overrides from before the drag. */
  function cleanupDrag(d: Drag) {
    drag = undefined;
    d.abort.abort();
    if (d.el.hasPointerCapture(d.pointerId)) d.el.releasePointerCapture(d.pointerId);
    d.slot.el.classList.remove("dragging");
    frames.live.delete(d.slot.instance);
    ghost.classList.remove("on");
    frames.overrides.clear();
    for (const [instance, rect] of d.baseOverrides) frames.overrides.set(instance, rect);
  }

  function endDrag(e: PointerEvent, released: boolean) {
    const d = drag;
    if (!d || e.pointerId !== d.pointerId) return;
    if (released && d.active) updateDrag(e);
    const result = d.result;
    cleanupDrag(d);

    if (released && d.active && result) {
      if (!result.ok) shake(d.slot);
      else if (result.changes.length > 0) return commit(result);
    }
    frames.render(); // settle back where it was (or where the host says)
  }

  // ---- per-slot chrome ----

  function decorate(slot: Slot) {
    const cover = h("div", "cover edit-only");
    const grip = h("div", "grip edit-only");
    const menuButton = h("button", "menu-btn edit-only");
    menuButton.type = "button";
    menuButton.setAttribute("aria-label", "Widget menu");
    grip.title = "Drag to move";

    cover.addEventListener("pointerdown", (e) => beginDrag(e, slot, "move", cover));
    grip.addEventListener("pointerdown", (e) => beginDrag(e, slot, "move", grip));
    menuButton.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      select(slot.instance);
    });
    menuButton.addEventListener("click", () => openMenu(slot, menuButton));
    cover.addEventListener("contextmenu", (e) => e.preventDefault());

    slot.el.append(cover, grip, menuButton);
    for (const handle of HANDLES) {
      const el = h("div", `rh rh-${handle} edit-only`);
      el.addEventListener("pointerdown", (e) => beginDrag(e, slot, handle, el));
      slot.el.append(el);
    }
  }

  // ---- keyboard ----

  /** Arrow = move one cell, Shift+arrow = resize (right/down grow, left/up shrink), via the same rule. */
  function nudge(dir: Vec2, resize: boolean) {
    const slot = selected ? frames.slots.get(selected) : undefined;
    if (!state || !slot || drag) return;
    const items = effectiveItems();
    const origin = items.find((i) => i.instance === slot.instance);
    if (!origin) return;
    if (resize && slot.manifest.resizable === false) return shake(slot);

    const target: Rect = resize
      ? resizeBy(state.grid, sizeLimits(slot.manifest), origin, dir[0] !== 0 ? "e" : "s", dir)
      : { cell: moveBy(state.grid, origin, dir), size: origin.size };
    const result = resolve(state.grid, items, { instance: origin.instance, ...target }, resize ? "resize" : "move");
    if (!result.ok || result.changes.length === 0) return shake(slot);
    commit(result);
  }

  /** Selects the next/previous widget in reading order. */
  function cycle(step: 1 | -1) {
    const order = effectiveItems().sort((a, b) => a.cell[1] - b.cell[1] || a.cell[0] - b.cell[0]);
    if (order.length === 0) return;
    const at = order.findIndex((i) => i.instance === selected);
    const next = order[(at === -1 ? (step === 1 ? 0 : order.length - 1) : at + step + order.length) % order.length];
    select(next?.instance);
  }

  window.addEventListener("keydown", (e) => {
    if (!state?.editing) return;
    const target = e.target instanceof Element ? e.target : undefined;

    if (e.key === "Escape") {
      e.preventDefault();
      if (popover) closePopover();
      else send({ type: "edit", on: false });
    } else if (e.key === "Tab") {
      if (target?.closest(".popover, .toolbar")) return; // normal focus order inside those
      e.preventDefault();
      cycle(e.shiftKey ? -1 : 1);
    } else {
      const dir = ARROWS.get(e.key);
      if (!dir || target?.closest("input, select, textarea")) return;
      e.preventDefault();
      nudge(dir, e.shiftKey);
    }
  });

  // ---- host state ----

  function sync(next: ShellState) {
    const wasEditing = state?.editing ?? false;
    state = next;
    root.classList.toggle("editing", next.editing);

    if (wasEditing && !next.editing) {
      if (drag) cleanupDrag(drag);
      closePopover();
      select(undefined);
    }

    const { work } = next;
    root.style.setProperty("--wx", `${work.x}px`);
    root.style.setProperty("--wy", `${work.y}px`);
    root.style.setProperty("--ww", `${work.width}px`);
    root.style.setProperty("--wh", `${work.height}px`);
    renderGridMap(next.grid);

    // The toolbar sits at the top of the work area, or at the bottom when a widget is up there.
    const edge = toolbarEdge(
      work,
      { width: toolbar.offsetWidth, height: toolbar.offsetHeight },
      next.instances.map((inst) => rectToPx(next.grid, inst)),
    );
    toolbar.classList.toggle("bottom", edge === "bottom");

    banner.textContent = next.error ?? "";
    banner.classList.toggle("on", Boolean(next.error));

    if (selected && !frames.slots.has(selected)) select(undefined);
    else select(selected);
    for (const slot of frames.slots.values()) slot.el.classList.toggle("fixed", slot.manifest.resizable === false);
    // A menu or settings panel for a widget that just went away.
    const [kind, instance] = popover?.key.split(":") ?? [];
    if (kind && instance && !frames.slots.has(instance)) closePopover();
  }

  return {
    decorate,
    reset: () => {
      if (!drag) frames.overrides.clear();
      frames.looks.clear(); // the host's answer carries what was saved
    },
    sync,
  };
}
