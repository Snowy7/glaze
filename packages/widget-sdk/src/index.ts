import { createMypaper, type Transport } from "./runtime";
import type { Mypaper } from "./types";

export * from "./types";
export {
  DEFAULT_APPEARANCE,
  appearanceAttributes,
  appearanceRule,
  appearanceVars,
  applyAppearance,
  WIDGET_STYLES,
  normalizeAppearance,
  sparkColors,
  withToneDefaults,
} from "./appearance";
export { MypaperError, isMypaperError } from "./errors";
export { isLiquid, plateOf, trackPlate, type PlateTracker } from "./plate";
export { audioBins, createMypaper, type RuntimeOptions, type Transport } from "./runtime";

/** postMessage to the shell page that frames this widget. */
export function windowTransport(): Transport {
  const shell = window.parent;
  // Unframed (opened straight in a browser): nobody to talk to, so the handshake times out
  // with a clear error instead of echoing our own messages.
  const framed = shell !== window;
  return {
    send(message) {
      if (framed) shell.postMessage(message, "*");
    },
    listen(handler) {
      const onMessage = (e: MessageEvent) => {
        if (e.source === shell) handler(e.data);
      };
      window.addEventListener("message", onMessage);
      return () => window.removeEventListener("message", onMessage);
    },
  };
}

let shared: Mypaper | undefined;

/** The `mypaper` object for this page (created once). */
export function connect(): Mypaper {
  return (shared ??= createMypaper(windowTransport()));
}
