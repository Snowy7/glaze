// @ts-check
import { $, trackSettings, trackSize } from "./kit.js";
import { dayKey, recordActivity } from "./activity.js";

/** @typedef {{ id: number, text: string, done: boolean }} Item */

const KEY = "items";
const SAVE_DELAY_MS = 400;
const MAX_ITEMS = 300;
const MAX_TEXT = 240;
const COMPACT_BELOW_PX = 124; // shorter than this there is no room for the add field
const NARROW_BELOW_PX = 150; // narrower than this the title makes way for the count
const FADE_PX = 18;
const SVG_NS = "http://www.w3.org/2000/svg";

const card = $("#card");
const titleEl = $("#title");
const countEl = $("#count");
const bar = $("#bar");
const barFill = $("#bar-fill");
const scroller = $("#scroller");
const list = $("#list");
const emptyEl = $("#empty");
const emptyText = $("#empty-text");
const add = $("#add", HTMLInputElement);

const defaults = { title: "To do", addPosition: "bottom", hideDone: false };
let settings = defaults;
/** @type {Item[]} */
let items = [];
let nextId = 1;
let loaded = false;
let compact = false;
/** The row being edited (its text is `draft` until committed), or null. */
let editingId = /** @type {number | null} */ (null);
let draft = "";

// ---- persistence: debounced; flushed when hidden and on pagehide

/** @type {ReturnType<typeof setTimeout> | undefined} */
let saveTimer;
let dirty = false;

function flush() {
  clearTimeout(saveTimer);
  if (!dirty || !loaded) return;
  dirty = false;
  const saved = items.map(({ text, done }) => ({ text, done }));
  mypaper.storage.set(KEY, saved).catch((err) => {
    dirty = true;
    console.error("todo: save failed", err);
  });
}

function changed() {
  dirty = true;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(flush, SAVE_DELAY_MS);
}

mypaper.on("visibility", (visible) => {
  if (!visible) flush();
});
addEventListener("pagehide", flush);

/** @param {string} text */
const clean = (text) => text.replace(/\s+/g, " ").trim().slice(0, MAX_TEXT);

/**
 * Storage is plain JSON anyone can edit: keep only well-formed items.
 * @param {unknown} raw
 * @returns {Item[]}
 */
function readItems(raw) {
  if (!Array.isArray(raw)) return [];
  /** @type {Item[]} */
  const out = [];
  for (const entry of raw) {
    if (out.length >= MAX_ITEMS) break;
    if (typeof entry !== "object" || entry === null) continue;
    const text = "text" in entry && typeof entry.text === "string" ? clean(entry.text) : "";
    if (text) out.push({ id: nextId++, text, done: "done" in entry && entry.done === true });
  }
  return out;
}

// ---- rendering: DOM nodes only, never innerHTML

/**
 * @param {string} d path in a 16x16 box
 */
function icon(d) {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(SVG_NS, "path");
  path.setAttribute("d", d);
  svg.append(path);
  return svg;
}

/**
 * @param {string} className
 * @param {string} label accessible name
 */
function button(className, label) {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.setAttribute("aria-label", label);
  return node;
}

/** @param {Item} item */
function rowNode(item) {
  const li = document.createElement("li");
  li.className = "row";
  li.dataset.id = String(item.id);
  li.toggleAttribute("data-done", item.done);

  const check = button("check gfx", item.done ? "Mark as not done" : "Mark as done");
  check.setAttribute("aria-pressed", String(item.done));
  check.append(icon("M3.5 8.5l3 3 6-7"));

  /** @type {HTMLElement} */
  let body;
  if (item.id === editingId) {
    const area = document.createElement("textarea");
    area.className = "edit";
    area.rows = 1;
    area.maxLength = MAX_TEXT;
    area.value = draft;
    area.setAttribute("aria-label", "Edit task");
    body = area;
  } else {
    body = document.createElement("span");
    body.className = "text";
    body.textContent = item.text;
    body.tabIndex = 0;
    body.setAttribute("role", "button");
    body.title = "Edit";
  }

  const del = button("del", `Delete task: ${item.text}`);
  del.append(icon("M4 4l8 8M12 4l-8 8"));

  li.append(check, body, del);
  return li;
}

function updateFade() {
  const { scrollTop, scrollHeight, clientHeight } = scroller;
  scroller.style.setProperty("--ft", scrollTop > 1 ? `${FADE_PX}px` : "0px");
  scroller.style.setProperty("--fb", scrollTop + clientHeight < scrollHeight - 1 ? `${FADE_PX}px` : "0px");
}
scroller.addEventListener("scroll", updateFade, { passive: true });

