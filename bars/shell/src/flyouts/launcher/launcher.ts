// The launcher (docs/bars.md, "Flyouts" → "Launcher"): our Start menu. Keyboard first: type to
// search apps, Windows Settings pages and recent files; arrows move, Enter opens, Escape closes.
// Four layouts (`launcher.layout`):
//   win11      search, pinned apps, recommended (recent files), all apps A–Z
//   grid       search over a big grid of every app (Launchpad)
//   compact    a search bar; recent apps until you type (Raycast, Flow)
//   dashboard  search, pinned, a row of widgets (time, media, system) and the clipboard history
// The index arrives once and stays (the host rebuilds it when the Start Menu changes); recent files
// and the clipboard are read each time the launcher opens.

import { button, glyph, glyphLetter, h, rectOf } from "../../dom";
import { defineFlyout, type FlyoutContext } from "../../flyout";
import { fire, request } from "../../host";
import { glyphs } from "../glyphs";
import type { AppsResult, ClipboardHistory, LauncherLayout, Pin, RecentFile } from "../protocol";
import { barsConfig, fit, show } from "../surface";
import { appIcon } from "../ui";
import { alphabetical, appEntry, fileEntry, gridStep, recentApps, resolvePins, search, step, type Entry } from "./model";
import { SETTINGS_PAGES } from "./settings-pages";

/** The index and history, kept between opens. */
let cache: AppsResult | undefined;

const SIZES: Record<LauncherLayout, { w: number; h: number }> = {
  win11: { w: 660, h: 720 },
  grid: { w: 1080, h: 780 },
  compact: { w: 660, h: 460 },
  dashboard: { w: 860, h: 720 },
};

/** The most results each group shows. */
const PER_GROUP = { app: 8, setting: 5, file: 5 } as const;

/** Pinned columns per layout (arrow keys move by rows of this many). */
const COLUMNS: Record<LauncherLayout, number> = { win11: 6, grid: 8, compact: 1, dashboard: 5 };

interface Props {
  layout: LauncherLayout;
  size: { w: number; h: number };
}

export async function open(anchor: HTMLElement): Promise<void> {
  const layout = barsConfig().launcher.layout;
  const want = SIZES[layout];
  const size = fit(want.w, want.h);
  const fromStart = anchor.dataset.module === "start";
  await show(launcher, { layout, size }, { anchor, align: fromStart ? "start" : "center", extent: size.h + 24, keyboard: true });
}

/** A selectable thing on screen: what Enter opens and the arrows walk through. */
interface Item {
  el: HTMLElement;
  entry: Entry;
}

