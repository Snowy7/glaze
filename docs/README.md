# Glaze docs

Back to the [project README](../README.md).

## Product

| Doc | About |
|---|---|
| [ui.md](ui.md) | The UI kit (`crates/ui`) and `glaze-shell`: surfaces, display lists, text, icons, input, accessibility, measurements |
| [glass.md](glass.md) | Liquid Glass: materials, live glass over web/app wallpapers, GPU cost and the GPU default |
| [bars.md](bars.md) | The top bar, dock and taskbar: presets, styles, modules, tray host, flyouts, and their live measurements |
| [widgets.md](widgets.md) | Native desktop widgets: windows and glass, edit mode, sizes, scheduling, storage, Studio settings |
| [integrations.md](integrations.md) | Integration providers (T3 Code, Discord, Docker, OBS, Tailscale, Grok Bot, calendars, notifications) and `integrations.json` |
| [desktop-extras.md](desktop-extras.md) | The desktop right-click menu, cursor themes, the classic Windows 11 menu, interactive wallpapers |
| [windows-styling.md](windows-styling.md) | One-switch native style sync, reversible Windows frames/accent/mode, and the experimental App styler's presets and current limits |
| [system.md](system.md) | Reversible Windows preferences and games: background quiescence, pause during games, and `engine.during_games` |
| [index.md](index.md) | Experimental native name/path search and folder sizes: `experimental.file_index`, `index.exclude` / `index.include`, measurements and limits |
| [files.md](files.md) | Glaze Files: native tabs, previews, search and file operations; optional folder/Win+E routing with `experimental.files_default` |
| [web-and-app-wallpapers.md](web-and-app-wallpapers.md) | `web` (WebView2) and `app` (external renderer) wallpapers |
| [studio-ux.md](studio-ux.md) | Studio: places, navigation, saving and undo, previews, keyboard, cost, code map |

## Building on it

| Doc | About |
|---|---|
| [plugins.md](plugins.md) | WebAssembly plugins: SDK, native widget faces, host capabilities, lifecycle and game pauses |
| [writing-widgets.md](writing-widgets.md) | Legacy JSON-RPC service plugins and adding a compiled widget view; new plugins use the WebAssembly guide |
| [writing-plugins.md](writing-plugins.md) | Pointer to the plugin guide above; the HTML widget SDK was removed |

## Engine and performance

| Doc | About |
|---|---|
| [performance.md](performance.md) | Harness and budgets; 5–6 November engine/shell hill-climbs, covered-resource release, game quiescence and before/after measurements |
| [engine-notes.md](engine-notes.md) | Engine findings: memory, EXIF rotation, 10-bit/HDR video, renderer settings, glass overlays, hot-patching |

## Research

| Doc | About |
|---|---|
| [research/customization-landscape.md](research/customization-landscape.md) | How Seelen UI, YASB, Zebar, Rainmeter, ManagedShell and others work, with sources |
| [research/integrations.md](research/integrations.md) | Feasibility study for the app integrations |
| [research/wallpaper-sources.md](research/wallpaper-sources.md) | Where the bundled wallpapers come from and their redistribution terms |
| [research/scrolling-tiling.md](research/scrolling-tiling.md) | Scrolling (niri-style) tiling on Windows: prior art, Win32 edge cases, measured animation spike, verdict |

## Media

- [media/readme/](media/readme/): the screenshots used by the project README.
- [media/native/](media/native/) and [media/studio/](media/studio/): screenshots referenced by the docs above.

Plans and decisions live in [`plans/`](../plans/), starting with the
[native plan](../plans/council/native-plan.md) and [working live on main](../plans/native/LIVE.md).
