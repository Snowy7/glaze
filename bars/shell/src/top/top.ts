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
  // Every module carries `data-module="<name>"` and every zone `data-zone`: the hooks theme CSS
  // and presets style by (docs/bars.md, "Styling").
  const build = (name: (typeof layout)["left"][number]) => {
    const el = factories[name]({ config, scope });
    el.dataset.module = name;
    return el;
  };
  for (const zone of ["left", "center", "right"] as const) {
    const el = h("div", `zone ${zone}`, ...layout[zone].map(build));
    el.dataset.zone = zone;
    strip.append(el);
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
