// Runs in the page before the app: stands in for Tauri's IPC with fixture data (fixtures.mjs).
(() => {
  const F = window.__FIXTURES__;
  const state = {
    config: structuredClone(F.config),
    themes: structuredClone(F.themes),
    layout: structuredClone(F.layout),
    bars: { doc: { version: 1, preset: "macos" }, exists: true, path: "C:\\Users\\me\\AppData\\Roaming\\mypaper\\bars.json", problem: null },
  };
  const delay = (value, ms = 30) => new Promise((resolve) => setTimeout(() => resolve(value), ms));
  const putInstance = ({ patch }) => {
    const { instance, ...fields } = patch;
    const items = state.layout.instances;
    const index = items.findIndex((i) => i.instance === instance);
    const base = items[index] ?? { instance, widget: fields.widget, monitor: null, cell: null, size: null, appearance: {}, settings: {} };
    const next = { ...base, ...fields };
    state.layout = { ...state.layout, instances: index < 0 ? [...items, next] : items.with(index, next) };
    return state.layout;
  };
  const handlers = {
    get_config: () => state.config,
    update_config: ({ patch }) => {
      const { desktop_menu, ...flat } = patch;
      state.config = { ...state.config, ...flat, desktop_menu: { ...state.config.desktop_menu, ...desktop_menu } };
      if (flat.theme) state.themes = state.themes.map((t) => ({ ...t, active: t.applyAs === flat.theme }));
      return state.config;
    },
    reset_config: () => state.config,
    list_themes: () => ({ themes: state.themes, roots: F.roots }),
    generate_preview: () => null,
    duplicate_theme: ({ dir }) => state.themes.find((t) => t.dir === dir),
    get_style: ({ dir }) =>
      dir.includes("northern")
        ? { accent: "auto", border: null, caption: "#1b1b1f", caption_text: null, corners: "round", backdrop: "mica", dark_mode: true, cursors: "mypaper-dark" }
        : { accent: "auto", border: "auto", caption: null, caption_text: null, corners: "round", backdrop: "mica", dark_mode: true, cursors: null },
    set_style: ({ style }) => style,
    get_skin: () => ({ appearance: { surface: "glass", radius: 22 }, css: null }),
    set_skin: ({ skin }) => skin,
    import_skin_css: () => "widgets.css",
    active_skins: () => ({ widgets: {}, bars: {} }),
    get_interactive: () => false,
    set_interactive: ({ on }) => on,
    list_cursor_sets: () => [],
    get_bars: () => state.bars,
    bars_catalog: () => F.catalog,
    set_bars: ({ doc }) => (state.bars = { ...state.bars, doc }),
    reset_bars: () => state.bars,
    theme_vars: () => ({ theme: "photo", css: null, vars: [], error: null }),
    list_icon_packs: () => [],
    icon_pack_apps: () => [],
    list_monitors: () => [
      { device: "\\\\.\\DISPLAY1", primary: true, width: 2560, height: 1440, scale: 1.5, x: 0, y: 0, work: { x: 0, y: 0, width: 2560, height: 1368 } },
      { device: "\\\\.\\DISPLAY2", primary: false, width: 1920, height: 1080, scale: 1, x: 2560, y: 0, work: { x: 0, y: 0, width: 1920, height: 1032 } },
    ],
    dock_icon: () => Promise.reject("no icon"),
    list_widgets: () => F.widgets,
    get_layout: () => state.layout,
    set_widgets_appearance: ({ appearance }) => (state.layout = { ...state.layout, appearance }),
    set_widgets_overlay: ({ on }) => (state.layout = { ...state.layout, overlay: on }),
    remove_instance: ({ instance }) => (state.layout = { ...state.layout, instances: state.layout.instances.filter((i) => i.instance !== instance) }),
    reset_instance_look: () => state.layout,
    put_instance: putInstance,
    toggle_widget_edit: () => null,
    components: () => [
      { id: "core", exe: "mypaper-core.exe", running: true, path: "C:\\mypaper\\mypaper-core.exe" },
      { id: "wallpaper", exe: "mypaper-wallpaper.exe", running: true, path: "C:\\mypaper\\mypaper-wallpaper.exe" },
      { id: "widgets", exe: "mypaper-widgets.exe", running: true, path: "C:\\mypaper\\mypaper-widgets.exe" },
      { id: "bars", exe: "mypaper-bars.exe", running: false, path: null },
    ],
    engine_status: () => ({ running: true, wallpaperRunning: true, corePath: "C:\\mypaper\\mypaper-core.exe" }),
    start_engine: () => ({ running: true, wallpaperRunning: true, corePath: "C:\\mypaper\\mypaper-core.exe" }),
    stop_engine: () => ({ running: false, wallpaperRunning: false, corePath: "C:\\mypaper\\mypaper-core.exe" }),
    get_autostart: () => ({ enabled: true, method: "task", command: '"C:\\mypaper\\mypaper-core.exe" --autostart' }),
    set_autostart: ({ enabled }) => ({ enabled, method: enabled ? "task" : null, command: null }),
    inspect_source: ({ path }) => ({ source: path, kind: "image", usesFit: true, suggestedName: "Aurora from the ISS", presets: [], defaultArgs: "" }),
    inspect_url: ({ url }) => ({ source: url, kind: "web", usesFit: false, suggestedName: "Example", presets: [], defaultArgs: "" }),
    "plugin:event|listen": () => 1,
    "plugin:event|unlisten": () => null,
    "plugin:dialog|open": () => F.pickedFile,
  };
  let nextId = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback(callback, once) {
      const id = nextId++;
      window[`_${id}`] = (payload) => {
        if (once) delete window[`_${id}`];
        return callback?.(payload);
      };
      return id;
    },
    unregisterCallback(id) {
      delete window[`_${id}`];
    },
    // Like Tauri's: the whole path as one encoded segment. The server decodes each segment, so
    // folder URLs (segments joined with "/") resolve relative links too.
    convertFileSrc(filePath) {
      return `/asset/${encodeURIComponent(filePath)}`;
    },
    async invoke(cmd, args = {}) {
      const handler = handlers[cmd];
      if (!handler) return Promise.reject(`mock: no handler for ${cmd}`);
      return delay(structuredClone(await handler(args)));
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
})();
