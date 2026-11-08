# Glaze Studio: the native redesign (N7)

Design spec, 2026-11-05. Mockups of every screen, light and dark: `plans/native/studio-redesign.html`.
It builds on `docs/studio-ux.md` (the web Studio's audit and model, which this keeps where it worked)
and on N7 of `plans/council/native-plan.md` (Studio rebuilt on `crates/ui`, at most 60 MB while open
and nothing while closed). The inventory in section 2 was built from `apps/studio` at `61a3da4`, then
checked against the Studio rows (S01–S33) of the parity audit, `plans/native/parity.md`. The
Integrations page follows the first batch recommended by `docs/research/integrations.md`. Both files
landed while this spec was being written.

## The answer

- **Seven places in the sidebar:** Home, Wallpapers, Look, Widgets, Bars, Integrations and, pinned at
  the bottom, Settings. Search (Ctrl K) reaches every setting, wallpaper, widget, integration and
  action from anywhere.
- **One Look editor** covers everything Glaze draws (top bar, dock, widgets, menus, panels) and,
  on its Windows tab, Windows' own windows and pointer. It replaces four look editors: Widgets › Look,
  the bar Look panels, the theme editor's skins, and the Windows page. The five styles and five
  material controls live there.
- **Every editing page has the same shape:** a live preview drawn by the real renderer, and an
  inspector that goes presets, then the common few, then **More**. Home is an overview with no
  inspector: clicking anything on it opens the right page with that thing selected.
- **Nothing is lost.** Section 2 maps every old setting and feature to its new place. What the native
  plan drops on purpose (CSS, gradients, glow, neon and others) is listed separately (section 11).
- **Seven decisions are yours** (section 0). Each one has a recommendation, so the build can start on
  the recommendations.

---

## 0. Decisions for Alex

| # | Decision | Recommendation | The other way |
|---|---|---|---|
| D1 | **The word "theme".** Today a theme is a folder holding a wallpaper and, optionally, a look. | Call the library **Wallpapers**. A wallpaper can **bring a look** (a badge on its card). The word "theme" leaves the UI. | Keep "Themes" as the page name, with "theme" meaning wallpaper plus look. |
| D2 | **Windows styling becomes yours, not the theme's.** Today it's saved inside the theme file, so on a built-in theme you must "Make a copy" first. | Store your Windows styling in your look (a user layer), with the wallpaper's own as the layer below. That removes the copy-first dead end. It needs a small schema addition: a user `windows` section next to the look. | Keep it in the theme file and keep "Make a copy". |
| D3 | **Glass in Studio's previews.** Studio has no GPU device (60 MB budget), and liquid glass is drawn by the engine. | The engine renders the preview's glass offscreen, once per change, into shared memory. Studio composites the shell's own content above it. While the engine is off, a flat tinted stand-in shows. Add **Peek** (Ctrl D) for a 1:1 check on the real desktop. | A stand-in only (a CPU blur and tint, no refraction). It's cheaper, but the material sliders won't show refraction. |
| D4 | **What a click on a wallpaper card does.** | Click **selects** it and shows its details (performance, look, credit). The card's hover button **Use**, a double-click or Enter applies it, with Undo. | Click applies at once, as today. |
| D5 | **Integrations as a top-level place.** The research recommends seven first features (media, launcher, notifications, T3 Code, OBS, Docker, Tailscale). | Make it top-level. It also gathers the built-in providers (media, notifications) and data plugins (weather, RSS). | Start it as a Settings section and promote it later. |
| D6 | **First-run starters:** four whole-desktop presets (style, wallpaper, bars preset and a few widgets). | Yes. They replace the empty first launch, and the bars "pairings" become their bar half. It's four presets to curate. | A first run with only a style picker. |
| D7 | **Material knobs the native material doesn't have:** bar saturation, glass depth, sheen, and the glass "Strength" master. | Drop them in N6. Strength folds into Refraction and Highlight. | Add them to `Material` in N6 (more data for the engine per plate). |

---

## 1. Principles

1. **Organize by the thing on screen, not by file.** There's one place per thing, and the preview shows it.
2. **The real thing, live.** Previews come from the same `view()` functions the shell runs,
   headless. Every change is written immediately, undoable, and visible on the real desktop as well.
3. **Few, then more.** Presets as visual tiles, the common few settings, then **More**. A setting
   that doesn't apply in the current mode isn't drawn.
4. **Plain words.** Say what a setting does, not how it works. No file names, process names or
   registry paths in labels (they may appear under Troubleshoot).
5. **Calm, monochrome, high-contrast grays.** No accent hue, gradients, gloss or faux 3D in Studio's
   chrome. Content (wallpapers, previews) keeps its own colors.
6. **Native-drawable.** Everything is fills, strokes, lines, text, icon glyphs, images, arcs, stepped
   shadows and RichEdit text fields (`crates/ui` ops). There are no blur filters or gradients in the
   chrome, and no HTML.

---

## 2. Inventory: every feature and setting, and its new place

Old places are as in `apps/studio` today. "Look ›" means the Look page, "Bars ›" the Bars page, and so
on. **(new)** marks things the old Studio didn't have. **(plan)** marks a change forced by the native
plan (section 11).

### 2.1 App-wide

| Feature | Old place | New place |
|---|---|---|
| Search everything (settings, themes, actions, pages) | Sidebar field, Ctrl K | Command palette, Ctrl K. It also searches widgets, integrations and recent changes. |
| Undo and redo (app-wide; slider drags merge into one step) | Page bar, Ctrl Z / Ctrl Shift Z | Same, plus Ctrl Y |
| Save state (Saving… / Saved) | Page bar | Page bar |
| Toast with Undo ("Applied Aurora · Undo") | Bottom center | Same |
| Status chip: Playing / Paused / Off, the theme on screen, pause and resume | Sidebar, bottom | Sidebar status block. It also shows the GPU, the frame rate and any part that stopped. |
| The settings file can't be read: Reset to defaults | Themes banner | A banner on whichever page uses the file. It keeps a backup (section 6). |
| Render error: Try again | Error boundary | Native Studio restarts the failed page's model. A crash restarts Studio, and core is unaffected. |
| Narrow window: icon rail under 1024 px; inspector under the stage under 1100 px | Layout | Icon rail under 1100 px; the inspector becomes a sheet over the stage under 960 px |
| (new) Studio's own light or dark | n/a | Settings › General › Studio appearance (Match Windows, Light, Dark) |
| (new) Peek: see the real desktop | n/a | The page bar's eye button, or Ctrl D |
| Follows edits made outside Studio (file watcher, parity S32) | Automatic | Same. Pages refresh live, and an editor never overwrites a newer file. |

### 2.2 Desktop page → Home

| Feature | Old place | New place |
|---|---|---|
| The live desktop: wallpaper, widgets and bars on the real geometry | Desktop stage | Home preview. Clicking a surface opens its page with it selected. |
| Click a widget, a bar or the wallpaper to edit it in place | Desktop inspector | Opens Widgets, Bars or Wallpapers with that item selected (one editor per thing) |
| Theme strip under the stage | Desktop | Home › Wallpapers strip |
| Pause / Resume | Desktop page bar | Home page bar, the sidebar status, the palette |
| Overview links (Widgets, Top bar, Bottom bar, Windows, Right-click menu) | Desktop inspector | Home › Your desktop list |
| Display switcher | Stage tools | Home, Widgets and Bars: the display chips under the preview |

### 2.3 Themes → Wallpapers

| Feature | Old place | New place |
|---|---|---|
| Gallery grouped by category (Yours, City & rain, Nature, Space, Cozy, Abstract, More) with counts | Themes | Wallpapers grid, the same groups |
| Search by name, kind, category and author | Themes page bar | Wallpapers page bar (and the palette) |
| Filter: All / Yours / Built in | Themes page bar | Wallpapers page bar |
| Apply, with Undo | Click a card | The card's **Use** button, a double-click, Enter, or the inspector's **Use this wallpaper** (D4) |
| "On your desktop now" badge, NC badge, kind badge, look badges | Card | Card: an *On desktop* dot, a *Personal use* badge for NC, the kind, and a *Brings a look* badge |
| A broken theme shows its error and sinks to the end | Card | Same. The inspector shows the reason, and Use is disabled. |
| Edit / Copy and edit | Card pencil | Inspector › **Its look** › Edit (opens Look scoped to this wallpaper); built-ins: **Make a copy** |
| Empty states (no themes, nothing matches, no own themes) | Themes | Wallpapers (section 6) |
| New theme: picture, video, shader (.wgsl), 3D scene (.glb, .gltf) | New theme | Wallpapers › **Add wallpaper** › File |
| New theme: a folder of pictures (a slideshow) | New theme | Add wallpaper › Folder |
| New theme: a web page (URL) or an .html file | New theme | Add wallpaper › Web page |
| New theme: a program (an app wallpaper) | New theme | Add wallpaper › App |
| Name | New theme details | Add wallpaper › Name |
| Fit: Fill / Fit / Stretch / Center (with picture tiles) | New theme details | Add wallpaper, and later in its inspector › On the screen |
| An app's arguments with `{hwnd}`, plus presets | New theme details | Add wallpaper › App › Start it with; later in its inspector › App |
| Use it right away | New theme details | Add wallpaper › **Add and use** (the primary button) or **Add** |
| Change source | New theme details | Add wallpaper › Back |
| Theme editor › Wallpaper tab: react to the mouse (web, 3D scene) | Theme editor, Desktop › Wallpaper | A wallpaper's inspector › Mouse |
| Credit: category, author, license, source, the NC notice | Wallpaper inspector | A wallpaper's inspector › Credit |
| Theme editor › Windows tab | Theme editor | Look › Windows, scoped to this wallpaper (D2) |
| Theme editor › Widgets look and Bars look (the theme's skin) | Theme editor | Look, scoped to this wallpaper: "Editing the look Rain City brings" |
| Theme editor › liquid glass knobs for widgets | Theme editor | Look › Material, scoped to this wallpaper |
| Theme editor › style sheet (CSS): choose, replace, remove | Theme editor | **(plan) Removed.** CSS is gone from the native shell. |
| Use this theme (from the editor) | Theme editor page bar | A wallpaper's inspector › Use this wallpaper |
| Make a copy to edit (a built-in) | Inspector | A wallpaper's inspector › Make a copy (only needed to edit a built-in's own look) |
| Preview generation | Automatic | Same |
| (new) Per-wallpaper GPU and frame-rate cap | n/a | A wallpaper's inspector › Performance: Same as Settings, or its own |
| (new) Remove one of your wallpapers | n/a (file system) | A wallpaper's inspector › More › Remove (undoable for the session; the folder goes to the Recycle Bin) |
| (new) Show a wallpaper's folder | n/a | A wallpaper's inspector › More › Show in folder |

### 2.4 Widget look, bar look and the theme skins → Look

Old widget styles (minimal, depth, neon, paper, aero, liquid) and surfaces map onto the five native
styles **(plan)**:

| Old | New |
|---|---|
| liquid | Liquid |
| aero | Liquid, with more Clarity |
| minimal | Mono |
| depth, paper | Solid |
| neon | Mono (its glow is gone) |

| Setting | Old place | New place |
|---|---|---|
| Widget style (6 tiles) | Widgets › All widgets | Look › **Style** (5 tiles: Liquid, One UI, Solid, Fluent, Mono) |
| Surface: None / Glass / Solid | Widgets › All widgets | Look › More › Different for › Widgets › Background: Style's / None |
| Color (surface tint) | Widgets › All widgets | Look › Material › Tint |
| Opacity | Widgets › All widgets | Look › Material › **Clarity** (clear to tinted) |
| Blur | Widgets › All widgets | Look › Material › Blur |
| Corners | Widgets › All widgets | Look › More › Shape › Corners |
| Text: Light / Dark | Widgets › More | Look › More › Text and icons › Text on the wallpaper (Auto, Light, Dark) |
| Accent | Widgets › More | Look › **Accent** (one accent for everything: From wallpaper or a color) |
| Font | Widgets › More | Look › More › Text and icons › Font |
| Size (scale) | Widgets › More | Look › More › Text and icons › Size |
| Edge line | Widgets › More | Look › More › Shape › Edge line |
| Text shadow | Widgets › More | Look › More › Text and icons › Text shadow |
| Liquid: Bend | Theme editor | Look › Material › Refraction |
| Liquid: Shine | Theme editor | Look › Material › Highlight |
| Liquid: Tint amount | Theme editor | Look › Material › Clarity |
| Liquid: Blur | Theme editor | Look › Material › Blur |
| Liquid: Strength | Theme editor | Folds into Refraction and Highlight (D7) |
| "Liquid glass doesn't show over web/app/video wallpapers" hint | Widgets › Style | Gone: with D-W, glass is live over every wallpaper |
| Bars: light or dark | A bar › Look | Look › **Appearance** (Light, Dark, Match Windows); a bar's own under Bars › a bar › More › Look |
| Bars: glass style (liquid, frosted, none) | A bar › Look | Look › Style; per bar: Bars › a bar › More › Look › Style |
| Bars: background color | A bar › More look › Surface | Look › Material › Tint (or per bar) |
| Bars: corner radius | A bar › More look › Surface | Bars › a bar › More › Look › Corners |
| Bars: glass intensity, refraction, highlight, tint, blur | A bar › More look › Glass | Look › Material (or per bar) |
| Bars: glass menus (menus and panels as glass) | A bar › More look › Menus and panels | Look › More › Different for › Menus and panels |
| Bars: "drawn like" (Glaze, macOS, Windows 11, ChromeOS) | A bar › More look | Bars › a bar › Design (the design sets it), with an override under More › Look › Drawn like |
| Bars: icons (Segoe Fluent, SF Symbols style) | A bar › More look | Look › More › Text and icons › Icon style (Lucide, Fluent) **(plan: N6 bundles both)** |
| Bars: border width and color | A bar › More look › Edges and depth | Look › More › Shape › Edge line; per bar, under its More › Look |
| Bars: shadow | A bar › More look › Edges and depth | Bars › a bar › More › Look › Shadow (on or off; native `Shadow` op) |
| Bars: module and app buttons at rest, hover and pressed (text and fill) | A bar › More look › Buttons | Bars › a bar › More › Look › Buttons |
| Bars: running-indicator colors | Dock › More look | Bars › Dock › More › Look › Running indicators |
| Bars: base appearance (shared by both bars) | Bars › Look for both bars | Look (it *is* the base now) |
| Bars: a bar's look back to its design (reset) | A bar's header | Same (the bar's header ↺) |
| (new) Five styles for every surface | n/a | Look › Style |
| (new) Clarity, Blur, Refraction, Highlight, Tint | n/a | Look › Material (only what the style uses, section 4.3) |
| (new) The look a wallpaper brings, edited in place | Theme editor | Look's scope switch: **Your look** / **The look Rain City brings** |
| (new) Different for bars, widgets, or menus and panels | n/a | Look › More › Different for |

### 2.5 Windows page → Look › Windows (and Settings › Desktop)

| Setting | Old place | New place |
|---|---|---|
| Looks: As is, Wallpaper, Dark, Light, Glass, Sharp | Windows › Look tiles | Look › Windows › tiles: **As is, Match Glaze, Glass, Sharp**. Dark and Light are Match Glaze with Appearance Dark or Light. "Wallpaper" is Match Glaze with Accent and Edge From wallpaper. |
| Accent color: As is / From wallpaper / Pick | Windows | Look › Windows › Accent: Same as Glaze's (default), As is, Pick |
| Light or dark apps: As is / Dark / Light | Windows | Look › Windows › Apps: Match Glaze, Light, Dark, As is |
| Corners: As is / Round / Small / Square | Windows | Look › Windows › Corners |
| Material: As is / Mica / Acrylic / Mica tabbed / None / Windows decides | Windows | Look › Windows › Window material |
| Window edge: As is / From wallpaper / Pick | Windows › More | Look › Windows › More › Window edge |
| Title bar color | Windows › More | Look › Windows › More › Title bar |
| Title text color | Windows › More | Look › Windows › More › Title text |
| Pointer: As is / Dark / Light / Other (sets and a folder) | Windows › Pointer | Look › Windows › Pointer |
| Reset everything to how Windows has it | Windows header ↺ | Look › Windows header ↺ |
| "Saved with Aurora" | Windows page bar | The scope switch (D2): your look, or the wallpaper's |
| Right-click menu: use Glaze's | Windows › Right-click menu | Settings › Desktop › Right-click menu |
| Menu items: reorder, remove, add a built-in, add a separator, reset | Windows › Right-click menu | Settings › Desktop › Right-click menu (with a live menu preview) |
| Your own menu item (name, program, arguments) | Windows › Right-click menu › More | Settings › Desktop › Right-click menu › Add your own |
| Full menus in Explorer (the classic Windows 11 menu) | Windows › Right-click menu › More | Settings › Desktop › Explorer's menus |

### 2.6 Widgets

| Feature | Old place | New place |
|---|---|---|
| Widgets live on the desktop: drag to move, snap to cells, refuse overlaps | Widgets stage | Widgets canvas (the same rules) |
| Resize (cells) | Inspector › Size; stage | Drag the corner handle on the canvas; Inspector › Size |
| Add from the catalog, with search and counts | Widgets › Add widget | Widgets › **Add widget** (catalog sheet with categories and search) |
| No free space: a message | Add | Same, plus "Make room": it selects the largest widget |
| Arrange on the real desktop (edit mode) | Widgets page bar | Widgets page bar › Arrange on desktop (or Ctrl Alt Shift W anywhere) |
| What's on this display (a list) | All widgets inspector | Widgets inspector, nothing selected › On this display (also the keyboard and screen-reader route) |
| Always-on-top widgets (the overlay) | All widgets › Always on top | Widgets, nothing selected › Always on top |
| A widget's own options (from its manifest, in groups) | Widget inspector | Widget inspector (section 2.6.1) |
| Options set "on the desktop" (lists, pictures) | Edit on desktop | **In Studio now (plan):** list editors, image pickers, font and place pickers |
| Its own look (on or off, with look fields) | Widget inspector | Widget inspector › Look › Its own look: style and material overrides |
| Duplicate | Widget header | Widget header, or Ctrl Shift D on the canvas |
| Remove (undoable) | Widget header | Widget header, or Delete on the canvas |
| Not installed: placeholder, offer Remove | Stage, inspector | Same |
| Place (weather location): Automatic or a town, with search | Widget inspector | Same |
| (new) Move with the keyboard: arrows move a cell, Shift+arrows resize | n/a | Canvas |

#### 2.6.1 The 17 native widgets' options (all in the Widget inspector)

| Widget | Options |
|---|---|
| Clock | Face (Digital, Flip, Analog, Greeting, World); 24-hour; seconds; date; cities (list, World face); name to greet; font |
| Calendar | Layout (Month, Agenda, Week); week starts on; week numbers |
| Countdown | Look (Number, Ring); event; date; time; once it's passed; seconds; ring start |
| Crypto | Coins (list); currency; 24 h change; refresh |
| Activity heatmap | Style (Weeks, Month, Streak); count (everything, tasks, focus); week start; labels; legend |
| Launcher | Layout (Dock, Grid, List); apps (list, with an app picker); names; icon size; search bar; engine; custom URL |
| Now playing pill | Shape (Pill, Notch, Mini); progress line |
| Notes | Text size |
| Now playing | Look (Card, Backdrop, Strip, Visualizer); artwork colors; glow; spectrum; sensitivity |
| Photos | Frame; photos 1–6 (image pickers); change every; motion; shuffle; fit; caption |
| Quick toggles | Layout; tiles: Wi-Fi, Bluetooth, Focus, Airplane, Night light, Sound, Display, Battery |
| Quote | New quote (daily, hourly, on click); source; your quotes (list) |
| RSS | Feeds (list); display; headlines; source; time; refresh |
| System | Look (Rings, Bars, Graphs); CPU, memory, GPU, network, drives, battery; only these drives; update every; a color per metric |
| Timer | Focus, short and long breaks; custom; sessions before a long break; auto-advance |
| To do | Title; add field position; hide completed |
| Weather | Layout (Card, Compact, Forecast); location (place); units; animated illustration |

A select with three or more visual variants renders as **tiles** with a small preview of each
(drawn by the widget's real view). Lists use the sortable list. Files and pictures use the native
picker.

### 2.7 Bars

| Feature | Old place | New place |
|---|---|---|
| Live bars on your wallpaper; click a bar or a module | Bars stage | Bars stage (real `bars` views, headless) |
| Drag a module within a list, to another list or bar, or off to remove it (a caret shows the drop) | Bars stage | Same |
| Pairings (Monochrome, macOS, Windows 11, Windows 10, ChromeOS, Islands, Neon glass, Floating) | Bars › Pairings | Bars, nothing selected › **Presets** (Neon glass loads as Floating, **plan**) |
| Top bar / bottom bar on or off | Bars overview, a bar | Same |
| Windows taskbar: Keep / Auto-hide / Hide; icon alignment | Bars overview | Bars, nothing selected › Windows taskbar |
| Per display settings: list, remove one | Bars › Per display | Same (only when there are any) |
| Start over (the bars file can't be read) | Bars trouble | Bars banner › Start over (keeps a backup) |
| "Not running: changes apply when the bars start" | Bars page bar | A banner with **Start** |
| **Top bar** › design (live thumbnails) | A bar › Design | Same |
| Top bar › edge (top, bottom), height | A bar › Layout | Same |
| Top bar › shape (strip, floating, islands), width (fit, full, share, pixels), margin, padding, spacing | More layout | The top bar › More › Layout |
| Top bar › clock seconds and format | More layout › Clock | The top bar › More › Clock (also the Clock module's inspector) |
| Top bar › displays (all, primary, choose) | More layout | The top bar › More › Displays |
| **Dock** › design | A bar › Design | Same |
| Dock › style (floating, taskbar), edge, icon size, auto-hide, magnify | Dock › Layout | Same |
| Dock › margin, padding, spacing; magnify scale and reach | Dock › More layout › Room | Dock › More › Layout |
| Dock › taskbar style: apps alignment, window titles, the taskbar's clock | Dock › More layout | Dock › More › Taskbar (only for the taskbar style) |
| Dock › running apps (all, this display, pinned only), one per window, count badge, previews and delay, bounce, attention, running indicator, after the apps (desktop, bin) | Dock › More layout › Apps | Dock › More › Apps |
| Dock › pinned apps: Windows' pins or your own; add, reorder, remove | Dock › Pinned apps | Same |
| Dock › displays | More layout | Dock › More › Displays |
| Module lists (Start, Middle, End): reorder, remove, open | A bar › Modules | Same |
| Add: every module not shown, a gap, your own (name and program) | Add a module | Same |
| A module: click, middle click, right click, scroll up and down (default, nothing, an action, a program) | Module inspector | Same |
| A module: icon (icon-pack name or picture), text color, background, padding, radius, text size and weight | Module inspector › Icon and look | Same |
| Your own module: program, arguments, run every; delete it everywhere | Module inspector | Same |
| Motion: animations on or off; appearing style, duration and curve (named, cubic, spring) | A bar › Motion | A bar › More › Motion |
| Motion: pointing, pressing, menus opening; magnify, launch bounce, attention | A bar › Motion › Movements | A bar › More › Motion |
| Reduce motion (Like Windows, Always, Never) | Bars › Motion and power | Settings › Performance and power › Reduce motion (global, every surface) |
| Save power (On battery, Always, Never) | Bars › Motion and power | Settings › Performance and power › Save power |
| Start menu: layout, search recent files, the Windows key opens it, its pinned apps | Bars › Menus and panels | Bars, nothing selected › **Panels** › Start menu (two layouts, **plan**) |
| Sound: let the mixer switch the output device | Menus and panels | Bars › Panels › Sound |
| Calendar: other time zones, week starts on | Menus and panels | Bars › Panels › Calendar |
| Tray: collect the tray icons | Menus and panels | Bars › Panels › Tray (not while the Windows taskbar is hidden) |
| Icon packs: choose a pack, start your own, single apps' icons | Bars › Icon packs | Look › More › Text and icons › **Icon pack** (it applies to the dock, top bar and launcher) |
| Theme variables (CSS custom properties) | Bars › Look for both bars | **(plan) Removed** with CSS |
| (new) Notifications: Win+N and Win+A open Glaze's | n/a | Bars › Panels › Notifications |
| (new) Integration modules (for example T3 Code status) | n/a | Add a module › From integrations |

### 2.8 Settings

| Setting | Old place | New place |
|---|---|---|
| Start with Windows | Settings › Startup | Settings › General |
| Show the theme while Windows starts | Settings › Startup | Settings › General › Show the wallpaper while Windows starts |
| Pause on battery | Settings › Power | Settings › Performance and power |
| Pause behind fullscreen apps | Settings › Power | Settings › Performance and power |
| Glaze: running or stopped; Start, Stop | Settings › Glaze | Settings › Troubleshoot › Glaze (and the sidebar status) |
| Emergency stop hotkey (shown) | Settings › Glaze | Settings › Keyboard, Settings › Troubleshoot, first run |
| Parts: core, wallpaper, widgets, bars (on, off, not installed) | Settings › Details | Settings › Troubleshoot › Parts: core, engine, shell (**plan**: three processes), each with Restart |
| How it starts (sign-in task or startup entry) | Settings › Details | Settings › Troubleshoot › Details |
| Installed at | Settings › Details | Settings › Troubleshoot › Details |
| (new) Graphics card: Integrated (default), High performance, Let Windows choose | n/a | Settings › Performance and power |
| (new) Frame rate: the wallpaper's own, or a cap (24, 30, 60, 120, the display's, or a number 1–360) | n/a | Settings › Performance and power |
| (new) Widget edit mode and overlay shortcuts (shown) | n/a | Settings › Keyboard |
| (new) Restore the Windows desktop now | n/a | Settings › Troubleshoot (also the palette and the tray) |
| (new) Reset one part's settings (bars, widgets, look), keeping a backup | Bars only | Settings › Troubleshoot › Start over |
| (new) Copy diagnostics, Open logs | n/a | Settings › Troubleshoot |
| (new) Version, licenses, wallpaper credits | n/a | Settings › About |

### 2.9 New areas

| Feature | Place |
|---|---|
| Integrations (first batch from the research: T3 Code, OBS, Docker, Tailscale, GitHub): connect, status, where each shows (bar module, notifications, panel), how it updates, its options, disconnect | Integrations |
| Built-in providers (media, including Spotify; notifications, covering Discord and chat apps) and data plugins (weather, RSS): status, what uses them | Integrations › Built in |
| First run | A welcome flow over the window (section 5.1). Later: the palette's "Start from a preset…", and Settings › Troubleshoot › Start over |

**Count:** the tables above have 163 rows for old features and settings (each widget's options
count as one row) and 22 rows for new ones. Every old row has a new place. The only removals are
the native plan's (marked **plan**, all listed in section 11) and the knobs in D7.

---

## 3. Information architecture and navigation

### 3.1 The places

| Place | Question it answers | Main parts |
|---|---|---|
| **Home** (Ctrl 1) | "What's on my desktop, and is it OK?" | The live desktop (click anything to edit it), the wallpapers strip, status (GPU, frame rate, CPU and memory), Your desktop (a row per thing, one click from its editor) |
| **Wallpapers** (Ctrl 2) | "What's behind everything?" | Library grid, Add wallpaper, a wallpaper's inspector: use, on the screen, mouse, performance, its look, credit |
| **Look** (Ctrl 3) | "How does it all look?" | The **Desktop** tab (style, material, appearance, accent; More: text and icons, shape, Different for) and the **Windows** tab (presets, accent, apps, corners, material, pointer; More: edges, title bars) |
| **Widgets** (Ctrl 4) | "What's on my desktop?" | Canvas, Add widget, a widget's options and size, Always on top |
| **Bars** (Ctrl 5) | "My top bar and dock." | Stage, presets, a bar, a module, panels, Windows taskbar |
| **Integrations** (Ctrl 6) | "What does it connect to?" | Connected, Built in, Available; a selected integration's details |
| **Settings** (Ctrl ,) | "How does Glaze behave?" | General, Performance and power, Desktop, Keyboard, Troubleshoot, About |

There are six places plus Settings, down from seven: the theme editor, the Windows page and New theme
fold into Look and Wallpapers. Settings sits at the bottom of the sidebar, apart from the six.

### 3.2 Navigation

- **Sidebar** (232 px; a 64 px icon rail under 1100 px): the mark and name, a search field that opens
  the palette, the six places, then Settings, then the **status block** (state, wallpaper, pause).
  The current place has a `selected` fill and ink-1 text. The status block turns into a warning row
  when a part has stopped ("Shell stopped · Restart").
- **Page bar** (52 px): the title, one line of context ("Rainy Hong Kong · Display 1"), the save
  state, undo and redo, **Peek**, the page's main action, and the window's caption buttons (Studio
  draws its own title area, so snap layouts work through `WM_NCHITTEST`).
- **Inside a page:** selection, not tabs. The inspector shows what's selected, and its header has a
  back link to the parent ("‹ Bars"). Esc goes up one level. Look's Desktop | Windows switch and the
  Settings sub-list are the only second-level navigation.
- **Deep links:** every setting has an id (as `settingsIndex.ts` does today). The palette and
  in-app links open the page, select the owner, open the **More** around it, scroll to it and
  flash it once.
- **Command palette** (Ctrl K): one field, results grouped Settings, Actions, Wallpapers, Widgets,
  Integrations, Go to, with Recent first when empty. Actions include Pause, Peek, Arrange widgets,
  Restart the shell, Restore the Windows desktop, Use (wallpaper), Add a widget, Start from a
  preset. Typing a setting's name and pressing Enter goes there; toggles can be flipped right in
  the list (Space).

### 3.3 Progressive disclosure

Every inspector follows the same order, and each step is optional:

1. **Header:** icon, name, one line of context, then reset ↺, duplicate and remove where they apply.
2. **Presets:** visual tiles (styles, presets, designs, widget layouts, pointer sets).
3. **The common few:** 3 to 6 rows.
4. **More:** a closed disclosure holding the rest, grouped (Layout, Look, Motion, Displays). Search
   can open it.

A value inherited from below shows a quiet chip ("From Rain City", "Default", "Follows Look").
Touching it makes it yours; ↺ on the row (shown on hover or focus) hands it back. **Layers, from
the bottom:** the style, then the wallpaper's look, then your look, then a bar's or a widget's own.
Your choice always wins.

---

## 4. Pages

### 4.1 Home

- **Preview** (left, 16:10): your desktop as it is now. It's drawn from the wallpaper's still and
  the shell's real views (bars, widgets) at the screen's geometry, scaled. Hovering outlines a
  surface and shows a name tag. Clicking opens its page with it selected: a widget opens Widgets, a
  bar or module opens Bars, and bare wallpaper opens Wallpapers with the current one selected.
  Right-clicking the bare wallpaper opens Settings › Desktop › Right-click menu.
- **Display chips** under the preview: Display 1 · 2560×1440, Display 2.
- **Wallpapers strip:** the current wallpaper and five recent ones, then **+**. Clicking one uses it
  (with Undo), and "All wallpapers ›" goes to the library.
- **Right column:** a status card (Playing or Paused, the wallpaper, the GPU and frame rate, Glaze's
  CPU and memory, Pause) and **Your desktop**: Look, Widgets, Top bar, Dock, Right-click menu and
  Integrations. Each row shows its current value and is one click from its editor.
- **Off state:** when core isn't running, Home shows "Glaze is off" with **Start**, and the preview
  dims (section 6).

### 4.2 Wallpapers

- **Grid** of cards (aspect 16:10, a 248 px minimum, grouped by category with counts). A card shows the
  picture, its name, the kind and its badges. On hover it shows **Use**. Selected cards get a 2 px
  ink-1 outline; the active one has an *On desktop* dot.
- **Inspector** (on selection):
  - preview, name, kind, and **Use this wallpaper**;
  - **On the screen:** fit (image, video, slideshow);
  - **Mouse:** react to the mouse (web, 3D scene);
  - **Performance:** Graphics card and Frame rate, each "Same as Settings (Integrated · 30 fps)" by
    default or its own. A line under them shows what the engine is using now, read from the engine.
  - **App** (app wallpapers): arguments, presets, restart the app;
  - **Its look:** what it brings (style, appearance, accent swatch), with Edit (scoped Look) or, on a
    built-in, **Make a copy**;
  - **Credit:** author, license, source, and the NC notice;
  - **More:** Show in folder, Remove (yours only).
- **Add wallpaper** (a sheet over the page): four sources as large tiles (File; Folder; Web page;
  App), then a details step over a live preview: name, fit, the app's arguments, an optional
  performance override, then **Add and use** or **Add**.

### 4.3 Look

- **Stage:** a *specimen* on your wallpaper, built from real views: the top bar, an open calendar
  panel, the desktop menu, two widgets and the dock. On the Windows tab it's two app windows
  (active and inactive) with title bars, edges, corners and the pointer. A toggle under the stage
  switches between **Specimen** and **Display 1** (your real layout).
- **Scope switch** in the page bar: **Your look** / **The look *Rain City* brings** (only when the
  wallpaper brings one). Editing a built-in's look offers Make a copy inline.
- **Desktop tab:**
  - **Style:** five tiles, each a tiny bar and widget drawn in that style;
  - **Material**, only the controls the style uses:

    | Style | Clarity | Blur | Refraction | Highlight | Tint |
    |---|:-:|:-:|:-:|:-:|:-:|
    | Liquid | ✓ | ✓ | ✓ | ✓ | ✓ |
    | One UI | ✓ | ✓ | | | ✓ |
    | Fluent | ✓ | ✓ | | | ✓ |
    | Mono | ✓ | | | | ✓ |
    | Solid | | | | | ✓ (the fill) |

  - **Color:** Appearance (Light, Dark, Match Windows); Accent (From wallpaper, or a swatch, or a
    hex);
  - **More** › Text and icons: font, size, text shadow, text on the wallpaper, icon style, icon
    pack;
  - **More** › Shape: corners, edge line;
  - **More** › Different for: Bars, Widgets, Menus and panels. Each is "Follows Look" (default) or its
    own style and material.
- **Windows tab:**
  - **Windows look** tiles (As is, Match Glaze, Glass, Sharp);
  - Accent, Apps (light or dark), Corners, Window material;
  - **Pointer** tiles;
  - **More:** window edge, title bar, title text;
  - one quiet line at the end: "Undone when Glaze stops or you press Ctrl Alt Shift Backspace."

### 4.4 Widgets

- **Canvas:** the display at its geometry, with widgets drawn by their real views. A faint dot grid
  shows while dragging, resizing or adding. Selection is a 2 px ink-1 outline over a 1 px dark line
  (readable on any wallpaper), with a resize handle and a name tag. Dropping onto an occupied cell is
  refused, and the widget springs back.
- **Page bar:** display chips, **Arrange on desktop**, **Add widget**.
- **Inspector, nothing selected:** Always on top, On this display (a list), and a link to "How
  widgets look" (Look, Different for › Widgets).
- **Inspector, a widget:**
  - header: icon, name, size and display, duplicate, remove;
  - the manifest's groups in order, with variant selects as tiles;
  - **Size;**
  - **Look:** Its own look (off: "Follows Look").
- **Add widget** (a sheet): search, then categories (Time, Media, Info, Productivity, Launch), cards
  drawn by the widget's real default view. Click adds it to the first free cell and selects it.

### 4.5 Bars

- **Stage:** both bars at their real size on a "readable" screen width, as today: the bars keep their
  size, and only the room between modules shrinks. Hovering outlines a bar or a module; dragging
  moves a module.
- **Inspector, nothing selected:**
  - Bars (two rows with switches);
  - **Presets** (tiles with silhouettes);
  - Windows taskbar;
  - **Panels:** Start menu, Calendar, Sound, Tray, Notifications;
  - Per display, when there are any.
- **A bar:**
  - Show it;
  - **Design** (live thumbnails);
  - the common few (top bar: edge, height, shape; dock: style, edge, icon size, auto-hide, magnify);
  - **Modules** (Start, Middle, End, each a sortable list, with Add);
  - Dock: **Pinned apps**;
  - **More:** Layout, Look (its own style and material, drawn like, corners, edge, shadow, buttons,
    running indicators), Motion, Displays.
- **A module:** what it is (one line); When clicked; Other clicks and scrolling; Icon and look; for
  your own, What it shows (program, arguments, run every); Remove from this bar; Delete everywhere.
- **Add a module:** where (Start, Middle, End), the built-ins not shown, the integration modules, a
  gap, or your own.

### 4.6 Integrations

The page is a list on the left and details on the right (no stage). Its content follows
`docs/research/integrations.md`: shared Windows providers first, then a few small native clients,
and nothing that wakes the PC while idle.

- **List groups:**
  - **Connected:** for example T3 Code and OBS.
  - **Built in:** Media (any player that publishes to Windows, Spotify included); Notifications
    (one listener: badges and the notification center, which covers Discord, Grok Bot and chat apps
    without a client per app); Weather and RSS (plugins).
  - **Available:** Docker, Tailscale, GitHub, then later candidates (Home Assistant, calendars).

  Each row has an icon (a generic glyph: no brand logos in Studio), a name, what it adds, and a
  status (Connected, On, Needs attention, Connect).
- **Details:**
  - header with status and **Connect** or **Disconnect**;
  - **Shows in:** switches for a bar module, notifications or a panel, each with a small real
    preview of what it adds ("2 agents · 1 needs you");
  - **Updates:** Live, or When you open it. The research gates live updates on proving no idle
    wake-ups; if that fails, the integration falls back and says so here.
  - **Options** (from the integration);
  - **Privacy:** one line ("Read-only. Connects to T3 Code on this PC with a token you approve. No
    transcripts.");
  - **More:** account, re-authorize, logs.
- **Connecting** asks the local app for a scoped, read-only token (T3 Code, OBS), or opens the
  service's own consent page (GitHub, calendars). Studio shows "Waiting for T3 Code…", then
  Connected. No integration borrows another app's credentials or injects into it.
- **Discord:** voice and mute controls need Discord's approval for public distribution, so Discord
  shows as notification cards and launcher actions through the shared providers. If approval comes,
  it gets its own row.

### 4.7 Settings

A sub-list on the left (General, Performance and power, Desktop, Keyboard, Troubleshoot, About) and
grouped rows on the right, at most 720 px wide.

- **General:** Start with Windows; show the wallpaper while Windows starts; Studio appearance.
- **Performance and power:**
  - **Graphics card:** tiles for Integrated (with the detected name, recommended), High performance
    and Let Windows choose;
  - **Frame rate:** the wallpaper's own, or a cap;
  - **Per wallpaper:** "2 wallpapers have their own ›" (it filters the library);
  - Pause on battery; Pause behind fullscreen apps; Save power; Reduce motion;
  - a live line: "Now: Intel Iris Xe · 30 fps · 2.8% CPU".
- **Desktop:** Right-click menu (on or off; the items as a sortable list beside a live preview of
  the real menu; add a built-in, a separator or your own; reset); Explorer's menus (full menus).
- **Keyboard:** emergency stop, widget edit mode, always-on-top widgets, the palette, Peek. All are
  shown; none are rebindable yet.
- **Troubleshoot:**
  - Glaze running or stopped (Start, Stop);
  - **Parts** (core, engine, shell: state, memory, Restart);
  - **Restore the Windows desktop** (the emergency stop, as a button);
  - **Start over** (reset the bars, widgets or look, keeping a backup);
  - Copy diagnostics; Open logs; Details (how it starts, installed at).
- **About:** version, update channel (when one exists), licenses, wallpaper credits.

---

## 5. Key flows

### 5.1 First run (D6)

1. Studio opens over a dimmed window: **"Make this desktop yours."** It offers four starters as
   large tiles, each a real mini render: **Calm** (Liquid, the macOS preset, Rainy Hong Kong, clock
   and weather), **Familiar** (Fluent, the Windows 11 preset, Spring forest), **Bold** (One UI,
   Islands, Star Nest), **Quiet** (Mono, top bar only, Snowfall). Picking one applies it to the
   real desktop at once, and a Peek hint shows.
2. **"Your taskbar":** Replace it with Glaze's bars (default for the starter); Keep Windows'
   taskbar and add widgets only; Hide everything (wallpaper only).
3. **"You're set":** Start with Windows (on), and the safety line with keycaps: "If anything goes
   wrong, press Ctrl Alt Shift Backspace. Windows comes back as it was." Then **Open Studio**.

Every step can be skipped, and each is one undo step.

### 5.2 Switch the wallpaper ("switch theme")

From Home's strip (one click), the Wallpapers grid (Use on hover, or select then Use), or the
palette ("milky" → Enter). The toast says "Using Milky Way · Undo". If the wallpaper brings a look,
it says "Using Milky Way and its look · Undo". Your own look choices still win (section 3.3).

### 5.3 Change the look

Look, then a Style tile: the specimen and the real desktop restyle together. Material sliders
update the preview while you drag, and the drag is one undo step. **Peek** (Ctrl D) hides Studio
until you press any key, so you can judge it on the real desktop. ↺ in the header returns to the
wallpaper's look, or the style's defaults.

### 5.4 Arrange widgets

1. Widgets, then **Add widget**, then a card. It lands in the first free cell, selected.
2. Drag it on the canvas; the dot grid shows and it snaps. Drag the corner to resize.
3. The inspector shows its options (variant tiles first).
4. For pixel-perfect placement on the real screen, use **Arrange on desktop** (native edit mode).
   Studio follows the layout file live.

### 5.5 Set up bars

1. Bars, then a **Preset** (macOS, Windows 11…). It sets both bars' designs and which are on.
2. Click the top bar: design, edge, height. Drag modules on the stage, or reorder them in the list.
3. **Add** in Modules: built-ins, integration modules, a gap, or your own program.
4. Click the dock: style, size, magnify, Pinned apps.
5. Windows taskbar: Hide (default with both bars on).

### 5.6 Add a wallpaper

1. Wallpapers, then **Add wallpaper**, then a source: **File** (picture, video, shader, 3D scene),
   **Folder** (slideshow), **Web page** (a URL field, or Choose an .html file), **App** (pick a
   program).
2. Details over a live preview: name, fit (tiles), for an app its arguments (`{hwnd}`) with
   presets, and optionally Performance.
3. **Add and use**. The toast offers Undo. Web and app wallpapers get live glass over them (D-W), so
   nothing about the look changes.

### 5.7 Connect an integration

1. Integrations, then **T3 Code** › **Connect**. Studio finds the local app and asks it to approve
   a read-only token.
2. **Shows in:** turn on the top bar module (it's added to End, and the toast offers "Show me"),
   notifications, and the agent panel the module opens.
3. Its options (for example, also notify when an agent finishes). The status reads Connected, and
   Updates reads Live (or When you open it, if live updates fail the idle gate). **Disconnect**
   removes what it added, in one undo step.

### 5.8 Troubleshooting and panic

- **Something looks wrong now:** press **Ctrl Alt Shift Backspace** anywhere (core handles it,
  whether Studio is open or not). Studio, if open, shows "Glaze is stopped. Windows is back as
  it was." with **Start Glaze**.
- **A part stopped:** the sidebar status says "Shell stopped" and links to Troubleshoot. There,
  Parts lists each with its state, memory and **Restart**.
- **Settings broke:** the page that uses the file shows a banner ("Your bar settings can't be read:
  line 14") with **Start over** (keeps `bars.json.bak`) and **Show file**. Studio never rewrites a
  file it can't read.
- **Slow or hot:** Settings › Performance and power shows the live line, and Graphics card and
  Frame rate are right there. A wallpaper's own override is one click away.
- **Help someone help you:** Troubleshoot › Copy diagnostics copies versions, parts, GPU, the
  settings files' validity and the last errors, with no personal paths.

---

## 6. Empty, loading and error states

| Where | State | What shows |
|---|---|---|
| Any page | Loading | Local reads are fast: content draws when ready. Past 150 ms, static skeleton blocks show (panel fill, no shimmer). Previews draw the wallpaper still first, then surfaces as their views are ready. |
| Previews | Engine off (no glass) | A flat tinted stand-in, with one line under the stage: "Glass shows when Glaze is running." |
| Home | Glaze is off | The preview dims to 40%, and a centered card says "Glaze is off" with **Start Glaze** |
| Home | Paused | The status card reads Paused, and the preview badge says "Paused" |
| Wallpapers | No wallpapers of your own (Yours filter) | "Your own wallpapers show here." **Add wallpaper** |
| Wallpapers | Search finds nothing | "Nothing called 'xyz'." with **Search everything** (opens the palette with the text) |
| Wallpapers | A wallpaper can't load | The card shows "Can't load" with an alert glyph; the inspector gives the reason and offers Show in folder |
| Widgets | None on this display | The canvas shows the empty desktop and one centered **Add a widget** button |
| Widgets | Not installed | A placeholder tile ("Unknown widget") on the canvas; the inspector offers Remove |
| Widgets | No room | A toast: "No room for Weather on Display 1." with **Make room** |
| Widgets | A plugin failed (place search) | An inline line under the field: "Search isn't available right now." |
| Bars | Bars not running | Banner: "The bars aren't running. Changes apply when they start." **Start** |
| Any file-backed page | The file can't be read | Banner with the reason and line, **Start over** (backup kept), **Show file**; editing disabled |
| Integrations | Nothing connected | The Available list is the page; a one-line intro at the top |
| Integrations | Needs attention | Row status "Needs attention"; details show the reason and **Reconnect** |
| Settings › Performance | High performance chosen, none found | An inline line: "No high-performance graphics card found; using Intel Iris Xe." |
| Palette | No results | "No matches. Try a setting's name, a wallpaper or a widget." |

Errors never use color: an alert glyph, ink-1 text and a clear action. Destructive actions are
undoable rather than confirmed. The only confirmation is Start over, which replaces a file.

---

## 7. Keyboard and accessibility

### 7.1 Keyboard

| Keys | Does |
|---|---|
| Ctrl K | Command palette |
| Ctrl 1 – 6, Ctrl , | Home, Wallpapers, Look, Widgets, Bars, Integrations; Settings |
| Ctrl Z, Ctrl Shift Z / Ctrl Y | Undo, redo (outside text fields) |
| Ctrl D | Peek: Studio hides until any key or click |
| F6 / Shift F6 | Next or previous region: sidebar, page bar, stage, inspector |
| Tab / Shift Tab | Next or previous control in the region |
| Arrows | Inside composites: segmented, tiles, lists, the wallpaper grid, the sidebar |
| Enter / Space | Activate; Space toggles a switch |
| Esc | Close a popover, then clear the selection, then go up one level |
| Alt ↑ / Alt ↓ | Move the focused row in a sortable list (modules, pinned apps, menu items) |
| Canvas: arrows / Shift arrows | Move the selected widget a cell / resize it |
| Delete | Remove the selected widget or module (undoable) |
| Ctrl Shift D | Duplicate the selected widget |
| Ctrl F | Focus the page's search (Wallpapers, Add widget) |

Focus moves into the inspector when a stage item is selected with the keyboard, and back to the
item on Esc.

### 7.2 Accessibility

- **Contrast** (section 8.3): body text is at least 4.6:1 on every surface in both modes. Control
  boundaries that identify a control (switch tracks, slider tracks, text fields, checkbox edges) are
  at least 3.3:1. The focus ring is ink-1, 2 px, with a 2 px gap: at least 11:1.
- **Screen readers:** AccessKit from the hits, as in `crates/ui`. Roles: navigation list, radio
  group (segmented, tiles), slider (value text "Blur, 12 pixels"), switch (toggle), list with
  reorder actions, dialog (palette, sheets), status (toasts, save state as a polite live region).
  Stage items are buttons labeled "Weather widget, 3 by 2, column 9, row 1". Every stage action also
  exists in the inspector or a list, so nothing needs a pointer.
- **Targets:** at least 24×24 DIPs; rows are 36 high; controls 32 (28 compact).
- **Text size:** Studio follows Windows' Text size (Accessibility). Rows grow, and nothing truncates
  without a tooltip.
- **Contrast themes:** with a Windows contrast theme on, tokens map to system colors (Window,
  WindowText, Highlight, GrayText, ButtonFace). Previews keep their colors inside a 1 px outline.
- **Motion:** with Windows' animation effects off, or Reduce motion on, every transition is
  instant. Nothing in Studio animates on its own.
- **Never by color alone:** state is a glyph or a shape (a filled dot, a ring, the pause glyph, the
  alert triangle), plus text.

---

## 8. Visual system

Studio's chrome is monochrome. Content (wallpapers, live previews) keeps its colors. Sizes are DIPs;
`crates/ui` multiplies by the DPI scale.

### 8.1 Type: Segoe UI Variable (DirectWrite, grayscale AA), Cascadia Mono

| Token | Size / line | Weight | Use |
|---|---|---|---|
| `display` | 28 / 36 | Semibold (Display optical size) | First run, the off state |
| `title-1` | 20 / 28 | Semibold | Sheet titles (Add wallpaper, Add widget) |
| `title-2` | 15 / 20 | Semibold | Page bar titles, inspector header titles |
| `heading` | 13 / 18 | Semibold | Section titles in the inspector and Settings |
| `body` | 13 / 20 | Regular | Row labels, descriptions |
| `body-strong` | 13 / 20 | Medium | Buttons, sidebar items, emphasized labels |
| `caption` | 12 / 16 | Regular | Hints, row details, values (tabular figures) |
| `micro` | 11 / 14 | Medium | Badges, keycaps, name tags, counts |
| `mono` | 12 / 16 | Cascadia Mono | Paths, commands, hex values |

### 8.2 Space, size and radius

- **Spacing scale:** 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48, 64.
- **Layout:**
  - sidebar 232 (rail 64); page bar 52;
  - gutter 24 (32 at 1440 px and wider);
  - inspector 340 (380 at 1600 px and wider), with 16 padding and 24 between sections;
  - Settings content at most 720 wide, with 32 between groups;
  - rows 36 (compact 32); controls 32 (small 28).
- **Radii:** 4 (keycaps, badges); 6 (buttons, inputs, segmented items); 8 (rows, tiles, thumbnails,
  menu rows); 12 (cards, the stage frame, popovers, the palette, sheets; the main panel's top-left
  corner); full (switches, pills, dots).

### 8.3 Grays (tokens and contrast)

| Token | Dark | Light | Role |
|---|---|---|---|
| `bg` | `#0a0a0a` | `#f5f5f5` | The main panel (content area) |
| `chrome` | `#141414` | `#e9e9e9` | Sidebar, title area |
| `panel` | `#1c1c1c` | `#ffffff` | Inspector, cards, inputs, popovers |
| `hover` | `#282828` | `#ececec` | Hover fill |
| `selected` | `#363636` | `#dcdcdc` | Current nav item, selected segment, selected row |
| `line` | `#2b2b2b` | `#e2e2e2` | Dividers |
| `edge` | `#404040` | `#cccccc` | Card and panel edges (decorative) |
| `control` | `#787878` | `#7f7f7f` | Boundaries that identify a control |
| `ink-1` | `#f5f5f5` | `#0a0a0a` | Primary text, icons, selection outlines, focus ring |
| `ink-2` | `#c8c8c8` | `#333333` | Body text |
| `ink-3` | `#a0a0a0` | `#5c5c5c` | Muted text, hints |
| `ink-4` | `#666666` | `#a3a3a3` | Disabled, placeholders |
| `solid` / `on-solid` | `#f2f2f2` / `#0a0a0a` | `#111111` / `#fafafa` | Primary buttons, the switch's on track |
| `scrim` | `rgba(0,0,0,.6)` | `rgba(0,0,0,.28)` | Behind the palette and sheets |

Measured WCAG contrast (worst surface each text tier sits on):

| Pair | Dark | Light |
|---|---:|---:|
| ink-1 on bg / panel / selected | 18.2 / 15.6 / 11.1 | 18.2 / 19.8 / 14.4 |
| ink-2 on bg / panel / selected | 11.8 / 10.2 / 7.2 | 11.6 / 12.6 / 9.2 |
| ink-3 on bg / panel / hover / selected | 7.6 / 6.5 / 5.6 / 4.6 | 6.1 / 6.7 / 5.7 / 4.9 |
| ink-4 (disabled, exempt) on panel | 3.0 | 2.5 |
| control on bg / panel / hover | 4.5 / 3.9 / 3.3 | 3.7 / 4.0 / 3.4 |
| solid text (on-solid on solid) | 17.6 | 18.1 |

The gray steps are wider than T3 Code's: each surface step (bg → chrome → panel → hover → selected)
is clearly visible, and hover differs from selected by a full step, not a hint.

### 8.4 Components

| Component | Spec |
|---|---|
| Sidebar item | 36 high, radius 8, a 16 icon with a 12 gap, `body-strong`. Rest: ink-3. Hover: `hover` fill, ink-1. Current: `selected` fill, ink-1. |
| Button | 32 high (28 small), radius 6, 12 padding (10 small), `body-strong`, a 16 icon. **Primary:** `solid` fill, `on-solid` text; hover `#d6d6d6` (light: `#333`). **Secondary:** `panel` fill with a 1 px `edge`; hover `hover` with a `control` edge. **Ghost:** no fill; hover `hover`. Busy: the label stays and a 12 px arc spins (`Op::Arc`). |
| Icon button | 32×32 (28), radius 6, ghost by default, with a tooltip of its label |
| Switch | 36×20 track, 16 thumb. Off: a 1.5 px `control` ring with a `control` thumb. On: `solid` track, `bg` thumb. Moves on `spring`, 140 ms. |
| Segmented | 32 high, `panel` with an `edge`, 2 inset. Items radius 5. Selected: `selected` fill, ink-1, medium. Others ink-3, with hover ink-1. |
| Slider | 4 px track (`control`), the filled part ink-1, a 16 thumb (ink-1 with a 2 px `bg` ring). The value sits at the right in `caption`, tabular. Arrows step; Shift ×10; Home and End. |
| Select | Looks like a secondary button with a chevron; opens the native `Menu<C>` popup (no gap, it flips at edges) |
| Text field | 32 high, `panel` fill, a 1 px `control` edge, radius 6. Focus: a 2 px ink-1 ring. Windowless RichEdit. |
| Color field | A swatch row (From wallpaper, 8 presets, custom), plus a hex field. Custom opens a picker whose hue strip and saturation-value square are CPU-generated images (`Op::Image`, no gradient op needed). |
| Tile picker | Grid, tile radius 8, preview 16:10 (pointers 1:1), label below. Selected: a 2 px ink-1 outline and ink-1 label. Hover: a 1 px `control` outline. |
| Row (property) | Label (`body`), and optionally a hint (`caption` ink-3); the control right-aligned; at least 36 high. An inherited value shows a chip ("From Rain City"). ↺ shows on hover or focus when the value is set. |
| More | A 36 high row, a chevron that turns 90°, `body-strong` ink-2. Content shows at once (no height animation). |
| Sortable list | 36 high rows: a grip on hover, an icon, a label, trailing actions on hover or focus. A dragged row lifts (`Shadow`); a 2 px ink-1 caret marks the drop. Alt+arrows reorder. |
| Badge | 20 high, radius 4, `micro`, `panel` with an `edge` (or `solid` for "On desktop") |
| Keycap | 20 high, at least 20 wide, radius 4, a 1 px `edge` plus a 1 px darker bottom, `micro` |
| Status dot | 8: running is a filled ink-1 dot; stopped is a `control` ring; paused is the pause glyph; a problem is the alert glyph |
| Stage frame | Radius 12, a 1 px `edge`. Selection: a 2 px ink-1 outline inside a 1 px `#000` at 60% (readable on any wallpaper). The name tag is `micro` on `solid`. |
| Inspector header | 56 high: a 32 icon tile (`hover` fill, radius 8), `title-2`, a `caption` detail, then actions. A back link (‹ Bars, `caption`) sits above it when nested. |
| Page bar | 52 high: `title-2`, a `caption` detail in ink-3, then a right cluster: save state, undo, redo, Peek, the main action, caption buttons |
| Toast | Bottom center, 40 high, radius 10, `panel` with an `edge` and a `Shadow`. Glyph, text, one action, close. 6 s, paused while hovered or focused. |
| Banner | In the page, radius 8, `panel` with an `edge`. Alert glyph, one sentence, up to two buttons. |
| Command palette | 640 wide, radius 12, `panel` with an `edge` and a `Shadow`, over the `scrim`. A 48 high input (15 px). Results are 36 rows: icon, title, section (right, `caption`). The selected row has a `selected` fill. Group headers are `micro` ink-3. A 32 footer holds keycaps. |
| Sheet | Centered, 880 wide, radius 12, over the `scrim`. `title-1` header with a close button; content; footer actions right-aligned. |

### 8.5 Elevation

The page itself is flat (no shadows). Popovers, the palette, sheets, toasts and dragged rows use
one stepped shadow: a 24 px blur at 45% black in dark, and at 12% in light, offset 8 down. It's
drawn as `Op::Shadow` rings plus a 1 px `edge`.

### 8.6 Motion (springs from `crates/ui`)

| What | Motion |
|---|---|
| Hover, press | Instant (crisp; nothing animates while the pointer just moves) |
| Page change | Content fades in over 120 ms |
| Drill-in (inspector push, back) | 160 ms on `spring-soft`: a 16 px slide plus a fade |
| Palette, sheet, popover open | 120 ms fade plus a 4 px rise; close: 80 ms fade |
| Toast | 160 ms rise and fade in; 120 ms fade out |
| Switch thumb, segmented selection | 140 ms on `spring` |
| Drag | 1:1 with the pointer; on drop it settles in 200 ms on `spring`; a refused drop springs back |
| Reduce motion / animation effects off | Everything is instant |

The ticker runs only while something moves (Rule 2), so an idle Studio draws nothing.

---

## 9. What N7 needs from `crates/ui` and the shell

- **An app-window surface kind:** resizable, with a DWM shadow, snap layouts through
  `WM_NCHITTEST` (caption buttons drawn by Studio), and a minimum size of 960×640. It needs no
  `WS_EX_LAYERED` alpha path: Peek just hides and shows the window.
- **A scroll container** (clip plus offset, with wheel, keyboard and scrollbar hits): inspector, grid,
  Settings.
- **Controls** built on ops and hits: switch, segmented, slider, select (reusing `Menu<C>`), tiles,
  sortable list (drag plus Alt+arrows), color field, and the number stepper.
- **Headless previews:** the shell's `bars`, `widgets`, flyout and menu `view()`s callable from Studio
  with Studio's model and a scale (views already multiply by their scale), drawn into Studio's
  target. The goldens prove this path exists.
- **Glass for previews (D3):** an engine request "draw these plates over this still, at this size,
  into shared memory", answered once per change and freed when Studio closes. The stand-in is the
  still, CPU box-blurred once per material change, plus the tint.
- **Images:** wallpaper stills and thumbnails decoded at preview size (`Ui::decode_image` with
  `max_size`), and released when scrolled away, to stay under 60 MB.
- **CPU-generated images** for the color picker's hue strip and saturation-value square.

---

## 10. What gets simplified or merged, and why

| Change | Why |
|---|---|
| Four look editors become **one Look** (Desktop and Windows tabs) | N6 makes one token set restyle everything, so one place says so. "Which one wins?" goes away. |
| The theme editor page is gone; a wallpaper's look is edited in Look with a scope switch | Same controls, same preview; no second copy of the editor |
| Windows styling becomes a user layer (D2) | It removes the "Make a copy to edit" dead end on the theme on screen (the most common case) |
| "Themes" become **Wallpapers** that can bring a look (D1) | One object in the files, one word in the UI |
| Home becomes an overview with no inspector | One editor per thing. Home answers "is it OK" and links; it doesn't duplicate Widgets or Bars. |
| Widget styles (6) and bar glass styles become **five styles** | The plan's styles; neon, paper and depth were the looks the user rejected (visual restraint) |
| About 40 bar look knobs become Material plus a short per-bar More › Look | Native draws flat fills, edges, shadows and plate glass. Gradients, images, grain, glow and per-state glow are plan losses. |
| "Edit on desktop" for widget lists and pictures goes | All widget settings live in Studio (plan); one place to set a widget |
| Icon packs move from Bars to Look | They change the dock, top bar and launcher, so they're a look, not a bar |
| Reduce motion and Save power move from Bars to Settings | They apply to every native surface now, not just the bars |
| The desktop right-click menu moves from Windows to Settings › Desktop | It's Glaze behavior; its look already follows Look. It keeps a live preview. |
| Bar pairings become **Presets**, and first-run starters build on them | One preset concept for bars; starters add the style and wallpaper on top |
| Settings sections are reorganized: General, Performance and power, Desktop, Keyboard, Troubleshoot, About | The engine and its parts move out of the first screen, under Troubleshoot; GPU and fps get a home |
| Built-in plugins appear in Integrations | Everything that brings outside data in is in one list |
| Parts go from four to three (core, engine, shell) | The native plan's processes |

---

## 11. Losses (from the native plan) and at-risk settings

Removed by the native plan, not by this redesign. Studio drops their controls, and a file that
still has them loads with one logged warning (N6):

1. Theme style sheets (CSS) for widgets and bars, and the bars' theme variables.
2. Gradient, image and wallpaper-glass **fills** for bar surfaces; grain (noise); glow (border and
   per-state); neon presets' decoration (Neon glass loads as Floating); bevel or shaped corners.
3. The widget styles neon, paper and depth as distinct looks (mapped in 2.4).
4. Two of the four Start menu (launcher) layouts.
5. Settings edited on the widget itself ("Edit on desktop"), HTML widgets, and ambient widget
   motion.
6. The old Installed list's widget permissions (`launch`, `network`…): already gone in the web
   redesign.

**At risk, decided by D7:** bar saturation, glass depth, sheen and the glass Strength master. Native
`Material` is clarity, blur, refraction, highlight and tint.

## Decisions (Alex, 2026-11-05)
- D1: the UI says **Wallpapers**, not "theme"; a wallpaper can bring a look.
- D2: Windows styling is **part of the user's Look** (small schema addition), not inside a theme file.
- D3: previews use **engine-drawn glass**, plus **Peek** (Ctrl+D) to check the real desktop.
- D4: clicking a wallpaper card **selects** it; a **Use** button applies it.
- D5–D7: take the spec's recommendations.
