// @ts-check
// An analog dial: a disc (a kit `.tile`, so every style gives it its own body) under an SVG in a
// 100x100 box with ticks, optional quarter numerals and date window, and the hands. The Analog
// face shows one big dial; the World face one small dial per city. Markup comes from constants.

const NS = "http://www.w3.org/2000/svg";

/**
 * @param {string} name
 * @param {Record<string, string | number>} [attrs]
 */
function svg(name, attrs = {}) {
  const node = document.createElementNS(NS, name);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
  return node;
}

/** @param {number} deg */
const rotate = (deg) => `rotate(${deg.toFixed(2)}deg)`;

/**
 * A hand: a thin neck from the hub, then the body. Drawn pointing at 12, turned by CSS.
 * @param {string} cls
 * @param {number} neck where the body starts (y)
 * @param {number} tip
 */
function hand(cls, neck, tip) {
  const g = svg("g", { class: `hand ${cls}` });
  g.append(svg("line", { class: "neck", x1: 50, y1: 50, x2: 50, y2: neck }), svg("line", { class: "body", x1: 50, y1: neck, x2: 50, y2: tip }));
  return g;
}

/**
 * @param {{ numerals?: boolean, seconds?: boolean, date?: boolean }} [opts]
 */
export function createDial(opts = {}) {
  const el = document.createElement("div");
  el.className = "dial";
  const disc = document.createElement("div");
  disc.className = "disc tile";
  const face = svg("svg", { class: "dial-svg", viewBox: "0 0 100 100", "aria-hidden": "true" });

  const ticks = svg("g", { class: "ticks" });
  for (let i = 0; i < 60; i++) {
    const major = i % 5 === 0;
    ticks.append(
      svg("line", {
        class: major ? (i === 0 ? "tick major twelve" : "tick major") : "tick minor",
        x1: 50,
        x2: 50,
        y1: 3.5,
        y2: major ? 9.5 : 6,
        transform: `rotate(${i * 6} 50 50)`,
      }),
    );
  }
  const numerals = svg("g", { class: "numerals" });
  for (const hour of [12, 3, 6, 9]) {
    const angle = (hour % 12) * 30 * (Math.PI / 180);
    const text = svg("text", { x: (50 + 33 * Math.sin(angle)).toFixed(2), y: (50 - 33 * Math.cos(angle)).toFixed(2) });
    text.textContent = String(hour);
    numerals.append(text);
  }
  const dateBg = svg("rect", { class: "dial-date-bg", x: 38, y: 64.5, width: 24, height: 8, rx: 4 });
  const date = svg("text", { class: "dial-date", x: 50, y: 68.6 });

  const hour = hand("hour", 44, 27);
  const minute = hand("minute", 44, 12);
  const second = svg("g", { class: "hand second" });
  second.append(svg("line", { x1: 50, y1: 61, x2: 50, y2: 8 }), svg("circle", { class: "hub-sec", cx: 50, cy: 50, r: 1.9 }));
  const hub = svg("circle", { class: "hub", cx: 50, cy: 50, r: 2.6 });
  face.append(ticks, numerals, dateBg, date, hour, minute, hub, second);
  el.append(disc, face);

  let secAngle = 0;
  let snap = true;

  /** Which parts show. @param {{ numerals?: boolean, seconds?: boolean, date?: boolean }} next */
  const configure = (next) => {
    el.classList.toggle("has-numerals", Boolean(next.numerals));
    el.classList.toggle("has-seconds", Boolean(next.seconds));
    el.classList.toggle("has-date", Boolean(next.date));
    snap = true;
  };
  configure(opts);

  return {
    el,
    configure,
    /**
     * Turns the hands. The seconds hand keeps an unwrapped angle so its transition never swings
     * back at 59 to 0, and jumps (no transition) after a pause or a change.
     * @param {number} h
     * @param {number} m
     * @param {number} s
     */
    set(h, m, s) {
      const minutes = m + s / 60;
      hour.style.transform = rotate(((h % 12) + minutes / 60) * 30);
      minute.style.transform = rotate(minutes * 6);
      if (!el.classList.contains("has-seconds")) return;
      const target = s * 6;
      let delta = target - (secAngle % 360);
      if (delta < -180) delta += 360;
      secAngle = snap ? target : secAngle + delta;
      second.classList.toggle("snap", snap);
      second.style.transform = rotate(secAngle);
      if (snap) {
        void second.getBoundingClientRect(); // commit the jump before the transition returns
        second.classList.remove("snap");
        snap = false;
      }
    },
    /** @param {string} text */
    setDate(text) {
      if (date.textContent !== text) date.textContent = text;
    },
    /** The next `set` jumps instead of sweeping (after the widget was hidden). */
    resync() {
      snap = true;
    },
  };
}
