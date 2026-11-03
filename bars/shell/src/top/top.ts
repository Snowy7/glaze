// The top bar: a strip of `config.top.height` DIPs; the rest of the window is for popovers.

import type { Bar } from "../bar";
import { h } from "../dom";
import { createScope } from "../host";
import { topLayout } from "../layout";
import type { BarsConfig } from "../protocol";
import { closeMenu } from "../menu";
import { layerRects } from "../overlay";
import { factories } from "./modules";

export function createTop(root: HTMLElement, config: BarsConfig): Bar {
  const scope = createScope();
  const strip = h("div", "top");
  strip.style.setProperty("--bar-h", `${config.top.height}px`);

  const layout = topLayout(config.top);
  for (const zone of ["left", "center", "right"] as const) {
    strip.append(h("div", `zone ${zone}`, ...layout[zone].map((name) => factories[name]({ config, scope }))));
  }
  root.append(strip);

  return {
    rects: () => [[0, 0, window.innerWidth, strip.offsetHeight], ...layerRects()],
    destroy: () => {
      closeMenu();
      scope.dispose();
      strip.remove();
    },
  };
}
