# Council brief: go fully native, with resources at almost nothing

You are one of three council members (Grok, GPT Sol, Claude Opus). This round is **read-only**: do not
edit, build, commit, or run the app. Read the code and docs; you may web-search. Return your answer
as your final message.

## What changed
The council spent 6 rounds on `plans/council/plan.md`, which keeps WebView2 and drives idle CPU to
~0 (read it, and reuse every part that still applies). The user then saw the RAM cost and decided:

> "yes then all goes native them. I want all resources to be minimized to almost nothing"

That means **CPU, RAM and GPU, at idle and in use, all minimized**. The always-on surfaces (top bar,
dock/taskbar, widgets, flyouts, notification center, desktop menu) become native. They no longer use
WebView2.

## Facts (`D:\Coding\projects\desktop\wallpaper`, main `f378a4c`)
- Processes (README "Architecture"): `mypaper-core` (supervisor, tray, styling journal),
  `mypaper-wallpaper` (wgpu, one device, one surface per monitor), `mypaper-widgets` (one transparent
  WebView2 per monitor, plus plugin processes over JSON-RPC stdio), `mypaper-bars` (top bar and dock as
  app bars, native taskbar control, tray host, flyouts). `apps/studio` is a Tauri settings app, open
  only while in use.
- **Memory** (`docs/performance.md` table, private MB for every process): image wallpaper alone ~255;
  `widgets` ~504; `bars` ~515; `full` ~670. The bars and widgets share one WebView2 browser/GPU
  process (`crates/desktop/src/webview_env.rs`); renderers are per site. A web wallpaper adds its own
  browser (~+100 MB).
- **Idle CPU on the user's machine** (% of one core): bars visible ~38%, desktop visible ~87%,
  fullscreen 1.2%. The plan above attributes most of it to in-page filters, `backdrop-filter`, and
  animations.
- **Code size** (lines): `crates/bars/src` 22.7k Rust (hosts, app bars, tray host, window list,
  flyout data services, journal); `bars/shell/src` 13.6k TS/CSS (the page); `crates/widgets/src` 13.1k
  Rust; `widgets/shell/src` 4.9k TS/CSS; 17 HTML widgets in `widgets/*` (clock, weather, calendar,
  now-playing, media-pill, system-monitor, todo, notes, photo, rss, crypto, heatmap, quote, timer,
  countdown, launcher, quick-toggles); a widget SDK (`packages/widget-sdk`) and docs that invite
  third-party HTML widgets (`docs/writing-widgets.md`).
- **Native UI that already exists:** the desktop right-click menu in Direct2D (`crates/desktop/src/menu_ui.rs`,
  `menu_model.rs`, `desktop_menu.rs`; opens in ~25 ms), the bars' native stand-in first paint, the DComp
  target and DWM host-backdrop sprites (`crates/bars/src/blur.rs`), and the engine's liquid-glass plate
  pass (`crates/wallpaper/src/glass_pass.rs`).
- **Themes** today style the bars and widgets with CSS (`theme.json` `"css"`), presets live in
  `crates/bars/presets/`, and `look.family` (`crates/bars/src/style.rs`) is one of mono, fluent, macos or
  material.
- **User style:** macOS-calm (Tahoe), flat translucent tint, no gloss or gradients; Liquid Glass is an
  optional, very cheap style; there are several design families; keep it simple (YAGNI), and reuse
  proven libraries and techniques rather than inventing new ones.
- **Research already done** (reuse it): `plans/council/liquid-glass-research.md`,
  `docs/research/customization-landscape.md`, and the menus/icons/motion findings summarized in
  `plans/council/brief.md`.

## Your task (round 1, native)
Write a concrete, opinionated plan, under ~1300 words, citing file paths. Fight scope creep.
1. **Resource targets.** Set numbers for idle and active CPU, private RAM, GPU, and process count:
   for the whole product excluding wallpaper content, and for each surface. Say what is physically
   reachable (for example, the cost of a D3D device or a DComp tree, and DirectWrite font caches).
2. **UI technology.** Choose between Direct2D + DirectWrite + DirectComposition (extending
   `menu_ui.rs`), wgpu + Vello, Slint, iced or egui, WinUI 3, Sciter, Ultralight, and Blitz. Judge
   each on RAM, idle behaviour, text quality (ClearType, IME, accessibility via UI Automation),
   animation, glass, and effort from Rust. Pick one and justify it.
3. **Process model.** How many processes? Merge the bars, widgets and desktop menu into one shell
   process, into core, or into the wallpaper process (which already holds a GPU device)? Cover crash
   isolation and the panic restore.
4. **The UI layer.** A small retained scene, layout, text, hit-testing, input, a11y, and animation
   that runs only while something moves. Choose between hand-rolled and a library such as taffy for
   layout or accesskit for UIA. Keep it as small as possible.
5. **Theming without CSS.** What a design family or theme becomes (tokens? a declarative format?),
   and how the existing presets and `look.family` map onto it.
6. **Widgets.** Port which built-ins natively, in what order. Decide what happens to the HTML
   widget SDK and third-party widgets: drop them, or keep an opt-in "web widget" kind that starts
   WebView2 only while such a widget exists (pay only for what you use, like web wallpapers).
   Decide what plugins over JSON-RPC become.
7. **Glass.** Decide how flat tint, DWM backdrop and Liquid Glass render natively at ~0 idle cost.
8. **Migration order** as small shippable slices that keep the app usable throughout, each with an
   acceptance check in numbers. Name what to delete at each step.
9. **What stays** from `plans/council/plan.md`: attribution, the web wallpaper fixes, native app
   styling (DWM, `ColorPrevalence`, D1 Windhawk), and the families. Note anything that becomes moot.
10. **Risks and kill criteria.**