function render() {
  const total = items.length;
  const open = items.filter((i) => !i.done).length;
  // A compact box has room for the next things to do, not for what is finished.
  const shown = settings.hideDone || compact ? items.filter((i) => !i.done) : items;

  countEl.textContent = total === 0 ? "" : open === 0 ? "All done" : `${open} left`;
  countEl.toggleAttribute("data-all", total > 0 && open === 0);
  bar.hidden = total === 0;
  barFill.style.width = total === 0 ? "0%" : `${((total - open) / total) * 100}%`;

  list.replaceChildren(...shown.map(rowNode));
  emptyEl.hidden = !loaded || shown.length > 0;
  emptyText.textContent = total === 0 ? "Nothing to do" : "All done";

  const editor = list.querySelector(".edit");
  if (editor instanceof HTMLTextAreaElement) {
    editor.focus();
    editor.setSelectionRange(editor.value.length, editor.value.length);
  }
  updateFade();
}

// ---- actions

/** @param {number} id */
function startEdit(id) {
  const item = items.find((i) => i.id === id);
  if (!item) return;
  editingId = id;
  draft = item.text;
  render();
}

/** @param {number} id */
function focusText(id) {
  const node = list.querySelector(`.row[data-id="${id}"] .text`);
  if (node instanceof HTMLElement) node.focus();
}

/** Saves the edit; an emptied task is removed. Safe to call when nothing is being edited. */
function commitEdit() {
  if (editingId === null) return;
  const id = editingId;
  editingId = null;
  const text = clean(draft);
  items = text ? items.map((i) => (i.id === id ? { ...i, text } : i)) : items.filter((i) => i.id !== id);
  changed();
  render();
}

function cancelEdit() {
  if (editingId === null) return;
  editingId = null;
  render();
}

/** @param {string} raw */
function addItem(raw) {
  const text = clean(raw);
  if (!text || items.length >= MAX_ITEMS) return;
  const item = { id: nextId++, text, done: false };
  const atTop = settings.addPosition === "top";
  items = atTop ? [item, ...items] : [...items, item];
  changed();
  render();
  scroller.scrollTop = atTop ? 0 : scroller.scrollHeight;
  updateFade();
}

/**
 * Tasks completed during this session and the day each was counted on, so un-checking one of them
 * takes the count back (a task that was already done when the widget loaded is history).
 * @type {Map<number, string>}
 */
const counted = new Map();

/** @param {number} id */
function toggle(id) {
  const item = items.find((i) => i.id === id);
  if (!item) return;
  const done = !item.done;
  items = items.map((i) => (i.id === id ? { ...i, done } : i));
  if (done) {
    const day = dayKey(new Date());
    counted.set(id, day);
    recordActivity("todo", day, 1);
  } else {
    const day = counted.get(id);
    if (day !== undefined) recordActivity("todo", day, -1);
    counted.delete(id);
  }
  changed();
  render();
}

/** @param {number} id */
function remove(id) {
  items = items.filter((i) => i.id !== id);
  changed();
  render();
}

// ---- events

/** @param {Event} e */
function rowIdOf(e) {
  const row = e.target instanceof Element ? e.target.closest(".row") : null;
  return row instanceof HTMLElement ? Number(row.dataset.id) : null;
}

list.addEventListener("click", (e) => {
  const id = rowIdOf(e);
  const target = e.target instanceof Element ? e.target : null;
  if (id === null || !target) return;
  if (target.closest(".check")) toggle(id);
  else if (target.closest(".del")) remove(id);
  else if (target.closest(".text")) startEdit(id);
});

list.addEventListener("keydown", (e) => {
  const id = rowIdOf(e);
  if (id === null || !(e.target instanceof Element)) return;
  if (e.target.matches(".text") && (e.key === "Enter" || e.key === "F2")) {
    e.preventDefault();
    startEdit(id);
  } else if (e.target.matches(".edit")) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      commitEdit();
      focusText(id);
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
      focusText(id);
    }
  } else if (e.target.matches(".del, .check") && e.key === "Delete") {
    remove(id);
  }
});

list.addEventListener("input", (e) => {
  if (e.target instanceof HTMLTextAreaElement) draft = e.target.value;
});
list.addEventListener("focusout", (e) => {
  if (e.target instanceof Element && e.target.matches(".edit")) commitEdit();
});

add.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    addItem(add.value);
    add.value = "";
  } else if (e.key === "Escape") {
    if (add.value) add.value = "";
    else add.blur();
  }
});

// Edit mode (moving the widget) ends any typing.
mypaper.on("editmode", (editing) => {
  if (!editing) return;
  commitEdit();
  add.blur();
});

// ---- settings, size and first load

trackSettings(defaults, (next) => {
  settings = next;
  const title = next.title.trim();
  titleEl.textContent = title;
  titleEl.hidden = title === "";
  card.dataset.add = next.addPosition === "top" ? "top" : "bottom";
  render();
});

trackSize(({ px: [w, h] }) => {
  compact = h < COMPACT_BELOW_PX;
  card.toggleAttribute("data-compact", compact);
  card.toggleAttribute("data-narrow", w < NARROW_BELOW_PX);
  if (editingId !== null && compact) commitEdit();
  render();
});

mypaper.storage.get(KEY).then(
  (saved) => {
    items = readItems(saved);
    loaded = true;
    add.disabled = false;
    render();
  },
  (err) => {
    console.error("todo: load failed", err);
    loaded = true;
    add.disabled = false;
    render();
  },
);
