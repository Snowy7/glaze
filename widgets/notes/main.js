// @ts-check
import { $, trackSettings } from "./kit.js";
import { parse, toggleTask } from "./md.js";

/** @typedef {import("./md.js").Inline} Inline */
/** @typedef {import("./md.js").Block} Block */

const SAVE_DELAY_MS = 400;
const KEY = "text";

const card = $("#card");
const view = $("#view");
const editor = $("#editor", HTMLTextAreaElement);

let text = "";
let loaded = false;
let editing = false;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let saveTimer;
let dirty = false;

// ---- persistence: debounced while typing, flushed on blur and when hidden

function flush() {
  clearTimeout(saveTimer);
  if (!dirty || !loaded) return;
  dirty = false;
  mypaper.storage.set(KEY, text).catch((err) => {
    dirty = true;
    console.error("notes: save failed", err);
  });
}

/** @param {string} next */
function change(next) {
  text = next;
  dirty = true;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(flush, SAVE_DELAY_MS);
}

mypaper.on("visibility", (visible) => {
  if (!visible) flush();
});
addEventListener("pagehide", flush);

// ---- rendering: DOM nodes only, never innerHTML

/**
 * @param {string} tag
 * @param {string} [className]
 */
function el(tag, className) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  return node;
}

/** @param {Inline[]} inline */
function inlineNodes(inline) {
  return inline.map((part) => {
    const tag = { text: "", bold: "b", italic: "i", code: "code", strike: "s" }[part.kind];
    if (!tag) return document.createTextNode(part.text);
    const node = el(tag);
    node.textContent = part.text;
    return node;
  });
}

/** @param {Block} block */
function blockNode(block) {
  switch (block.kind) {
    case "blank":
      return el("div", "gap");
    case "rule":
      return el("hr");
    case "heading": {
      const node = el(`h${block.level}`);
      node.append(...inlineNodes(block.inline));
      return node;
    }
    case "bullet": {
      const node = el("div", "li");
      const body = el("span", "text");
      body.append(...inlineNodes(block.inline));
      node.append(body);
      return node;
    }
    case "task": {
      const node = el("div", "task");
      node.dataset.line = String(block.line);
      node.toggleAttribute("data-checked", block.checked);
      const body = el("span", "text");
      body.append(...inlineNodes(block.inline));
      node.append(el("span", "box"), body);
      return node;
    }
    case "quote": {
      const node = el("blockquote");
      node.append(...inlineNodes(block.inline));
      return node;
    }
    case "paragraph": {
      const node = el("div");
      node.append(...inlineNodes(block.inline));
      return node;
    }
  }
}

function render() {
  if (text.trim() === "") {
    const hint = el("div", "empty");
    if (loaded) {
      const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      icon.setAttribute("viewBox", "0 0 24 24");
      icon.setAttribute("aria-hidden", "true");
      const pen = document.createElementNS("http://www.w3.org/2000/svg", "path");
      pen.setAttribute("d", "M4 20l1-4.2L16.6 4.2a1.7 1.7 0 0 1 2.4 0l.8.8a1.7 1.7 0 0 1 0 2.4L8.2 19ZM14.5 6.3l3.2 3.2");
      icon.append(pen);
      const line = el("span");
      line.textContent = "Click to write a note";
      hint.append(icon, line);
    }
    view.replaceChildren(hint);
    return;
  }
  view.replaceChildren(...parse(text).map(blockNode));
}

// ---- view <-> edit

function startEditing() {
  if (editing || !loaded) return;
  editing = true;
  editor.value = text;
  editor.hidden = false;
  view.hidden = true;
  editor.focus();
  editor.setSelectionRange(editor.value.length, editor.value.length);
}

function stopEditing() {
  if (!editing) return;
  editing = false;
  editor.hidden = true;
  view.hidden = false;
  flush();
  render();
}

view.addEventListener("click", (e) => {
  const task = e.target instanceof Element ? e.target.closest(".task") : null;
  if (task instanceof HTMLElement && task.dataset.line !== undefined) {
    change(toggleTask(text, Number(task.dataset.line)));
    render();
    flush();
    return;
  }
  startEditing();
});
view.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    startEditing();
  }
});
editor.addEventListener("input", () => change(editor.value));
editor.addEventListener("blur", stopEditing);
editor.addEventListener("keydown", (e) => {
  if (e.key === "Escape") editor.blur();
});

// ---- settings and first load

// Text size only: the page scale is the user's global appearance setting.
trackSettings({ fontScale: 1 }, (s) => {
  card.style.setProperty("--fs-mult", String(s.fontScale));
});

// Edit mode (moving the widget) ends any typing so a half-typed note is saved.
mypaper.on("editmode", (on) => {
  if (on) editor.blur();
});

mypaper.storage.get(KEY).then((saved) => {
  if (typeof saved === "string") text = saved;
  loaded = true;
  render();
}, (err) => {
  console.error("notes: load failed", err);
  loaded = true;
  render();
});
