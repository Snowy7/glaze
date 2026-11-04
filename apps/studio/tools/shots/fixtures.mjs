// Fixture data for the mocked Tauri IPC, built from the repo's real themes and widgets.
import fs from "node:fs";
import path from "node:path";

const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1"));

/** `active`: the theme id `config.theme` names (a hidden example stays listed while it's active). */
export function buildFixtures(repo, { empty = false, active = "rain-city" } = {}) {
  const themesDir = path.join(repo, "themes");
  const catalog = read(path.join(themesDir, "catalog.json"));
  const themes = [];
  // Like Studio's lister: the curated themes and the examples, hidden ones only while active.
  const dirs = [themesDir, path.join(themesDir, "examples")].flatMap((root) =>
    fs.readdirSync(root).map((id) => path.join(root, id)).filter((dir) => fs.existsSync(path.join(dir, "theme.json"))),
  );
  for (const dir of dirs) {
    const id = path.basename(dir);
    const doc = read(path.join(dir, "theme.json"));
    const record = catalog.themes.find((t) => t.id === id);
    if ((doc.hidden || record?.hidden) && id !== active) continue;
    const preview = ["preview.jpg", "preview.png"].map((file) => path.join(dir, file)).find((file) => fs.existsSync(file)) ?? null;
    themes.push({
      id: `bundled:${id}`,
      name: doc.name ?? id,
      dir,
      origin: "bundled",
      kind: doc.wallpaper?.kind ?? null,
      error: null,
      preview,
      previewPending: false,
      styled: !!doc.windows,
      widgetsSkin: !!doc.widgets,
      barsSkin: !!doc.bars,
      active: id === active,
      applyAs: id,
      hidden: !!doc.hidden,
      credit: record
        ? { category: record.category ?? null, author: record.author ?? null, license: record.license ?? null, licenseUrl: record.license_url ?? null, source: record.source ?? null, noncommercial: !!record.noncommercial }
        : null,
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
    preview: path.join(themesDir, "examples", "aurora", "preview.jpg"),
    previewPending: false,
    styled: true,
    widgetsSkin: true,
    barsSkin: false,
    active: false,
    applyAs: "northern",
    hidden: false,
    credit: null,
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
    hidden: false,
    credit: null,
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
      theme: active,
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
    pickedFile: path.join(themesDir, "examples", "photo", "iss-aurora.jpg"),
  };
}
