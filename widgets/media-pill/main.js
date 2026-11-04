// @ts-check
// Now playing pill: artwork, a title that scrolls only when it does not fit, compact transport and a
// progress hairline. Meant to sit as an overlay (manifest preset "overlay"). The shape (pill, notch,
// mini) is CSS keyed on html[data-variant]; the pure logic is in pill.js.
import { $, every, setText, trackSettings } from "./kit.js";
import { describe, formatClock, marqueeSeconds, needsMarquee, progressAt } from "./pill.js";

/** @typedef {NonNullable<Awaited<ReturnType<typeof mypaper.media.current>>>} Media */

const card = $("#card");
const cover = $("#cover", HTMLImageElement);
const titleBox = $("#title");
const mq = $("#mq");
const artist = $("#artist");
const bar = $("#bar");
const meta = $("#meta");

const VARIANTS = ["pill", "notch", "mini"];
const defaults = { variant: "pill", progress: true };
let settings = defaults;
/** @type {Media | null} */
let media = null;
/** When the media state arrived: its position is as of then. */
let sampledAt = 0;
/** The last text and artwork drawn, so an update only touches what changed. */
let drawn = { text: "", art: "" };

/** Scrolls the title only if it overflows its box (the keyframes are in style.css). */
function fitTitle() {
  const overflow = mq.scrollWidth - titleBox.clientWidth;
  const run = needsMarquee(mq.scrollWidth, titleBox.clientWidth);
  mq.classList.toggle("run", run);
  if (run) {
    mq.style.setProperty("--dist", `${Math.ceil(overflow) + 4}px`);
    mq.style.setProperty("--dur", `${marqueeSeconds(overflow)}s`);
  }
}

function drawProgress() {
  const fraction = media ? progressAt({ position: media.position, duration: media.duration, playing: media.playing, at: sampledAt }, performance.now()) : null;
  card.toggleAttribute("data-nobar", fraction === null || !settings.progress);
  bar.style.transform = `scaleX(${fraction ?? 0})`;
}

function render() {
  document.documentElement.dataset["variant"] = VARIANTS.includes(settings.variant) ? settings.variant : "pill";
  const idle = media === null;
  card.toggleAttribute("data-idle", idle);
  card.toggleAttribute("data-playing", media?.playing === true);

  if (media === null) {
    setText(mq, "Nothing playing");
    setText(artist, "");
    cover.hidden = true;
    card.title = "";
    drawn = { text: "", art: "" };
  } else {
    const lines = describe(media);
    const text = `${lines.title}\n${lines.artist}`;
    if (text !== drawn.text) {
      drawn.text = text;
      setText(mq, lines.title);
      setText(artist, lines.artist);
    }
    const art = media.artwork ?? "";
    if (art !== drawn.art) {
      drawn.art = art;
      if (art) cover.src = art;
      else cover.removeAttribute("src");
      cover.hidden = art === "";
    }
    const clock = media.duration ? ` · ${formatClock((media.position ?? 0) + (media.playing ? (performance.now() - sampledAt) / 1000 : 0))} / ${formatClock(media.duration)}` : "";
    card.title = `${lines.title}${lines.artist ? ` - ${lines.artist}` : ""}${clock}`;
  }
  artist.hidden = artist.textContent === "";
  fitTitle();
  drawProgress();
}

for (const [id, action] of /** @type {const} */ ([
  ["#prev", "previous"],
  ["#play", "toggle"],
  ["#next", "next"],
])) {
  $(id).addEventListener("click", () => void mypaper.media.control(action).catch(() => {}));
}

// The title's box changes with the widget's width.
new ResizeObserver(fitTitle).observe(meta);

trackSettings(defaults, (next) => {
  settings = next;
  render();
});

/** @param {Media | null} next */
function apply(next) {
  media = next;
  sampledAt = performance.now();
  render();
}
void mypaper.media.current().then(apply, () => apply(null));
mypaper.media.onChange(apply);

// The position is only sent when something changes: move the hairline on once a second while playing.
every(1000, () => {
  if (media?.playing) drawProgress();
});
