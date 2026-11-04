// @ts-check
// Greeting: "Good morning, Ada" with the date (or another line) underneath. The words are in
// greeting.js; the layouts (hero, inline, stack) are pure CSS keyed on html[data-variant].
import { $, every, setText, trackSettings } from "./kit.js";
import { glyphSvg } from "./glyph.js";
import { greet, periodOf, secondaryLine, splitLead } from "./greeting.js";

const stage = $("#stage");
const glyph = $("#glyph");
const lead = $("#lead");
const nameEl = $("#name");
const sub = $("#sub");

/** @typedef {import("./greeting.js").Wording} Wording */
/** @typedef {import("./greeting.js").Line} Line */

const VARIANTS = ["hero", "inline", "stack"];
/** @type {readonly Wording[]} */
const WORDINGS = ["friendly", "formal", "short"];
/** @type {readonly Line[]} */
const LINES = ["date", "progress", "custom", "none"];

const defaults = { variant: "hero", wording: "friendly", name: "", line: "date", custom: "" };
let settings = defaults;
/** The Windows display name; "" until it arrives, or when the permission is denied. */
let accountName = "";
let drawnPeriod = "";

/** @template {string} T @param {readonly T[]} allowed @param {string} value @returns {T} */
function oneOf(allowed, value) {
  return allowed.find((a) => a === value) ?? /** @type {T} */ (allowed[0]);
}

function render() {
  const now = new Date();
  const period = periodOf(now.getHours());
  document.documentElement.dataset["variant"] = oneOf(VARIANTS, settings.variant);

  const typed = settings.name.trim();
  const words = greet(period, oneOf(WORDINGS, settings.wording), typed || accountName);
  const named = words.name !== "";
  // Without a name the last word of the greeting takes the accent: "Good" / "morning".
  const [before, accent] = named ? [words.lead, words.name] : splitLead(words.lead);
  stage.dataset["named"] = String(named);
  setText(lead, before);
  $(".lead").hidden = before === "";
  setText(nameEl, accent);

  if (period !== drawnPeriod) {
    drawnPeriod = period;
    glyph.innerHTML = glyphSvg(period);
  }
  const line = secondaryLine(oneOf(LINES, settings.line), now, settings.custom);
  setText(sub, line);
  sub.hidden = line === "";
}

trackSettings(defaults, (next) => {
  settings = next;
  render();
});

// The name is asked once. A denied permission or an empty name just means no name.
void mypaper.user
  .info()
  .then((info) => {
    accountName = info.name || info.account;
    render();
  })
  .catch(() => {});

// Once a minute while visible: enough for the hour boundary, the date and the day's progress.
every(60_000, render);