const launcher = defineFlyout<Props>("launcher", ({ layout, size }, ctx) => {
  const config = barsConfig();
  const pins = (): Pin[] => pinned ?? config.launcher.pinned ?? config.dock.pinned;
  let pinned: Pin[] | undefined;
  let files: RecentFile[] = [];
  let clipboard: ClipboardHistory | undefined;
  // Built once per open: it holds subscriptions and a timer.
  let widgetRow: HTMLElement | undefined;
  let view: "home" | "all" = "home";
  let items: Item[] = [];
  let columns = 1;
  let selected = -1;

  const root = h("div", "launcher");
  root.dataset.layout = layout;
  root.style.width = `${size.w}px`;
  root.style.height = layout === "compact" ? "" : `${size.h}px`;
  root.style.maxHeight = `${size.h}px`;

  const input = h("input", "launcher-input");
  input.type = "text";
  input.spellcheck = false;
  input.autocomplete = "off";
  input.placeholder = layout === "compact" ? "Search apps, settings and files" : "Type to search";
  input.setAttribute("aria-label", "Search");
  const searchBar = h("label", "launcher-search", glyph(glyphs.search, "launcher-search-glyph"), input);
  const body = h("div", "launcher-body");
  const menu = h("div", "launcher-menu");
  menu.hidden = true;
  root.append(searchBar, body, menu);
  const userName = h("span", "launcher-user-name", cache?.user.name ?? "");
  if (layout !== "compact") root.append(footer(ctx, userName));

  // --- Running things ------------------------------------------------------------------------

  const run = (entry: Entry) => {
    request("run", { id: entry.id, path: entry.path })
      .then(({ recent }) => {
        if (cache) cache = { ...cache, recent };
      })
      .catch((error: unknown) => console.warn("[bars] run failed", error));
    ctx.close();
  };

  const select = (index: number) => {
    items[selected]?.el.classList.remove("is-selected");
    selected = index;
    const item = items[selected];
    if (!item) return;
    item.el.classList.add("is-selected");
    item.el.scrollIntoView({ block: "nearest" });
  };

  /** A tile (icon over name) or a row (icon, name, detail) that opens `entry`. */
  const entryEl = (entry: Entry, kind: "tile" | "row"): HTMLElement => {
    const el = button(kind === "tile" ? "launcher-tile" : "launcher-row");
    el.dataset.kind = entry.kind;
    el.title = entry.name;
    const icon =
      entry.kind === "setting"
        ? h("span", "fly-icon launcher-glyph", glyph(glyphs.gear))
        : entry.icon
          ? appIcon(entry.icon, entry.name)
          : entry.kind === "file"
            ? h("span", "fly-icon launcher-glyph", glyph(glyphs.file))
            : h("span", "fly-icon fly-monogram", glyphLetter(entry.name));
    if (kind === "tile") el.append(icon, h("span", "launcher-tile-name", entry.name));
    else
      el.append(
        icon,
        h("span", "launcher-row-text", h("span", "launcher-row-name", entry.name), ...(entry.detail ? [h("span", "launcher-row-detail", entry.detail)] : [])),
      );
    const index = items.length;
    items.push({ el, entry });
    el.addEventListener("click", () => run(entry));
    el.addEventListener("pointerenter", () => select(index));
    el.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      if (entry.kind === "app") openMenu(entry, el);
    });
    return el;
  };

  // --- The context menu (inline: one flyout is open at a time) ---------------------------------

  const openMenu = (entry: Entry, at: HTMLElement) => {
    const isPinned = pins().some((p) => p.path.toLowerCase() === entry.path.toLowerCase());
    const action = (label: string, fn: () => void) => {
      const el = button("menu-item", h("span", "menu-label", label));
      el.addEventListener("click", () => {
        menu.hidden = true;
        fn();
      });
      return el;
    };
    menu.replaceChildren(
      action("Open", () => run(entry)),
      action(isPinned ? "Unpin from launcher" : "Pin to launcher", () => {
        const next = isPinned
          ? pins().filter((p) => p.path.toLowerCase() !== entry.path.toLowerCase())
          : [...pins(), { path: entry.path, name: entry.name }];
        pinned = next;
        if (isPinned) fire("launcherUnpin", { path: entry.path });
        else fire("launcherPin", { path: entry.path, name: entry.name });
        render();
      }),
      action("Pin to dock", () => fire("pin", { path: entry.path, name: entry.name })),
    );
    menu.hidden = false;
    const [x, y, , hgt] = rectOf(at);
    const [rx, ry] = rectOf(root);
    menu.style.left = `${Math.min(x - rx, size.w - 200)}px`;
    menu.style.top = `${y - ry + hgt}px`;
  };
  root.addEventListener("pointerdown", (event) => {
    if (!menu.hidden && event.target instanceof Node && !menu.contains(event.target)) menu.hidden = true;
  });

  // --- Views ----------------------------------------------------------------------------------------

  const apps = () => cache?.apps ?? [];
  const section = (title: string, content: HTMLElement, action?: HTMLElement) =>
    h("section", "launcher-section", h("div", "launcher-section-head", h("span", "launcher-section-title", title), ...(action ? [action] : [])), content);
  const grid = (entries: Entry[], cols: number) => {
    const el = h("div", "launcher-grid", ...entries.map((e) => entryEl(e, "tile")));
    el.style.setProperty("--columns", String(cols));
    return el;
  };
  const list = (entries: Entry[]) => h("div", "launcher-list", ...entries.map((e) => entryEl(e, "row")));
  const textButton = (label: string, onClick: () => void, icon?: string) => {
    const el = button("launcher-text-button", label, ...(icon ? [glyph(icon)] : []));
    el.addEventListener("click", onClick);
    return el;
  };

  const loading = () => h("div", "fly-notice", cache ? "No apps found" : "Loading apps…");

  const home = (): HTMLElement[] => {
    const pinnedEntries = resolvePins(pins(), apps());
    const recentFiles = files.slice(0, 6).map(fileEntry);
    const recent = recentApps(cache?.recent ?? [], apps(), 6);
    switch (layout) {
      case "win11": {
        columns = COLUMNS.win11;
        const recommended = recentFiles.length > 0 ? recentFiles : recent;
        return [
          section("Pinned", pinnedEntries.length ? grid(pinnedEntries, columns) : h("div", "fly-notice", "Right-click an app to pin it"), textButton("All apps", () => setView("all"), glyphs.chevronRight)),
          section("Recommended", recommended.length ? h("div", "launcher-two", ...recommended.map((e) => entryEl(e, "row"))) : h("div", "fly-notice", "Recent files show here")),
        ];
      }
      case "grid": {
        columns = COLUMNS.grid;
        const all = apps().map(appEntry);
        return [all.length ? grid(all, columns) : loading()];
      }
      case "compact": {
        columns = 1;
        return recent.length ? [section("Recent", list(recent))] : [];
      }
      case "dashboard": {
        columns = COLUMNS.dashboard;
        return [
          h(
            "div",
            "launcher-dash",
            h(
              "div",
              "launcher-dash-main",
              section("Pinned", pinnedEntries.length ? grid(pinnedEntries, columns) : h("div", "fly-notice", "Right-click an app to pin it"), textButton("All apps", () => setView("all"), glyphs.chevronRight)),
              (widgetRow ??= widgets(ctx)),
            ),
            section("Clipboard", clipboardList()),
          ),
        ];
      }
    }
  };

  const allApps = (): HTMLElement[] => {
    columns = 1;
    const groups = alphabetical(apps()).map((group) =>
      h("div", "launcher-letter-group", h("div", "launcher-letter", group.letter), list(group.apps.map(appEntry))),
    );
    return [section("All apps", h("div", "launcher-az", ...(groups.length ? groups : [loading()])), textButton("Back", () => setView("home"), glyphs.chevronLeft))];
  };

  const results = (query: string): HTMLElement[] => {
    columns = 1;
    const found = search(query, { apps: apps(), settings: SETTINGS_PAGES, files: config.launcher.files ? files : [] }, cache?.recent ?? [], Date.now());
    if (found.length === 0) return [h("div", "fly-notice", `No results for “${query}”`)];
    const best = found[0];
    const rest = found.slice(1);
    return [
      ...(best ? [section("Best match", list([best]))] : []),
      ...(["app", "setting", "file"] as const).flatMap((kind) => {
        const of = rest.filter((e) => e.kind === kind).slice(0, PER_GROUP[kind]);
        const title = { app: "Apps", setting: "Settings", file: "Files" }[kind];
        return of.length ? [section(title, list(of))] : [];
      }),
    ];
  };

  const clipboardList = (): HTMLElement => {
    if (!clipboard) return h("div", "fly-notice", "Loading…");
    if (!clipboard.enabled) {
      const turnOn = button("fly-link", "Turn on clipboard history");
      turnOn.addEventListener("click", () => fire("openSettings", { uri: "ms-settings:clipboard" }));
      return h("div", "fly-notice", "Clipboard history is off", turnOn);
    }
    if (clipboard.entries.length === 0) return h("div", "fly-notice", "Copy something to see it here");
    return h(
      "div",
      "clip-list",
      ...clipboard.entries.map((entry, index) => {
        const el = button(`clip-entry clip-${entry.kind}`);
        if (entry.image) {
          const img = h("img", "clip-image");
          img.src = entry.image;
          img.alt = "";
          el.append(img);
        } else {
          el.append(glyph(entry.kind === "link" ? glyphs.link : entry.kind === "image" ? glyphs.image : glyphs.clipboard, "clip-glyph"), h("span", "clip-text", entry.text));
        }
        el.title = "Copy to the clipboard";
        el.addEventListener("click", () => {
          request("clipboardPaste", { index })
            .then(() => ctx.close())
            .catch((error: unknown) => console.warn("[bars] clipboardPaste failed", error));
        });
        return el;
      }),
    );
  };

  const setView = (next: "home" | "all") => {
    view = next;
    render();
  };

  const render = () => {
    items = [];
    selected = -1;
    const query = input.value.trim();
    const content = query ? results(query) : view === "all" ? allApps() : home();
    body.replaceChildren(...content);
    root.dataset.view = query ? "results" : view;
    if (query && items.length > 0) select(0);
    if (layout === "compact") ctx.reposition();
  };

  // --- Keyboard -------------------------------------------------------------------------------

  input.addEventListener("input", render);
  root.addEventListener("keydown", (event) => {
    const key = event.key;
    const inGrid = columns > 1 && !input.value.trim() && view === "home";
    if (key === "ArrowDown" || key === "ArrowUp" || key === "ArrowLeft" || key === "ArrowRight") {
      if (!inGrid && (key === "ArrowLeft" || key === "ArrowRight")) return; // the caret moves
      event.preventDefault();
      const direction = { ArrowDown: "down", ArrowUp: "up", ArrowLeft: "left", ArrowRight: "right" } as const;
      select(inGrid ? gridStep(selected, items.length, columns, direction[key]) : step(selected, items.length, key === "ArrowDown" ? 1 : -1));
      return;
    }
    if (key === "Enter") {
      event.preventDefault();
      const item = items[selected] ?? items[0];
      if (item) run(item.entry);
      return;
    }
    // Typing anywhere goes to the search box.
    if (key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey && document.activeElement !== input) {
      input.focus();
    }
  });

  // --- Data -------------------------------------------------------------------------------------

  request("apps")
    .then((result) => {
      cache = result;
      userName.textContent = result.user.name;
      render();
    })
    .catch((error: unknown) => console.warn("[bars] apps failed", error));
  ctx.scope.on("launcherApps", (result) => {
    cache = result;
    render();
  });
  ctx.scope.on("launcherPins", (next) => {
    pinned = next;
    render();
  });
  fire("watch", { topic: "launcherApps" });
  ctx.scope.add(() => fire("unwatch", { topic: "launcherApps" }));
  if (layout === "win11" || config.launcher.files) {
    request("recentFiles")
      .then((list) => {
        files = list;
        render();
      })
      .catch((error: unknown) => console.warn("[bars] recentFiles failed", error));
  }
  if (layout === "dashboard") {
    request("clipboard")
      .then((history) => {
        clipboard = history;
        render();
      })
      .catch((error: unknown) => console.warn("[bars] clipboard failed", error));
  }

  render();
  requestAnimationFrame(() => input.focus({ preventScroll: true }));
  return root;
});

