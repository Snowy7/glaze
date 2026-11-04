import { applyAppearance } from "./appearance";
import type { Bar } from "./bar";
import { dockStyles } from "./dock/styles";
import { initSurfaces, setSurfaceConfig } from "./flyouts";
import { connect, fire, on } from "./host";
import { initOverlay } from "./overlay";
import type { BarsConfig } from "./protocol";
import { createRegion } from "./region";
import "./style.css";
import { createTop } from "./top/top";

async function main(): Promise<void> {
  const hello = await connect();
  const html = document.documentElement;
  const app = document.getElementById("app");
  const overlay = document.getElementById("overlay");
  if (!app || !overlay) throw new Error("index.html is missing #app or #overlay");

  html.setAttribute("data-bar", hello.bar);
  applyAppearance(hello.appearance, hello.themeCss);

  const region = createRegion((rects) => fire("region", { rects }));
  initOverlay(overlay, region.invalidate);

  let bar: Bar | undefined;
  region.provide(() => bar?.rects() ?? []);
  initSurfaces(hello);
  const build = (config: BarsConfig) => {
    setSurfaceConfig(config);
    bar?.destroy();
    if (hello.bar === "top") {
      html.removeAttribute("data-position");
      html.removeAttribute("data-style");
      bar = createTop(app, config);
    } else {
      html.setAttribute("data-style", config.dock.style);
      bar = dockStyles[config.dock.style](app, config, { invalidateRegion: region.invalidate });
    }
    region.invalidate();
  };
  build(hello.config);

  on("config", build);
  on("appearance", ({ appearance, themeCss }) => applyAppearance(appearance, themeCss));
  // Hidden or suspended: stop animations (style.css).
  on("visibility", (visible) => html.classList.toggle("paused", !visible));
  window.addEventListener("pagehide", () => bar?.destroy());
}

main().catch((error: unknown) => console.error("[bars] failed to start", error));
