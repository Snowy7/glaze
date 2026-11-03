// The WebView2 message channel to the Rust host (docs/widgets.md, "Host ⇄ shell").

import type { HostToShell, ShellToHost } from "./protocol";

interface ChromeWebview {
  postMessage(message: ShellToHost): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: ChromeWebview };
  }
}

export function send(message: ShellToHost): void {
  window.chrome?.webview?.postMessage(message);
}

function isHostMessage(data: unknown): data is HostToShell {
  return typeof data === "object" && data !== null && "type" in data && typeof data.type === "string";
}

/** Calls `cb` for every message from the host. The host is trusted: only the envelope is checked. */
export function listen(cb: (message: HostToShell) => void): void {
  window.chrome?.webview?.addEventListener("message", (event) => {
    if (isHostMessage(event.data)) cb(event.data);
  });
}
