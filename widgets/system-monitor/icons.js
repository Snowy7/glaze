// @ts-check
// The System widget's icons: 24x24 outlines, stroked with currentColor. Constants only.

const svg = (/** @type {string} */ body) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

/** @type {Record<string, string>} */
export const ICONS = {
  cpu: svg('<rect x="6" y="6" width="12" height="12" rx="2.5"/><rect x="9.5" y="9.5" width="5" height="5" rx="1"/><path d="M9.5 3v3M14.5 3v3M9.5 18v3M14.5 18v3M3 9.5h3M3 14.5h3M18 9.5h3M18 14.5h3"/>'),
  ram: svg('<rect x="3" y="6.5" width="18" height="9" rx="2"/><path d="M7.5 10v2M12 10v2M16.5 10v2M6 15.5v2.5M10 15.5v2.5M14 15.5v2.5M18 15.5v2.5"/>'),
  gpu: svg('<rect x="2.5" y="6" width="19" height="11" rx="2.2"/><circle cx="9" cy="11.5" r="2.7"/><path d="M15 9.5h3M15 13.5h3M5.5 17v2.5"/>'),
  wifi: svg('<path d="M2.8 9.2a13.5 13.5 0 0 1 18.4 0M5.9 12.6a9 9 0 0 1 12.2 0M9 15.9a4.6 4.6 0 0 1 6 0"/><circle cx="12" cy="19" r="1.1" fill="currentColor" stroke="none"/>'),
  ethernet: svg('<rect x="4" y="8.5" width="16" height="11" rx="2"/><path d="M9 8.5V5.5h6v3M8.5 13v2.5M11 13v2.5M13 13v2.5M15.5 13v2.5"/>'),
  offline: svg('<path d="M3 3l18 18M8.9 15.9a4.6 4.6 0 0 1 6.1 0M2.8 9.2A13.5 13.5 0 0 1 7 6.5M10.6 5.6a13.5 13.5 0 0 1 10.6 3.6M5.9 12.6a9 9 0 0 1 3.4-2"/><circle cx="12" cy="19" r="1.1" fill="currentColor" stroke="none"/>'),
  disk: svg('<path d="M3.5 13.5 6 6.8A2 2 0 0 1 7.9 5.5h8.2a2 2 0 0 1 1.9 1.3l2.5 6.7"/><rect x="3.5" y="13.5" width="17" height="5.5" rx="1.8"/><path d="M7.5 16.25h.01M10.5 16.25h.01"/>'),
  battery: svg('<rect x="2.5" y="7" width="17" height="10" rx="2.6"/><path d="M22 10.5v3"/>'),
  bolt: svg('<path d="M13 3.5 6.5 13H12l-1 7.5L17.5 11H12z" fill="currentColor" stroke="none"/>'),
  pc: svg('<rect x="3" y="4.5" width="18" height="12" rx="2"/><path d="M8.5 20h7M12 16.5V20"/>'),
};
