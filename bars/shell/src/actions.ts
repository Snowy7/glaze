// Click and wheel actions (docs/bars.md, "Modules"): `bars.json` → `modules.<name>.click` binds a
// module's left, middle and right click and wheel notches to a built-in action, or to `exec` /
// `open`, which the host runs from its own copy of the config (`run`). Unbound inputs keep the
// module's own behavior: the binding listens in the capture phase on the module and only stops
// the inputs it takes over.

import { fire, request } from "./host";
import type { Action, BuiltinAction, ModuleName, ModuleOptions, Trigger } from "./protocol";

const VOLUME_STEP = 0.04;

export const BUILTIN: Record<Exclude<BuiltinAction, "default">, () => void> = {
  none: () => undefined,
  start: () => fire("start"),
  search: () => fire("search"),
  notifications: () => fire("actionCenter", { panel: "notifications" }),
  "quick-settings": () => fire("actionCenter", { panel: "quick" }),
  "task-manager": () => fire("launch", { path: "taskmgr.exe" }),
  settings: () => fire("openSettings", { uri: "ms-settings:" }),
  "show-desktop": () => fire("showDesktop"),
  "media-toggle": () => fire("media", { action: "toggle" }),
  "media-next": () => fire("media", { action: "next" }),
  "media-previous": () => fire("media", { action: "previous" }),
  "volume-up": () => fire("volume", { delta: VOLUME_STEP }),
  "volume-down": () => fire("volume", { delta: -VOLUME_STEP }),
  "volume-mute": () => {
    request("volume", {})
      .then((state) => fire("volume", { mute: !state.muted }))
      .catch((error: unknown) => console.warn("[bars] volume failed", error));
  },
  "workspace-next": () => fire("workspace", { direction: "right" }),
  "workspace-previous": () => fire("workspace", { direction: "left" }),
  lock: () => fire("power", { action: "lock" }),
  explorer: () => fire("launch", { path: "explorer.exe" }),
};

/** The action bound to an input, or undefined when the module keeps its own behavior. */
export function boundAction(options: ModuleOptions | undefined, trigger: Trigger): Action | undefined {
  const action = options?.click[trigger];
  return action === "default" ? undefined : action;
}

/** Which wheel trigger a wheel event is, if it scrolls at all. */
export function wheelTrigger(deltaY: number): Trigger | undefined {
  return deltaY < 0 ? "wheelUp" : deltaY > 0 ? "wheelDown" : undefined;
}

export function runAction(module: ModuleName, trigger: Trigger, action: Action): void {
  if (typeof action === "string") {
    if (action !== "default") BUILTIN[action]();
  } else {
    fire("run", { module, trigger });
  }
}

/** Wheel notches closer than this count as one (a flick shouldn't run an action ten times). */
const WHEEL_SPACING = 120;

/** Binds `module`'s configured inputs on `el`. Returns the unbinding. */
export function bindActions(el: HTMLElement, module: ModuleName, options: ModuleOptions | undefined): () => void {
  if (!options || Object.keys(options.click).length === 0) return () => undefined;
  let lastWheel = 0;
  const take = (event: Event, trigger: Trigger | undefined): void => {
    const action = trigger && boundAction(options, trigger);
    if (!trigger || !action) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.type === "wheel") {
      const now = performance.now();
      if (now - lastWheel < WHEEL_SPACING) return;
      lastWheel = now;
    }
    runAction(module, trigger, action);
  };
  const onClick = (e: MouseEvent) => take(e, e.button === 0 ? "left" : undefined);
  const onAux = (e: MouseEvent) => take(e, e.button === 1 ? "middle" : undefined);
  const onDown = (e: MouseEvent) => {
    // No autoscroll cursor on a bound middle click.
    if (e.button === 1 && boundAction(options, "middle")) e.preventDefault();
  };
  const onMenu = (e: MouseEvent) => take(e, "right");
  const onWheel = (e: WheelEvent) => take(e, wheelTrigger(e.deltaY));
  const capture = { capture: true } as const;
  el.addEventListener("click", onClick, capture);
  el.addEventListener("auxclick", onAux, capture);
  el.addEventListener("mousedown", onDown, capture);
  el.addEventListener("contextmenu", onMenu, capture);
  el.addEventListener("wheel", onWheel, { capture: true, passive: false });
  return () => {
    el.removeEventListener("click", onClick, capture);
    el.removeEventListener("auxclick", onAux, capture);
    el.removeEventListener("mousedown", onDown, capture);
    el.removeEventListener("contextmenu", onMenu, capture);
    el.removeEventListener("wheel", onWheel, capture);
  };
}