// --- The footer: who's signed in, settings, power ------------------------------------------------

function footer(ctx: FlyoutContext, name: HTMLElement): HTMLElement {
  const user = button("launcher-user", h("span", "launcher-avatar", glyph(glyphs.user)), name);
  user.title = "Your account";
  user.addEventListener("click", () => {
    fire("openSettings", { uri: "ms-settings:yourinfo" });
    ctx.close();
  });
  const settings = button("fly-icon-button", glyph(glyphs.gear));
  settings.title = "Settings";
  settings.addEventListener("click", () => {
    fire("openSettings", { uri: "ms-settings:" });
    ctx.close();
  });
  const powerMenu = h("div", "launcher-power-menu");
  powerMenu.hidden = true;
  const power = button("fly-icon-button", glyph(glyphs.power));
  power.title = "Power";
  type Action = "lock" | "sleep" | "signout" | "restart" | "shutdown";
  const fill = (confirm?: "restart" | "shutdown") => {
    const item = (label: string, run: () => void, primary = false) => {
      const el = button(primary ? "menu-item primary" : "menu-item", h("span", "menu-label", label));
      el.addEventListener("click", run);
      return el;
    };
    const act = (action: Action) => () => {
      fire("power", { action });
      ctx.close();
    };
    powerMenu.replaceChildren(
      ...(confirm
        ? [
            h("div", "menu-header", confirm === "restart" ? "Restart your PC?" : "Shut down your PC?"),
            item("Cancel", () => (powerMenu.hidden = true)),
            item(confirm === "restart" ? "Restart now" : "Shut down now", act(confirm), true),
          ]
        : [
            item("Lock", act("lock")),
            item("Sleep", act("sleep")),
            item("Sign out", act("signout")),
            h("div", "menu-sep"),
            item("Restart…", () => fill("restart")),
            item("Shut down…", () => fill("shutdown")),
          ]),
    );
  };
  power.addEventListener("click", () => {
    fill();
    powerMenu.hidden = !powerMenu.hidden;
  });
  return h("div", "launcher-footer", user, h("span", "launcher-footer-gap"), settings, h("div", "launcher-power", power, powerMenu));
}

