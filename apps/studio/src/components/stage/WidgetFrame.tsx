import type { WidgetInfo } from "../../lib/api";
import type { FrameState } from "../../lib/widgetHost";

type Props = {
  widget: WidgetInfo;
  state: FrameState;
};

/** Layout placeholder until N7 embeds native previews. Settings keep their original schemas. */
export function WidgetFrame({ widget }: Props) {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 flex size-full items-center justify-center rounded-2xl border border-white/15 bg-black/25 text-sm text-white/70"
    >
      {widget.name}
    </div>
  );
}
