// Fixture data for the mocked Tauri IPC, built from the repo's real themes and widgets.
import fs from "node:fs";
import path from "node:path";

const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1"));

export function buildFixtures(repo, { empty = false } = {}) {
  const themesDir = path.join(repo, "themes");
  const themes = [];
  for (const id of fs.readdirSync(themesDir)) {
    const dir = path.join(themesDir, id);
    if (!fs.statSync(dir).isDirectory()) continue;
    const manifest = path.join(dir, "theme.json");
    const preview = fs.existsSync(path.join(dir, "preview.jpg")) ? path.join(dir, "preview.jpg") : null;
    let doc = null;
    let error = null;
    try {
      doc = read(manifest);
    } catch {
      error = `in ${manifest}: the file is missing`;
    }
    themes.push({
      id: `bundled:${id}`,
      name: doc?.name ?? id,
      dir,
      origin: "bundled",
      kind: doc?.wallpaper?.kind ?? null,
      error,
      preview,
      previewPending: false,
      styled: !!doc?.windows,
      widgetsSkin: !!doc?.widgets,
      barsSkin: !!doc?.bars,
      active: id === "photo",
      applyAs: id,
    });
  }
  // Two of the user's own: one with a preview, one without.
  themes.push({
    id: "user:northern",
    name: "Northern lights",
    dir: "C:\\Users\\me\\AppData\\Roaming\\mypaper\\themes\\northern",
    origin: "user",
    kind: "image",
    error: null,
    preview: path.join(themesDir, "aurora", "preview.jpg"),
    previewPending: false,
    styled: true,
    widgetsSkin: true,
    barsSkin: false,
    active: false,
    applyAs: "northern",
  });
  themes.push({
    id: "user:desk",
    name: "Desk scene",
    dir: "C:\\Users\\me\\AppData\\Roaming\\mypaper\\themes\\desk",
    origin: "user",
    kind: "scene",
    error: null,
    preview: null,
    previewPending: false,
    styled: false,
    widgetsSkin: false,
    barsSkin: false,
    active: false,
    applyAs: "desk",
  });

  const widgetsDir = path.join(repo, "widgets");
  const widgets = [];
  for (const id of fs.readdirSync(widgetsDir)) {
    const file = path.join(widgetsDir, id, "widget.json");
    if (!fs.existsSync(file)) continue;
    const w = read(file);
    widgets.push({
      id: w.id ?? id,
      name: w.name ?? id,
      version: w.version ?? null,
      author: w.author ?? null,
      description: w.description ?? null,
      size: { default: w.size?.default ?? [2, 2], min: w.size?.min ?? [1, 1], max: w.size?.max ?? null },
      resizable: w.resizable ?? true,
      permissions: w.permissions ?? [],
      preset: w.preset ?? "desktop",
      origin: "bundled",
      dir: path.join(widgetsDir, id),
      entry: w.entry ?? "index.html",
      manifest: w,
      error: null,
    });
  }

  const D1 = "\\\\.\\DISPLAY1";
  const D2 = "\\\\.\\DISPLAY2";
  const menuDefault = ["view", "sort", "refresh", "separator", "new", "paste", "separator", "wallpaper", "widgets", "separator", "display", "personalize", "terminal", "more"];
  return {
    config: {
      theme: "photo",
      paused: false,
      pause_on_battery: true,
      pause_on_fullscreen: true,
      static_snapshot: true,
      classic_context_menu: false,
      desktop_menu: { enabled: true, items: [...menuDefault, { label: "Notes", command: "notepad.exe", args: [] }] },
      desktop_menu_default: menuDefault,
    },
    themes: empty ? [] : themes,
    roots: ["C:\\Users\\me\\AppData\\Roaming\\mypaper\\themes", path.join(repo, "themes")],
    widgets: empty ? [] : widgets,
    layout: {
      appearance: empty ? {} : { surface: "glass", radius: 20 },
      overlay: true,
      exists: true,
      grid: { cell: 0, gap: 8, margin: 16 },
      iconCell: [75, 75],
      instances: empty
        ? []
        : [
            { instance: "clock-1", widget: "clock", monitor: D1, cell: [0, 0], size: [4, 2], appearance: {}, settings: {} },
            { instance: "weather-1", widget: "weather", monitor: D1, cell: [0, 3], size: [4, 2], appearance: {}, settings: {} },
            { instance: "calendar-1", widget: "calendar", monitor: D1, cell: [17, 0], size: [3, 3], appearance: {}, settings: {} },
            { instance: "system-monitor-1", widget: "system-monitor", monitor: D1, cell: [17, 4], size: [3, 2], appearance: { style: "depth" }, settings: {} },
            { instance: "now-playing-1", widget: "now-playing", monitor: D1, cell: [0, 6], size: [4, 2], appearance: {}, settings: {} },
            { instance: "gone-1", widget: "old-widget", monitor: D2, cell: [2, 2], size: [2, 2], appearance: {}, settings: {} },
          ],
    },
    catalog: read(path.join(here, "catalog.json")),
    /** What the native file picker "returns". */
    pickedFile: path.join(themesDir, "photo", "iss-aurora.jpg"),
  };
}
