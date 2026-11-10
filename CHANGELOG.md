# Changelog

All notable changes to Glaze are recorded in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-11-09

The all-native rewrite. The shell (top bar, dock, flyouts, menus, widgets) and Studio draw with software Direct2D through the stillkit UI library, only the wallpaper engine holds a hardware GPU device, and no part of the shell uses WebView2. The product is now named Glaze: binaries are `glaze-*`, settings live in `%APPDATA%\glaze`, and `scripts/migrate-local.ps1` moves an existing local install.

### Wallpaper studio and effects

- Studio is a native app on the same UI kit as the shell, and closing it ends its process.
- First run opens a full-screen setup: choose the taskbar, then optionally a look, top bar, dock, wallpaper and extras; nothing else changes unless you pick it, and `glaze-studio --onboarding` shows it again.
- Your desktop, Look, Widgets and Bars are edited on the real desktop: **Edit on desktop** docks Studio as a panel while you pick, drag and resize the actual bars and widgets.
- Changes save as you make them, with app-wide undo and redo, and **Ctrl+K** searches every setting, wallpaper, widget and action.
- Wallpapers is a grid with a details pane (Apply, Edit, Show in folder, Remove), and Settings is split into tabs.
- The wallpaper editor builds a wallpaper from a base (image, video, colour, gradient, WGSL shader, Shadertoy-style GLSL, 3D scene or another wallpaper) plus layers of effects, images and 3D models, saved as an ordinary theme folder.
- 38 bundled effects in six categories (weather, camera, colour, motion, stylize, light) open from a searchable gallery, and hovering one previews it on the canvas.
- The shader editor reloads live, shows compile errors on their lines and turns `@param` comments into sliders.
- The editor shows an estimated GPU cost; a still stack renders once and costs nothing at rest, and a moving stack bakes the layers below its first animated one.
- Built-in wallpapers stay read-only: editing one saves "<Name> (edited)" to your own wallpapers.
- The rain and snow wallpapers are native WGSL shaders instead of web pages.
- The wallpaper renders on the GPU that drives your screens by default, and the GPU (auto, integrated, high performance) and frame rate apply live, per wallpaper too.
- Fifteen curated, openly licensed wallpapers. Studio shows each one's author, licence and source. Eight of them, and the Rain on glass effect, are CC BY-NC-SA 3.0 (non-commercial). All third-party licences are in `THIRD-PARTY-NOTICES.md`.

### Liquid Glass

- The bars, dock, widgets, flyouts and menus sit on Liquid Glass drawn by the wallpaper engine with stillkit-glass, stillkit's copy of liquid-rust (by al1h3n, MIT OR Apache-2.0): a refracting lens, light frost, a directional rim light and a soft shadow.
- Clarity (clear to tinted), blur, refraction, highlight and tint are adjustable for everything at once or per family (bars, widgets, menus).
- Five styles apply to every surface in light and dark: Liquid (Liquid Glass, the default), One UI (frosted), Fluent (acrylic), Solid and Mono (no glass).
- Thin glass, such as a 24 pt menu bar, gets a shallow lens with a flat face, so labels don't sit on the highlight.
- Glass draws a single edge that matches its surface, and nothing draws an outline on top of it.
- Over app windows, menus, flyouts and covered bars use Windows' own backdrop frost, with the engine's captured glass as a fallback.
- Widgets and bars also get live glass over `web` and `app` wallpapers, and that capture stops while the screen is covered.
- Glass redraws only when the pixels behind it change; a still wallpaper under still glass costs no GPU time.
- The prototype's widget designs (depth, neon, paper, aero) and the neon bar glow are gone; settings that name them still load.

### Dock and bars

- A native top bar and dock replace the taskbar, which can be kept, auto-hidden or hidden and is always restored; with the dock on, the taskbar is hidden rather than auto-hidden.
- Presets include macOS, Windows 11, Windows 10, ChromeOS, monochrome, islands, floating, pill and edge, and any top-bar design pairs with any dock.
- A bar can drop its glass and sit straight on the wallpaper (Background: Transparent); the macOS top bar does this by default.
- The dock groups windows by app, including Squirrel apps such as Discord and Slack with their pinned shortcut, and shows live window previews, badges and running indicators; magnification is opt-in.
- Right-clicking a dock app shows its Windows jump list (pinned, recent, frequent, custom categories and tasks) with icons.
- One visibility setting can hide the dock while a maximized window is in front.
- A tray host shows your tray icons without injecting code into Explorer.
- Native flyouts cover Control Center (Wi-Fi, Bluetooth, do not disturb, night light, airplane mode, power mode, display, sound), the Wi-Fi list, a per-app volume mixer, keyboard layouts, power, a calendar with extra clocks, and a notification center grouped per app.
- With the taskbar hidden, Win opens the launcher on the monitor under the pointer to search apps, settings and files, with pinned and recent apps and clipboard history; Win+N and Win+A open notifications and quick settings.
- The desktop right-click menu is Windows 11's full menu with each item's real icon, shell extensions and the New submenu; Shift or a right-click on an icon still gets Windows' own.
- Bar modules include media, tray, network, volume, battery, stats, keyboard language, notifications, custom commands and plugin modules such as T3 Code, OBS, Docker and Tailscale.

