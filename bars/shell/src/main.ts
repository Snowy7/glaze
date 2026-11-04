import type { Bar } from "./bar";
import { dockStyles } from "./dock/styles";
import { initSurfaces, setSurfaceConfig } from "./flyouts";
import { connect, fire, on } from "./host";
import { createGlass } from "./glass";
import { initOverlay } from "./overlay";
import type { BarsConfig } from "./protocol";
import { createRegion } from "./region";
import { applyLook, type PageLook } from "./style/apply";
import "./style.css";
import "./designs/marks.css";
import "./designs/top.css";
import "./designs/dock.css";
import { createTop } from "./top/top";

async function main(): Promise<void> {
  const hello = await connect();
  const html = document.documentElement;
  const app = document.getElementById("app");
  const overlay = document.getElementById("overlay");
  if (!app || !overlay) throw new Error("index.html is missing #app or #overlay");

  const state: PageLook = {
    kind: hello.bar,
    config: hello.config,
    look: hello.look,
    mode: hello.mode,
    backdrop: hello.backdrop,
  };
  applyLook(state);

  const region = createRegion((rects) => fire("region", { rects }));
  const glass = createGlass((panes) => fire("glass", { panes }));
  // Whatever moves the region may move glass too (glass also follows animations by itself).
  const invalidate = () => {
    region.invalidate();
    glass.invalidate();
  };
  initOverlay(overlay, invalidate);

  let bar: Bar | undefined;
  let pack = JSON.stringify(state.look.iconPack);
  region.provide(() => bar?.rects() ?? []);
  initSurfaces(hello);
  const build = () => {
    setSurfaceConfig(state.config);
    bar?.destroy();
    const ctx = { invalidateRegion: invalidate, iconPack: state.look.iconPack };
    bar =
      state.kind === "top" ? createTop(app, state.config) : dockStyles[state.config.dock.style](app, state.config, ctx);
    invalidate();
  };
  build();
  // The bar comes in with its `show` animation (data-m-show), once.
  html.classList.add("entering");
  window.setTimeout(() => html.classList.remove("entering"), 2000);

  on("config", (config: BarsConfig) => {
    state.config = config;
    applyLook(state);
    build();
  });
  on("appearance", (look) => {
    state.look = look;
    applyLook(state);
    glass.invalidate();
    const next = JSON.stringify(look.iconPack);
    if (next !== pack) {
      pack = next;
      build(); // app icons come from the pack
    }
  });
  on("mode", (mode) => {
    state.mode = mode;
    applyLook(state);
    glass.invalidate();
  });
  on("backdrop", (backdrop) => {
    state.backdrop = backdrop;
    applyLook(state);
  });
  // Hidden or suspended: stop animations (style.css); coming back plays the show animation.
  on("visibility", (visible) => {
    html.classList.toggle("paused", !visible);
    if (visible) {
      html.classList.remove("entering");
      void html.offsetWidth;
      html.classList.add("entering");
    }
  });
  html.addEventListener("animationend", (event) => {
    if (event.animationName.startsWith("bar-in")) html.classList.remove("entering");
  });
  window.addEventListener("pagehide", () => bar?.destroy());
}

main().catch((error: unknown) => console.error("[bars] failed to start", error));
