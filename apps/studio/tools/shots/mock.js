// Runs in the page before the app: stands in for Tauri's IPC with fixture data (fixtures.mjs).
(() => {
  const F = window.__FIXTURES__;
  const state = {
    config: structuredClone(F.config),
    themes: structuredClone(F.themes),
    layout: structuredClone(F.layout),
    bars: { doc: { version: 1, preset: "macos", dock: { enabled: true } }, exists: true, path: "C:\\Users\\me\\AppData\\Roaming\\mypaper\\bars.json", problem: null },
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
  // The weather plugin's answers, in its real shapes (plugins/weather/src/weather.rs).
  const place = { name: "Reykjavík", region: "Capital Region", country: "Iceland", lat: 64.1466, lon: -21.9426, source: "windows" };
  const units = { temperature: "°C", speed: "km/h", precipitation: "mm" };
  const day = (offset, code, icon, condition, min, max) => ({ date: new Date(Date.now() + offset * 864e5).toISOString().slice(0, 10), code, icon, condition, min, max, precipitation_probability: code > 3 ? 60 : 10 });
  const weather = {
    locate: () => place,
    current: () => ({ location: place, units, time: new Date().toISOString(), temperature: -2.4, apparent: -7.1, humidity: 81, wind: 18, wind_direction: 240, precipitation: 0, code: 2, condition: "Partly cloudy", icon: "partly-cloudy-night", is_day: false }),
    forecast: () => ({
      location: place,
      units,
      days: [day(0, 2, "partly-cloudy-day", "Partly cloudy", -5, -1), day(1, 71, "snow", "Light snow", -6, -2), day(2, 3, "cloudy", "Overcast", -4, 0), day(3, 0, "clear-day", "Clear sky", -8, -3), day(4, 61, "rain", "Light rain", -1, 3), day(5, 2, "partly-cloudy-day", "Partly cloudy", -3, 1), day(6, 3, "cloudy", "Overcast", -2, 2)],
      hourly: Array.from({ length: 24 }, (_, i) => ({ time: new Date(Date.now() + i * 36e5).toISOString(), temperature: -2 - Math.sin(i / 4) * 2, code: 2, condition: "Partly cloudy", icon: i > 8 && i < 16 ? "partly-cloudy-day" : "partly-cloudy-night", is_day: i > 8 && i < 16 })),
    }),
    search: ({ query }) =>
      [
        { name: "Paris", admin1: "Île-de-France", admin2: "Paris", country: "France", country_code: "FR", lat: 48.8534, lon: 2.3488, population: 2138551 },
        { name: "Paris", admin1: "Texas", admin2: "Lamar", country: "United States", country_code: "US", lat: 33.6609, lon: -95.5555, population: 24782 },
        { name: "Parisot", admin1: "Occitanie", admin2: "Tarn", country: "France", country_code: "FR", lat: 43.7976, lon: 1.8628, population: 1243 },
      ].filter((c) => c.name.toLowerCase().startsWith(String(query).toLowerCase().slice(0, 3))),
  };
  const handlers = {
    plugin_call: ({ plugin, method, params }) => (plugin === "weather" && weather[method] ? weather[method](params ?? {}) : Promise.reject(`the ${plugin} plugin isn't installed`)),
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