### Files

- Glaze Files is a native file browser that runs only while one of its windows is open.
- Tabs keep their own history, selection and search, come back on the next launch, and can be dragged out into a new window.
- Back, Forward, Up, clickable breadcrumbs, an editable address bar (Ctrl+L), pinned folders, and This PC with drive capacity bars.
- Details view has sortable, resizable Name, Date modified, Type and Size columns, and a large folder shows its first entries before the listing finishes.
- Copy, cut, paste, rename, delete and new folder use Windows' own file operations, so progress, conflicts, the Recycle Bin and undo work as in Explorer; **Show more options** opens the full shell menu.
- The preview pane (Alt+P) and quick look (Space) show images and text, and other formats through Windows' preview handlers running outside Files.
- An open folder refreshes from Windows change notifications, with no polling.
- Files follows your style, light or dark scheme, accent and glass.
- With the file index running, search covers indexed file names and folders show cached sizes, with incomplete sizes labelled; without it, search filters the current folder.
- Experimental, off by default: `experimental.files_default` sends folder opens and Win+E to Files through journaled per-user settings, and `glaze-files --restore-routing` undoes it.
- Experimental, off by default: `experimental.file_index` runs a native name and path index with folder sizes for Files and the launcher, refined by `index.include` and `index.exclude`; when it is off, no index process runs.

### Widgets and plugins

- 17 built-in native widgets, each with layouts per size: clock, calendar, countdown, timer, now playing, media pill, weather, RSS, crypto, system monitor, quick toggles, launcher, photos, notes, to-do, activity heatmap and quote.
- Edit mode (Ctrl+Alt+Shift+W) uses a fine per-monitor grid with one-cell moves and resizes, arrow-key moves and a searchable add-widget picker.
- Widgets follow the selected style and sit on the desktop layer or float above apps.
- A custom widget is one JSON file with a stat, list, text, image or gauge layout, fed by a value, an HTTPS URL, a file or a command, and it runs only while visible.
- Plugins are WebAssembly components in one helper process that starts when a plugin is needed and exits 30 s after the last one unloads.
- A plugin can add widgets, top-bar and dock modules, panels, desktop-menu items, global shortcuts and style patches, and each capability needs a permission declared in its manifest.
- A plugin that crashes is restarted from its saved state and stopped after three crashes in five minutes, without affecting other plugins.
- Studio > Integrations > Plugins installs `.glaze-plugin` packages or folders after showing their permissions, shows each plugin's CPU time and memory, and can disable or uninstall it, removing its data and stored credentials.
- T3 Code, Discord, Docker, OBS, Tailscale and Grok Bot run as bundled plugins, and placed widgets and bar modules move to them with their settings.
- The T3 Code widgets show running threads, threads that need you (approve or deny), a new-thread prompt, 30-day usage, and per-provider limits with reset countdowns.
- Experimental: a TypeScript plugin SDK (QuickJS) for the same component contract.
- Removed: HTML, CSS and JavaScript widgets and their SDK, and theme CSS for widgets and bars; older JSON-RPC executable plugins still run, without a sandbox.

### Windows styling

- The `look.style` switch also styles Windows: window borders, title bars, corners, backdrop (Mica, acrylic), accent colour and light or dark mode, with per-field overrides in Studio > Look > Windows.
- An `"auto"` accent takes its colour from the wallpaper.
- Frame styling reaches only allowlisted, Microsoft-signed Windows apps that are not elevated; games, anti-cheat processes, full-screen windows and third-party apps are never touched.
- Corners offer As is, Default, Round, Small and Square, and a preference that an app resets is reapplied within about a second.
- Style changes move straight to the new values, and releasing one field restores only that field.
- Every change to Windows is journaled and undone on quit, on the panic hotkey (Ctrl+Alt+Shift+Backspace) and after a crash; `glaze-core --restore` undoes everything without UI and reports the result as one JSON line.
- After an Explorer restart the taskbar is hidden again and the bars' reserved space, the tray and the wallpaper come back; Glaze never restarts Explorer itself.
- Optional, reversible Windows preferences in `config.json` (`system`): Game Mode, file extensions, hidden files, separate folder processes and power mode; first-run setup offers the first three.
- Cursor themes (built-in dark and light sets) and the opt-in classic Windows 11 context menu are journaled and reversible.
- Experimental, off by default: the App styler (`experimental.app_styler`) restyles the inside of Explorer's separate folder windows and, with its own opt-in, Settings; it never attaches to the Explorer that runs the desktop and taskbar, and known hangs and crashes keep it experimental.

