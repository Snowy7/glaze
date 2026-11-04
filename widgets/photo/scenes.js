// @ts-check
// The built-in pictures shown until the user adds their own: three painted-looking SVG landscapes.
// Their colors are artwork, so they are not from the kit's variables. Each is an image data URL, so
// it goes through exactly the same Ken Burns and crossfade as a real photo.

/** @param {string} defs @param {string} body */
const svg = (defs, body) =>
  `data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 800" width="1200" height="800"><defs>${defs}</defs>${body}</svg>`,
  )}`;

/** A vertical gradient: [offset, color, opacity?] stops. @param {string} id @param {[number, string, number?][]} stops */
const vertical = (id, stops) =>
  `<linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">${stops.map(([o, c, a = 1]) => `<stop offset="${o}" stop-color="${c}" stop-opacity="${a}"/>`).join("")}</linearGradient>`;

/** A soft light: a color fading to nothing. @param {string} id @param {string} color @param {number} [peak] */
const glow = (id, color, peak = 0.9) =>
  `<radialGradient id="${id}"><stop offset="0" stop-color="${color}" stop-opacity="${peak}"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></radialGradient>`;

const dunes = svg(
  vertical("sky", [[0, "#1a1446"], [0.42, "#5e2a7e"], [0.7, "#e0566f"], [1, "#ffb86b"]]) +
    glow("sun", "#ffc47a"),
  `<rect width="1200" height="800" fill="url(#sky)"/>
   <g fill="#fff" opacity=".7"><circle cx="140" cy="90" r="1.6"/><circle cx="310" cy="160" r="1.2"/><circle cx="520" cy="70" r="1.8"/><circle cx="900" cy="120" r="1.4"/><circle cx="1080" cy="60" r="1.6"/><circle cx="700" cy="190" r="1"/><circle cx="60" cy="230" r="1.1"/></g>
   <circle cx="780" cy="480" r="300" fill="url(#sun)"/>
   <circle cx="780" cy="480" r="52" fill="#fff2cc"/>
   <path d="M0 530C170 480 330 505 520 545S900 565 1200 505V800H0Z" fill="#84306c"/>
   <path d="M0 595C220 545 430 605 650 625S1010 605 1200 575V800H0Z" fill="#552064"/>
   <path d="M0 665C260 625 490 695 730 705S1060 685 1200 655V800H0Z" fill="#2f1654"/>
   <path d="M0 740C300 700 570 762 830 766S1110 752 1200 738V800H0Z" fill="#170b36"/>`,
);

const ridges = svg(
  vertical("sky", [[0, "#7fb2d6"], [0.55, "#cfe4ea"], [1, "#fbe3c9"]]) +
    glow("sun", "#fff4dc", 0.95) +
    // The mist fades in toward the bottom of each ridge, where the next one starts.
    vertical("mist", [[0, "#fdf1e2", 0], [1, "#fdf1e2", 0.92]]) +
    `<filter id="soft"><feGaussianBlur stdDeviation="1.6"/></filter>`,
  `<rect width="1200" height="800" fill="url(#sky)"/>
   <circle cx="330" cy="260" r="210" fill="url(#sun)"/>
   <circle cx="330" cy="260" r="40" fill="#fffaf0"/>
   <path filter="url(#soft)" d="M0 440C80 400 130 360 200 370S300 330 380 300C450 330 500 400 560 400S660 340 720 330C800 360 850 430 920 400S1040 350 1100 370 1160 410 1200 400V800H0Z" fill="#b3c4dc"/>
   <rect y="370" width="1200" height="190" fill="url(#mist)"/>
   <path filter="url(#soft)" d="M0 540C90 500 150 440 230 450S350 500 430 470C500 420 560 400 620 430S720 530 800 500C880 460 950 450 1020 490S1130 520 1200 480V800H0Z" fill="#7d97bd"/>
   <rect y="450" width="1200" height="200" fill="url(#mist)"/>
   <path d="M0 640C100 600 180 540 270 560S400 620 490 590C570 550 640 520 720 560S840 650 930 620C1010 590 1100 570 1200 610V800H0Z" fill="#4a6892"/>
   <rect y="560" width="1200" height="180" fill="url(#mist)" opacity=".8"/>
   <path d="M0 720C120 690 220 650 330 670S500 730 620 700C730 670 800 640 900 670S1080 740 1200 700V800H0Z" fill="#27406b"/>
   <path d="M0 780C200 750 380 790 600 770S980 750 1200 785V800H0Z" fill="#12213f"/>`,
);

const tide = svg(
  vertical("sky", [[0, "#2a2862"], [0.4, "#b9548a"], [0.72, "#ffa56a"], [1, "#ffdc8a"]]) +
    vertical("sea", [[0, "#ffc27a"], [0.18, "#d4668c"], [0.6, "#35407f"], [1, "#161d4a"]]) +
    glow("sun", "#ffe2a0", 0.95),
  `<rect width="1200" height="800" fill="url(#sky)"/>
   <circle cx="600" cy="430" r="280" fill="url(#sun)"/>
   <circle cx="600" cy="430" r="86" fill="#fff0c6"/>
   <path d="M880 430C920 396 970 392 1010 408 1050 392 1100 400 1130 430Z" fill="#3a2d6b" opacity=".85"/>
   <rect y="430" width="1200" height="370" fill="url(#sea)"/>
   <g fill="#ffe9b8"><rect x="510" y="446" width="180" height="5" rx="2.5" opacity=".85"/><rect x="540" y="466" width="120" height="5" rx="2.5" opacity=".7"/><rect x="480" y="488" width="240" height="6" rx="3" opacity=".55"/><rect x="520" y="514" width="160" height="6" rx="3" opacity=".42"/><rect x="450" y="544" width="300" height="7" rx="3.5" opacity=".3"/><rect x="500" y="580" width="200" height="8" rx="4" opacity=".2"/></g>
   <g fill="#ffffff" opacity=".12"><rect x="60" y="520" width="200" height="4" rx="2"/><rect x="900" y="560" width="240" height="5" rx="2.5"/><rect x="200" y="640" width="320" height="5" rx="2.5"/><rect x="760" y="700" width="300" height="6" rx="3"/></g>`,
);

/** @type {readonly { src: string, title: string }[]} */
export const SCENES = [
  { src: dunes, title: "Dusk dunes" },
  { src: ridges, title: "Misty ridges" },
  { src: tide, title: "Morning tide" },
];
