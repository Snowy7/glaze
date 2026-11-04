import { useEffect, useLayoutEffect, useRef } from "react";

import { folderUrl, type WidgetInfo } from "../../lib/api";
import { attachFrame, type FrameState } from "../../lib/widgetHost";

type Props = {
  widget: WidgetInfo;
  state: FrameState;
};

/**
 * One real widget page, at its real size, served from its folder. Studio answers its handshake
 * and calls (widgetHost.ts) and tells it about settings, size and look changes as they happen.
 * It never takes the pointer: the stage handles selecting and dragging.
 */
export function WidgetFrame({ widget, state }: Props) {
  const frame = useRef<HTMLIFrameElement>(null);
  const latest = useRef(state);
  latest.current = state;
  const bridge = useRef<ReturnType<typeof attachFrame> | null>(null);
  const url = folderUrl(widget.dir, widget.entry);

  // Attached before the page can say hello: the SDK doesn't ask twice.
  useLayoutEffect(() => {
    const element = frame.current;
    if (!element) return;
    const attached = attachFrame(element, () => latest.current);
    bridge.current = attached;
    return () => {
      attached.dispose();
      bridge.current = null;
    };
  }, [url]);

  const settings = JSON.stringify(state.settings);
  const size = JSON.stringify(state.size);
  const look = JSON.stringify(state.appearance);
  useEffect(() => bridge.current?.push("settings"), [settings]);
  useEffect(() => bridge.current?.push("size"), [size]);
  useEffect(() => bridge.current?.push("appearance"), [look]);

  return (
    <iframe
      ref={frame}
      src={url}
      title={widget.name}
      // The asset origin, not an opaque one: Tauri's asset responses only allow Studio's origin and
      // their own, and widgets load module scripts. It is still not Studio's origin, so the page
      // can't reach Studio's document or its commands.
      sandbox="allow-scripts allow-same-origin"
      tabIndex={-1}
      aria-hidden
      className="pointer-events-none absolute inset-0 size-full border-0 bg-transparent"
      style={{ colorScheme: "normal" }}
    />
  );
}
