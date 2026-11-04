// @ts-check
// Quick toggles: tiles in the style of a phone's quick settings. Be clear about what they do: a
// tile OPENS the matching Windows Settings page (`ms-settings:`), it does not flip the system
// state, because a widget has no safe way to do that. Wi-Fi and the battery tile also show live
// status from `system.subscribe`; the others have no state a widget could read.
import { $, trackSettings } from "./kit.js";
import { iconSvg } from "./icons.js";
import { bestGrid, listColumns, stateOf, TILES } from "./toggles.js";

/** @typedef {import("./toggles.js").TileId} TileId */
/** @typedef {import("./toggles.js").Stats} Stats */

const area = $("#area");

const GAP = 8;
const LIST_GAP = 5;
const VARIANTS = ["grid", "row", "list"];

const defaults = {
  variant: "grid",
  wifi: true,
  bluetooth: true,
  focus: true,
  airplane: true,
  nightlight: true,
  sound: true,
  display: true,
  battery: true,
};
let settings = defaults;
/** @type {Stats | undefined} */
let stats;
let box = { w: 0, h: 0 };

/**
 * The tile elements of the current layout, with what is drawn in them (so a live update only
 * touches the DOM when something changed).
 * @type {Map<TileId, { button: HTMLButtonElement, icon: HTMLElement, label: HTMLElement, status: HTMLElement, drawn: string }>}
 */
let nodes = new Map();
/** What the DOM was built for. */
let builtKey = "";

/** The enabled tiles, in catalogue order. */
const enabled = () => TILES.filter((t) => settings[t.id]);

/** @param {HTMLButtonElement} button */
function flashFailure(button) {
  button.dataset["failed"] = "";
  setTimeout(() => delete button.dataset["failed"], 700);
}

/** Builds the tiles of the current variant. */
function build() {
  const variant = settings.variant;
  const tiles = enabled();
  nodes = new Map();
  area.className = `area v-${variant}`;
  area.style.setProperty("--n", String(tiles.length));

  const buttons = tiles.map((tile) => {
    const button = document.createElement("button");
    button.className = variant === "row" ? "qt" : "qt tile";
    button.title = `Open ${tile.label} settings`;
    button.setAttribute("aria-label", `${tile.label}: open settings`);
    const icon = document.createElement("span");
    icon.className = "ic";
    const label = document.createElement("b");
    label.className = "lb";
    const status = document.createElement("span");
    status.className = "st";
    const text = document.createElement("span");
    text.className = "tx";
    text.append(label, status);
    button.append(icon, text);
    button.addEventListener("click", () => {
      button.dataset["press"] = "";
      setTimeout(() => delete button.dataset["press"], 260);
      mypaper.launch(tile.uri).catch(() => flashFailure(button));
    });
    nodes.set(tile.id, { button, icon, label, status, drawn: "" });
    return button;
  });

  if (variant === "row") {
    const bar = document.createElement("div");
    bar.className = "bar tile";
    bar.append(...buttons);
    area.replaceChildren(bar);
  } else {
    area.replaceChildren(...buttons);
  }
  update();
  layout();
}

/** Applies the live state to the tiles. */
function update() {
  for (const tile of TILES) {
    const node = nodes.get(tile.id);
    if (!node) continue;
    const state = stateOf(tile.id, stats);
    node.button.toggleAttribute("data-on", state.on);
    node.label.textContent = state.label ?? tile.label;
    node.status.textContent = state.status;
    node.button.toggleAttribute("data-quiet", state.status === "");
    // The icon only changes with the signal or the charge.
    const drawn = `${state.link ?? ""}${state.bars ?? ""}${state.level === undefined ? "" : Math.round(state.level * 12)}${state.charging ? "+" : ""}`;
    if (drawn !== node.drawn || node.icon.childElementCount === 0) {
      node.icon.innerHTML = iconSvg(tile.id, state);
      node.drawn = drawn;
    }
  }
}

/** Sets the columns and rows from the box. */
function layout() {
  const count = nodes.size;
  if (settings.variant === "grid") {
    const { cols, rows } = bestGrid(count, box.w, box.h, GAP);
    area.style.setProperty("--cols", String(cols));
    area.style.setProperty("--rows", String(rows));
  } else if (settings.variant === "list") {
    const cols = listColumns(count, box.w, box.h, LIST_GAP);
    area.style.setProperty("--cols", String(cols));
    area.style.setProperty("--rows", String(Math.ceil(count / cols)));
  }
}

/** Rebuilds when the layout or the set of tiles changed, otherwise just refreshes. */
function render() {
  const variant = VARIANTS.includes(settings.variant) ? settings.variant : "grid";
  settings = { ...settings, variant };
  const key = `${variant}:${enabled().map((t) => t.id).join(",")}`;
  if (key !== builtKey) {
    builtKey = key;
    build();
  }
}

new ResizeObserver(([entry]) => {
  if (!entry) return;
  box = { w: entry.contentRect.width, h: entry.contentRect.height };
  layout();
}).observe(area);

trackSettings(defaults, (next) => {
  settings = next;
  render();
});

// A first sample right away; the subscription then keeps it fresh.
void mypaper.system.stats().then((s) => {
  stats = s;
  update();
});
// The host drives one timer for all widgets, and pauses it while hidden.
mypaper.system.subscribe(5000, (s) => {
  stats = s;
  update();
});
