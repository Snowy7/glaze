// @ts-check
// The Visualizer look's spectrum: one canvas, as bars or a wave, fed by `mypaper.audio.spectrum`.
// It listens only while the look is on and the widget is visible, draws at most 30 frames a
// second, and stops drawing once silence has settled. Geometry and smoothing are in viz.js.
import { fitCanvas } from "./kit.js";
import { barLayout, barSpan, curveSegments, hasSettled, idleLevels, isSilent, sampleBars, shape, smoothInto, wavePoints } from "./viz.js";

const FRAME_MS = 1000 / 30;
const SMOOTHING = 0.5;

/**
 * @param {HTMLCanvasElement} canvas
 * @param {(silent: boolean) => void} onSilence told when the sound starts or stops
 */
export function createSpectrum(canvas, onSilence) {
  const ctx = canvas.getContext("2d");
  let width = 0;
  let height = 0;
  let style = "bars";
  let sensitivity = 1.2;
  let colors = { hi: "#fff", hi2: "#fff" };
  /** @type {number[]} */
  let levels = [];
  /** @type {number[]} */
  let targets = [];
  /** @type {number[]} */
  let latest = [];
  let silent = true;
  let active = false;
  let visible = false;
  let raf = 0;
  let last = 0;
  /** @type {(() => void) | undefined} */
  let unsubscribe;

  const count = () => (style === "wave" ? Math.max(12, Math.min(64, Math.round(width / 9))) : barLayout(width, Math.max(3, width / 90), 3).count);

  const retarget = () => {
    const n = count();
    if (levels.length !== n) levels = idleLevels(n);
    const source = silent ? idleLevels(n) : sampleBars(latest, n);
    targets = silent ? source : source.map((v, i) => shape(v, n <= 1 ? 0 : i / (n - 1), sensitivity));
  };

  /** @param {CanvasRenderingContext2D} c */
  const gradient = (c) => {
    const g = c.createLinearGradient(0, 0, width, 0);
    g.addColorStop(0, colors.hi);
    g.addColorStop(1, colors.hi2);
    return g;
  };

  const draw = () => {
    if (!ctx || width < 1) return;
    const dpr = canvas.width / width;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = gradient(ctx);
    if (style === "wave") {
      const points = wavePoints(levels, width, height - 1, height * 0.92);
      const first = points[0];
      if (!first) return;
      ctx.beginPath();
      ctx.moveTo(first.x, height);
      ctx.lineTo(first.x, first.y);
      for (const s of curveSegments(points)) ctx.bezierCurveTo(s.c1x, s.c1y, s.c2x, s.c2y, s.x, s.y);
      ctx.lineTo(width, height);
      ctx.closePath();
      ctx.globalAlpha = 0.85;
      ctx.fill();
      ctx.globalAlpha = 1;
      return;
    }
    const mirror = style === "mirror";
    const { count: n, pitch, bar } = barLayout(width, Math.max(3, width / 90), 3);
    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const span = barSpan(levels[i] ?? 0, height, mirror, Math.min(bar, height));
      ctx.roundRect(i * pitch, span.y, bar, span.h, bar / 2);
    }
    ctx.fill();
  };

  /** @param {number} now */
  const tick = (now) => {
    raf = requestAnimationFrame(tick);
    if (now - last < FRAME_MS - 1) return;
    const dt = last === 0 ? FRAME_MS : Math.min(100, now - last);
    last = now;
    smoothInto(levels, targets, dt, SMOOTHING);
    draw();
    // Silence has settled: nothing moves, so nothing draws until sound returns.
    if (silent && hasSettled(levels, targets)) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  };
  const wake = () => {
    if (raf || !active || !visible) return;
    last = 0;
    raf = requestAnimationFrame(tick);
  };

  /** @param {number[]} bins */
  const onBins = (bins) => {
    latest = bins;
    const now = isSilent(bins);
    if (now !== silent) onSilence(now);
    silent = now;
    retarget();
    wake();
  };

  // Subscribed only while the look is on and the widget visible: the host captures audio only
  // for visible subscribers, so this is what keeps the cost at zero otherwise.
  const sync = () => {
    const want = active && visible;
    if (want && !unsubscribe) unsubscribe = mypaper.audio.spectrum(onBins, { bins: 64 });
    if (!want) {
      unsubscribe?.();
      unsubscribe = undefined;
      cancelAnimationFrame(raf);
      raf = 0;
      if (!silent) onSilence(true);
      silent = true;
    }
  };

  fitCanvas(canvas, (w, h) => {
    width = w;
    height = h;
    retarget();
    draw();
  });

  return {
    /** @param {boolean} on the Visualizer look is showing */
    setActive(on) {
      active = on;
      sync();
      retarget();
      draw();
    },
    /** @param {boolean} on */
    setVisible(on) {
      visible = on;
      sync();
    },
    /** @param {{ style: string, sensitivity: number }} next */
    configure(next) {
      style = next.style;
      sensitivity = Math.min(2.5, Math.max(0.5, next.sensitivity));
      levels = [];
      retarget();
      draw();
    },
    /** @param {string} hi @param {string} hi2 resolved CSS colors */
    setColors(hi, hi2) {
      colors = { hi, hi2 };
      draw();
    },
  };
}