### Performance

- Only the wallpaper engine holds a hardware GPU device; the shell and Studio draw in software, and a test checks that no GPU vendor driver loads into the shell.
- On the development machine (two 2560×1440 screens, release builds, 5 November 2026), the shell with bars, dock and six widgets idled at 32 MB private memory and core at 2 MB, with three steady processes and no WebView2 process.
- On that machine's two-screen fixture (rain-city at 30 fps, 16 glass plates, integrated GPU), the engine uses 257 MiB private memory, down from 448 MiB.
- After 10 s with every screen covered, the engine releases its drawing resources and leaves the wallpaper's still on screen; in live tests on the development machine it went from 171–173 MiB to 123–132 MiB and drew again 33–62 ms after a screen was uncovered.
- During games, background work in the bars, widgets, plugins and file index pauses, and `engine.during_games` (default `"pause"`) holds the wallpaper too; `system.game_activity: false` turns the game observer off.
- In one live test on the development machine, with a full-screen window covering one screen, the engine paused both screens, measured 0.000% of a core, went from 208.7 MiB to 124.7 MiB, and resumed in 74 ms.
- Settings and folder watchers sleep on Windows change notifications instead of polling every 100 ms.
- Surfaces redraw only changed regions, timers run only while something moves, and the top-bar clock ticks every second only when its format shows seconds.
- Integrations subscribe only while a widget, module or panel shows them, and stop when it closes.
- The Rain on glass effect builds its drops at half resolution: 3.4 ms down to 1.6 ms per frame at 1080p on the development machine's integrated GPU.

### Fixes

- The shell no longer crashes when it exits.
- A crashed or killed core no longer leaves the wallpaper engine drawing: the engine, the file index and the App styler exit with it.
- A hung leftover shell is ended after a grace period instead of blocking start-up or a restore.
- Apps launched from Glaze start from your default environment instead of inheriting variables from whatever started Glaze.
- Widgets stay on their own monitor when Windows renumbers displays instead of piling onto the main one.
- A widget layout upgrade keeps the old file beside the new one, and an older Glaze can't overwrite a newer layout.
- Flyouts and menus on glass open in place without flashing, and a second click on a bar module closes its flyout.
- The tray overflow stays open while an app's own menu is open, and the bars recover their reserved screen space and the dock's edge reveal.
- Glass recovers after a lost GPU device and stays in place across monitor changes and resizes.
- Built-in effects take precedence over copies packaged with a theme, so effect fixes (such as the black CRT and the flat halftone) reach older wallpapers.

### Known issues

- If Glaze's core and shell are both killed at once (for example "End process tree" in Task Manager), the Windows taskbar stays invisible until Glaze starts again, `glaze-core --restore` runs, or Explorer restarts. Quitting, crashes of either process alone, and uninstalling all restore it.
- After a crash, window frames that Glaze styled return to Windows' defaults rather than their exact previous look.
- The Windows corner setting reaches only allowlisted Windows apps (Explorer, Settings, Notepad, Terminal and a few others); browsers and other third-party apps keep their own corners. Windows itself limits corners to its own few styles. Skipping elevated windows was checked in code, not live.
- A still wallpaper built in Studio wakes the engine about 2.4 times a second, against about 0.6 for a plain image. It does no GPU work, but it isn't yet as idle as a plain image.
- Studio's live preview still costs some CPU while an animated wallpaper plays in the editor; decoding moved off the UI thread, so the gain in the editor is modest.
- The Print halftone effect is heavy on dark photos and can dominate a busy stack (for example fog or light leaks).
- A faint seam can remain on the 3D planet's rings.
- The package's Windows identity is self-signed. On a PC without Developer Mode it may not register, and Glaze then runs without the features that need it (such as notifications). Windows SmartScreen may warn about the unsigned installer.
- Tested on one PC (two monitors, 100% scaling, AMD graphics) and in a fresh profile with Windows' software renderer; not yet on a separate clean machine.

## [0.0.1] - 2026-11-04

The WebView2 prototype, kept for reference under the `v0.0.1` tag.

[0.1.0]: https://github.com/Snowy7/glaze/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/Snowy7/glaze/releases/tag/v0.0.1