// --- The dashboard's widgets: time, what's playing, the machine ------------------------------------

function widgets(ctx: FlyoutContext): HTMLElement {
  const time = h("div", "widget-time");
  const date = h("div", "widget-date");
  const clock = h("div", "widget widget-clock", time, date);
  const tick = () => {
    const now = new Date();
    time.textContent = now.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    date.textContent = now.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  };
  tick();
  const timer = window.setInterval(tick, 15_000);
  ctx.scope.add(() => clearInterval(timer));

  const title = h("div", "widget-media-title", "Nothing playing");
  const artist = h("div", "widget-media-artist");
  const toggle = button("fly-icon-button", glyph(glyphs.play));
  const controls = h("div", "widget-media-controls");
  const control = (markup: string, action: "toggle" | "next" | "previous") => {
    const el = action === "toggle" ? toggle : button("fly-icon-button", glyph(markup));
    el.addEventListener("click", () => fire("media", { action }));
    return el;
  };
  controls.append(control(glyphs.previous, "previous"), control(glyphs.play, "toggle"), control(glyphs.next, "next"));
  const media = h("div", "widget widget-media", title, artist, controls);
  ctx.scope.on("media", (info) => {
    media.dataset.playing = String(info?.playing ?? false);
    title.textContent = info?.title ?? "Nothing playing";
    artist.textContent = info?.artist ?? "";
    toggle.innerHTML = "";
    toggle.append(glyph(info?.playing ? glyphs.pause : glyphs.play));
    controls.hidden = !info;
  });

  const meter = (label: string) => {
    const fill = h("span", "widget-meter-fill");
    const value = h("span", "widget-meter-value");
    const el = h("div", "widget-meter", h("span", "widget-meter-label", label), h("span", "widget-meter-track", fill), value);
    const set = (percent: number, text: string) => {
      fill.style.width = `${Math.round(percent)}%`;
      value.textContent = text;
    };
    return { el, set };
  };
  const cpu = meter("CPU");
  const ram = meter("Memory");
  const battery = meter("Battery");
  battery.el.hidden = true;
  const system = h("div", "widget widget-system", cpu.el, ram.el, battery.el);
  ctx.scope.on("stats", ({ cpu: c, memory }) => {
    cpu.set(c, `${Math.round(c)}%`);
    const used = memory.total ? (memory.used / memory.total) * 100 : 0;
    ram.set(used, `${Math.round(used)}%`);
  });
  ctx.scope.on("battery", (info) => {
    battery.el.hidden = !info;
    if (info) battery.set(info.percent, `${Math.round(info.percent)}%${info.charging ? " ⚡" : ""}`);
  });
  for (const topic of ["media", "stats", "battery"] as const) {
    fire("watch", { topic });
    ctx.scope.add(() => fire("unwatch", { topic }));
  }
  return h("div", "launcher-widgets", clock, media, system);
}
