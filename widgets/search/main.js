// @ts-check
import { $, trackSettings } from "./kit.js";
import { engineLabel, resolveTarget, templateFor } from "./url.js";

const ERROR_MS = 3500;

const form = $("#form", HTMLFormElement);
const input = $("#q", HTMLInputElement);
const engineEl = $("#engine");
const errorEl = $("#err");

const defaults = { engine: "duckduckgo", customUrl: "https://duckduckgo.com/?q={q}", openUrls: true };
let settings = defaults;

/** @type {ReturnType<typeof setTimeout> | undefined} */
let errorTimer;

/** @param {string} message */
function showError(message) {
  errorEl.textContent = message;
  errorEl.hidden = false;
  clearTimeout(errorTimer);
  errorTimer = setTimeout(clearError, ERROR_MS);
}

function clearError() {
  clearTimeout(errorTimer);
  errorEl.hidden = true;
}

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const target = resolveTarget({
    query: input.value,
    template: templateFor(settings.engine, settings.customUrl),
    openUrls: settings.openUrls,
  });
  if ("error" in target) {
    showError(target.error);
    return;
  }
  mypaper.launch(target.url).then(
    () => {
      clearError();
      input.value = "";
      input.blur();
    },
    (err) => showError(`Couldn't open the browser${err instanceof Error && err.message ? `: ${err.message}` : ""}`),
  );
});

input.addEventListener("input", clearError);
input.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (input.value) input.value = "";
  else input.blur();
  clearError();
});

// Edit mode (moving the widget) takes the keyboard away from the box.
mypaper.on("editmode", (editing) => {
  if (editing) input.blur();
});

trackSettings(defaults, (next) => {
  settings = next;
  engineEl.textContent = engineLabel(next.engine, next.customUrl);
});
