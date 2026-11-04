# Studio: the experience

How mypaper Studio is organized and why. Read this before adding a page or a setting. The rules
at the end ("Rules for new settings") are the short version.

## 1. Audit (November 2026, before the redesign)

### Who edits what, and how often

| What | Who | How often | Lived in |
|---|---|---|---|
| Switch theme, pause/resume | everyone | daily | Gallery (hero + cards), sidebar chip |
| Widget look (surface, blur, style…) | most users | after a theme change | Widgets → Look |
| Add / move / remove widgets | most users | weekly | the desktop's edit mode only; Studio listed them as cards |
| A widget's own options (clock format…) | most users | rarely | the desktop's edit mode only |
| Bar preset, top bar, dock | most users | setup, then rarely | Bars, 9 tabs |
| Dock pinned apps | dock users | occasionally | Bars → Dock |
| Bar modules and their clicks | tinkerers | setup | Bars → Modules (chip boxes) |
| Accent, title bars, corners, dark mode, cursors | most users | setup | Gallery → Customize → Windows (built-in themes: copy first) |
| Theme's own widget/bar look, CSS | theme authors | rarely | Gallery → Customize |
| New theme from a file/folder/page/program | most users | occasionally | New theme |
| Desktop right-click menu, classic menu | some users | once | Settings → Desktop extras |
| Start with Windows, pause on battery/fullscreen | everyone | once | Settings |
| Engine, components, panic hotkey | when something breaks | rarely | Settings (shown first) |

### Pain points

1. **Organized by file, not by thing.** Navigation followed the files (`config.json`, `theme.json`,
   `widgets.json`, `bars.json`). "How widgets look" had two homes (Widgets → Look, and a theme's
   Customize sheet) with no word on which wins. Accent color, a desktop-wide setting in the user's
   mind, sat three levels deep inside a theme.
2. **Dead ends.** Customize on a built-in theme (the most common case: the one on screen) showed a
   lock and a "Duplicate" button instead of the editor.
3. **No real preview.** Widgets → Look drew a sample clock card; Bars drew a 360 px sketch; Windows
   styling a tiny mock window. Nothing showed *your* desktop. Widgets on the desktop were a list of
   cards with "Cell 1, 4 · Size 3×2".
4. **Flat walls of settings.** Every option on screen at once, each with a paragraph of internals
   ("a mouse hook inside mypaper-core's own process", "Raw Input, like Lively", "Task Scheduler
   logon task", `[data-widget="clock"]`, file names). Settings that don't apply were shown and
   explained ("Only glass surfaces blur", "the dock is a floating pill, which has no modules").
5. **Abstract editors.** Bar modules were chips in "Before the task buttons / After the task
   buttons / Available" boxes with arrow-key instructions; the desktop menu was a list with up/down
   arrows; pinned apps a third list design.
6. **Two save models.** Theme editors had Save/Revert/Clear; everything else saved as you went. No
   undo anywhere, so live saving felt risky.
7. **No way to find a setting** short of opening every tab.
8. **Developer status first.** Settings opened on engine paths and process names.

## 2. Information architecture

Organized by the things on screen. Six places, each answering one question:

| Page | Question | Contents |
|---|---|---|
| **Desktop** (home) | "What does my desktop look like, and how do I change that bit?" | The live desktop: wallpaper, widgets, top bar, dock. Click anything to edit it right there. Theme switcher and pause. |
| **Themes** | "Which wallpaper?" | Gallery (click = apply, with undo), New theme, Edit theme. |
| **Widgets** | "What's on my desktop?" | The desktop with widgets in front: add from the catalog, drag to move, click to set options or look. The look of all widgets. |
| **Bars** | "My top bar and dock." | The live bars on your wallpaper: click a bar or a module to edit it, drag modules to move them; pairings and the Windows taskbar when nothing is picked (section 6). |
| **Windows** | "How do windows, pointers and menus look?" | Accent, borders, title bars, corners, material, light/dark, pointer; the desktop right-click menu; Explorer's classic menu. |
| **Settings** | "How does mypaper behave?" | Startup, power, and (one step away) the engine and its parts. |

Search (**Ctrl K**, or the field at the top of the sidebar) reaches every setting, theme, widget
and action from anywhere.

### Layers, in plain words

Looks stack: **built-in → theme → you**. Your choice always wins. The inspector shows a value that
comes from below as "From theme" (or "Default") instead of explaining layers; touching it makes it
yours, and the reset arrow hands it back. The theme layer is only edited in **Themes → Edit
theme**; the Windows page edits the theme on screen, and says so in one line ("Saved with
Aurora"). A built-in theme is copied with one click, in place, without leaving the page.

## 3. Interaction model

### One page shape: stage + inspector

Every editing page is the same three parts:

```
┌ page bar: title · what's selected            saved ✓  ↶ ↷  actions ┐
├──────────────────────────────────────────────┬───────────────────────┤
│                                              │ Inspector             │
│     Stage: the real thing, live              │  header (what, reset) │
│     (click to select, drag to move)          │  presets              │
│                                              │  the few common ones  │
│     stage tools: display switcher            │  ▸ More options       │
└──────────────────────────────────────────────┴───────────────────────┘
```

- **Stage.** A monitor-shaped frame showing the actual desktop at the actual geometry: the
  theme's picture, the widgets at their cells, the bars where they sit. Hover outlines what can be
  selected; click selects; Esc or a click on empty space goes back to the page's default
  selection. Widgets drag on the real grid and snap to cells; a drop that would overlap is
  refused. Below 1100 px the inspector moves under the stage. The stage never scrolls or zooms;
  the Bars page draws a narrower screen instead when the real one would make the bars too small
  to read (section 4).
- **Inspector.** Header (icon, name, one line of context, Reset), then **presets** as visual
  tiles, then the **common few** settings, then **More options** (a disclosure, closed by
  default). A setting that doesn't apply in the current mode is not rendered. No paragraphs: a
  label, a control, at most one short line under it.
- **Page bar.** Title, the selection, the save state (Saving… / Saved), **Undo / Redo** (also
  Ctrl Z / Ctrl Shift Z outside text fields), and the page's main action.

### Few patterns

| Pattern | Used for | Component |
|---|---|---|
| **Inspector property** | every setting | `Prop` (label, control, "From theme"/"Default" hint, reset on hover) |
| **Picker** | themes, presets, widget styles, surfaces, fits, corners, pointers, materials | `TilePicker` (visual tiles, radio semantics) |
| **List** | desktop menu, pinned apps, parts of mypaper | `SortableList` (drag handle, icon, title, trailing actions) |
| **Disclosure** | "More options" | `More` (native `<details>`, so search can open it) |
| **Toggle row** | on/off settings on Settings | `ToggleRow` |

### Instant feedback, safely

- Every change is written as you make it (the programs watch their files and apply it live).
- Every change is undoable: the history is app-wide, slider drags merge into one step, and undo
  writes the previous value back.
- Applying a theme toasts "Applied Aurora · Undo".
- Reset in the inspector header returns the selection to its preset (or to the theme / defaults).
- Destructive actions (remove a widget) are undoable rather than confirmed.

### Plain words

- Say what it does, not how: "Pause when a game is fullscreen", not "suspend the renderer on
  fullscreen foreground".
- No file names, registry paths, process names or CSS selectors in labels. They may appear in an
  error, or under **More options → Details**.
- No keyboard instructions in copy: keyboard support is there, announced to screen readers.
- No "Don't change": the neutral choice is "As is" (leave Windows' own setting alone).

## 4. Live previews: how they work

Every stage draws the real renderers where there is one in a page, on your wallpaper, at the
host's geometry.

| Layer | Renderer | Data |
|---|---|---|
| Wallpaper | the theme's preview picture (shader, scene, video and web themes are native in the engine; their `preview.jpg` stands in) | `list_themes` |
| Widgets | **the real widget pages**, each in an iframe at its real size, the whole stage scaled | Studio acts as the widget shell (`lib/widgetHost.ts`), see below |
| Bars | **the real bars page** (`bars/shell`), one frame per bar | the `bars.json` being edited, posted to the page on every change |
| Windows | a sample window, menu and pointer drawn with the theme's values | `get_style`, `config.desktop_menu` |

**Widgets.** Widget pages are files on disk; Studio serves them through Tauri's asset protocol
with a folder URL (so relative links resolve) and allows each widget folder in the asset scope
when it lists widgets. The frames are `sandbox="allow-scripts allow-same-origin"`: the asset
protocol only answers Studio's own origin, so an opaque sandbox origin couldn't load the widget's
module scripts. Studio answers the widget SDK's wire protocol (`docs/widgets.md`):

- `hello`: the instance, its settings (with the manifest's defaults), its size in cells and
  pixels, and the look as the host sends it: the fields the layers set, with the tone-aware tint
  and accent (`sentLook`). `plate: false`: nothing draws liquid glass in a preview, so the kit
  draws its own stand-in. Changes are pushed as `settings`, `size` and `appearance` events.
- Sample data for `system.stats`, `media.current`, `theme.current`, the audio spectrum and
  subscriptions; storage in memory.
- `plugin` calls go to the widget's own plugin through the Rust `plugin_call`, with the host's
  rules (the manifest must list it). `launch`, `pickFile`, `appIcon`, `settings.set` and `plate`
  answer without doing anything.

Geometry is the host's: monitor size, DPI scale and work area from `list_monitors`, the cell size
from `widgets.json` (`cell: 0` reads the desktop icon spacing, like the host), and, while the
bars aren't running, the space they will reserve.

**Bars.** `pnpm -C bars/shell build` builds the bars page into `bars/shell/dist`; Studio's
`vite.config.ts` copies it into `dist/bars-shell` (and serves it in dev), with a small
`studio-embed.js` that hides `window.chrome`, so the page uses its own sample host instead of the
real one. Without that build Studio draws a silhouette of the bars instead (`BarsSilhouette`).
`components/bars/BarFrame.tsx` then:

1. loads `bars-shell/index.html?bar=top|dock&embed` (plus the preset and designs, so the first
   draw is close), same-origin;
2. waits for the first `[data-band]`, then injects a style: the wallpaper (as a data URL, the page
   only loads its own files) behind the glass and as `--backdrop`, and no pointer events;
3. posts `{ type: "mypaper-bars", bars }` with the document being edited, on every change, so the
   page resolves it exactly like the host (design, pairing, the file);
4. reads the layout from the page's DOM (`[data-band]` surfaces, `[data-zone]` and
   `.tb-start`/`.tb-end` lists, `[data-module]` modules) a few times as the bar settles and on
   every mutation, for picking, outlines, drag targets and the drop caret.

On a stage each frame is the size of the screen, clipped to its bars with room for shadows, and
faded over that room at any edge inside the screen (a hard edge on a scaled stage draws a dark
one-pixel seam). Studio does all picking and dragging over the frames. The design thumbnails are
the same frame per design, 1000 DIPs wide (so a design's modules have the room they'd have on a
screen), scaled into the tile and mounted once scrolled into view.

**The Bars stage** (`BarsScene`) draws only the wallpaper and the bars, never the widgets. Where
the real screen would draw the bars smaller than 0.66 (a 1280 px window), it draws a narrower
screen instead (`Stage` `readable`): the bars keep their real size, only the room between their
modules shrinks. It is at least 1000 DIPs wide and 800 tall (or the real screen, if that's
smaller), so a dock on a side edge still fits under the top bar. Nothing zooms or scrolls.

## 5. Where everything lives

| Old place | New place |
|---|---|
| Gallery hero ("On your desktop now", Pause, Customize) | Desktop page: the stage, Wallpaper inspector |
| Gallery grid, filter | Themes |
| Customize → Windows / Widgets look / Bars look / Wallpaper | Themes → Edit theme (same inspectors); the theme on screen: Windows page |
| Customize on a built-in theme (lock screen) | "Make a copy to edit" button in the inspector, which copies, applies, and keeps you in place |
| New theme (page) | Themes → New theme |
| Widgets → Installed | Widgets → Add widget catalog |
| Widgets → On the desktop | Widgets stage (each widget, live); inspector: options, own look, remove |
| Widgets → Look | Widgets, nothing selected: look of all widgets |
| Widgets → overlay toggle | Widgets, nothing selected → Always on top |
| Edit layout on desktop | Widgets page bar ("Arrange on desktop") |
| Settings → Playback | Settings → Power; Pause on the Desktop page bar and the sidebar |
| Settings → Startup | Settings → Startup |
| Settings → Engine, Components, panic hotkey | Settings → mypaper (parts one step away) |
| Settings → Desktop extras (menu, classic menu) | Windows → Right-click menu |
| Bars (tabs: Presets, Top bar, Bottom bar, Menus, Icons, Windows taskbar) | Bars: the stage and one inspector by selection (section 6) |

The full checklist, setting by setting, is in section 7.

## 6. Bars (outcome)

The page is the live bars on your wallpaper and an inspector that follows the selection.

- **Stage.** Both bars, live (section 4), framed and readable at any window size. Hover outlines a
  bar or a module; click picks it; a click on the wallpaper goes back to both bars. **Drag a
  module** to move it within its list, to another list or the other bar (a caret shows where it
  lands), or off the bars to remove it. Every move is one undo step. Name tags sit where there's
  room (beside a bar on a side edge).
- **Nothing picked: Bars.** The two bars with an on/off switch each (click one to open it), the
  **pairings** (Monochrome, macOS, Windows 11…: each sets both designs and which bars are on), and
  the **Windows taskbar** (Keep / Auto-hide / Hide, and its icons' alignment unless hidden). One
  step away: Menus and panels, Icon packs, Look for both bars, Motion and power, and Per display
  (only when the file has per-display settings).
- **A bar.** Show it; its **design** from live thumbnails; the common few (top bar: edge, height;
  bottom bar: style, edge, icon size, auto-hide, magnify); light or dark and glass; its **module
  lists** (Start, Middle, End: reorder, remove, open, Add); the bottom bar's **pinned apps**.
  Under More: the rest of its layout, its look and its motion. A reset arrow in the header returns
  its look and motion to the design's.
- **A module.** What a click does, the other clicks and scrolling, its icon and look; for one of
  your own, its program, arguments and how often it runs. Remove it from the bar, or delete one of
  your own everywhere.
- **Add a module.** Which list, every module the bar doesn't show yet, a gap, or a program of your
  own (a name and the program; the rest once it's added).
- **Hidden, not explained.** A floating dock has no module lists and no Add; magnify's strength
  only shows with magnify on a floating dock; window titles only on a horizontal taskbar; corner
  settings not on a square strip; glass knobs not with no glass; running-indicator colors only on
  the bottom bar; the tray switch not while the Windows taskbar is hidden (it's on then anyway);
  duration not for a spring curve (a spring sets its own).

Where it differs from the phase-2 plan: motion is per bar (Animations, Appearing, then each
movement with its own timing) rather than Calm/Snappy/Bouncy presets; the flyouts (Start menu,
sound, calendar, tray) and icon packs are under "Bars" (they're shared by both bars) rather than
in a module's or a bar's inspector; pinned apps reorder in the inspector's list, not by dragging
icons on the stage. The embed hook was built by the bars agent as the page's own sample host
(`?embed`) plus the `mypaper-bars` message, so Studio doesn't answer the page's protocol.

## 7. Settings coverage

Every setting Studio could change before the redesign (the Studio of `c88bac1`; for Bars, the
tabbed v3 page that replaced it, `700bfc0`), and where it is now. Search (Ctrl K) finds the ones
marked with an id in `apps/studio/src/lib/settingsIndex.ts`; the rest are one click from those.

### Themes and wallpaper

| Old | Now |
|---|---|
| Gallery: apply a theme | Themes: click a card (toast with Undo); Desktop: the theme strip under the stage |
| Gallery: pause / resume | Desktop page bar: Pause; the sidebar's Playing chip; Ctrl K |
| Gallery: reset config.json when it can't be read | Themes: the banner's "Reset to defaults" |
| Customize on a built-in theme: Duplicate | Make a copy to edit (in place) |
| Customize → Wallpaper: interactive (web and 3D scene themes) | Desktop → the wallpaper → Mouse → React to the mouse (same themes) |
| Customize → Widgets look / Bars look: every look field | Themes → Edit theme → Widgets / Bars (the same look fields) |
| Customize → Widgets look / Bars look: style sheet, remove it | Themes → Edit theme → Widgets / Bars → Style sheet |
| (new) liquid glass knobs for widgets | Themes → Edit theme → Widgets → Style: Liquid → Strength, Bend, Shine, Tint, Blur |
| New theme: source (file, folder, web page, program) | Themes → New theme |
| New theme: name, fit, a program's arguments | Themes → New theme |
| New theme: apply it right away | Themes → New theme → "Use it right away" |

### Windows (was Customize → Windows)

| Old | Now |
|---|---|
| Accent color | Windows → Accent color |
| Dark mode | Windows → Light or dark |
| Corners | Windows → Corners |
| Backdrop | Windows → Material |
| Window border | Windows → More options → Window edge |
| Title bar, title bar text | Windows → More options → Title bar, Title text |
| Cursors, cursor set | Windows → Pointer (As is, the sets, Other) |

### Widgets

| Old | Now |
|---|---|
| Installed: list, search | Widgets → Add widget (catalog with search) |
| On the desktop: list | Widgets stage, and "On this display" in the inspector |
| On the desktop: reset a widget's own look | a widget → "Its own look" off |
| On the desktop: remove | a widget → trash (undoable) |
| On the desktop: not installed | the stage draws a placeholder; its inspector says so and offers remove |
| Look: style, surface, tint, opacity, blur, corner radius | Widgets, nothing selected → Style, Surface, Color, Opacity, Blur, Corners |
| Look: text, accent, font, scale, border, text shadow | Widgets, nothing selected → More options |
| Overlay widgets: show them | Widgets, nothing selected → Always on top |
| Edit layout on the desktop | Widgets page bar → Arrange on desktop (also Desktop → a widget) |
| (new) a widget's own options and own look | Widgets or Desktop → a widget |
| (new) place, size, duplicate | drag and resize on the stage; a widget → Size; the header's duplicate |

### Settings

| Old | Now |
|---|---|
| Start with Windows | Settings → Startup |
| How it starts (shown) | Settings → mypaper → Parts of mypaper |
| Show the theme's still at sign-in | Settings → Startup → Show the theme while Windows starts |
| Pause on battery, pause while gaming / fullscreen | Settings → Power |
| Pause wallpaper | Pause (Desktop page bar, sidebar) |
| Engine: start, stop, status | Settings → mypaper |
| Components | Settings → mypaper → Parts of mypaper |
| Panic hotkey (shown) | Settings → mypaper → Emergency stop |
| Desktop right-click menu: on/off | Windows → Right-click menu |
| Menu items: reorder, hide, add built-in, add your own (label, command, arguments) | Windows → Right-click menu |
| Classic Windows 11 context menu | Windows → Right-click menu → Full menus in Explorer |

### Bars

| Old (tab → setting) | Now |
|---|---|
| Presets → apply a preset | Bars → Pairings |
| Presets → the conflict dialog (clear settings that hide the preset's) | gone: v3 pairings only set the designs, which bars are on and their displays; choosing one clears your own design and on/off choices, undoable |
| Top bar / Bottom bar → Design | a bar → Design (live thumbnails) |
| Top bar → Layout: edge, height | a bar → Layout |
| Top bar → Layout: shape, width (floating), margin, padding, spacing | the top bar → More layout → Shape and room |
| Top bar → Layout: clock seconds and format | the top bar → More layout → Clock |
| Top bar / Bottom bar → Layout: displays | a bar → More layout → Displays |
| Top bar → on/off | Bars → the Top bar switch; the top bar → Show the top bar |
| Bottom bar → Layout: style, edge, icon size, auto-hide, magnify | the bottom bar → Layout |
| Bottom bar → Layout: margin, padding, spacing, magnify scale and reach | the bottom bar → More layout → Room |
| Bottom bar → Layout: apps alignment, window titles, the taskbar's clock | the bottom bar → More layout → Taskbar (taskbar style) |
| Bottom bar → Layout: running apps, one per window, count badge, previews and their delay, bounce, attention, running indicator, after the apps | the bottom bar → More layout → Apps |
| Bottom bar → Layout: pinned apps (Windows' or your own) | the bottom bar → Pinned apps |
| Bottom bar → on/off | Bars → the Bottom bar switch; the bottom bar → Show the bottom bar |
| Modules → the lists (chip boxes) | the stage (drag) and a bar → Modules (reorder, remove) |
| Modules → add a built-in, a break | a bar → Modules → Add (or drag one off to remove it) |
| Modules → your own module (name, command) | a bar → Add a module → Your own module |
| Modules → a module's clicks, scrolling, icon, text color, background, padding, radius, text size and weight | a module (stage or list) → its inspector |
| Modules → a custom module's command, arguments, interval; delete it | a custom module → What it shows; Delete this module everywhere |
| Look → light or dark, glass | a bar → Look |
| Look → background (and its color, gradient, image, wallpaper-glass fields), tint, saturation, grain, corner radius and shape | a bar → More look → Surface |
| Look → glass intensity, refraction, highlight, tint, blur, depth, sheen | a bar → More look → Glass |
| Look → glass menus, drawn like, icons | a bar → More look → Menus and panels |
| Look → border, glow, shadow per state | a bar → More look → Edges and depth |
| Look → module and app buttons per state | a bar → More look → Buttons |
| Look → running-indicator colors | the bottom bar → More look → Running indicators |
| Look → theme variables | Bars → Look for both bars |
| Look → base appearance | Bars → Look for both bars |
| Motion → on/off, duration, curve, style (the bar's show animation) | a bar → Motion → Animations, Appearing |
| Motion → hover, press, open; magnify, launch bounce, attention | a bar → Motion → Movements |
| Motion → reduce motion, performance mode | Bars → Motion and power → Reduce motion, Save power |
| Menus → Start menu layout, search recent files, Windows key, its pinned apps | Bars → Menus and panels → Start menu |
| Menus → let the mixer switch the output device | Bars → Menus and panels → Sound |
| Menus → calendar's extra clocks, first day of the week | Bars → Menus and panels → Calendar |
| Menus → be the tray host | Bars → Menus and panels → Tray (not shown while the Windows taskbar is hidden) |
| Menus → show the quick settings, network, volume, notifications and language modules | a bar → Modules → Add (they're modules) |
| Icons → the pack, start your own, single apps' icons | Bars → Icon packs |
| Windows taskbar → keep, auto-hide, hide; icon alignment | Bars → Windows taskbar |
| (new) per-display settings: remove one | Bars → Per display (when the file has any) |

### Lost or changed

Nothing a bar, widget, theme or window can show lost its control. What changed:

1. **Motion at nested nodes of a bar's own tree.** The old Motion tab could also write fields at
   `bar`, `bar.top` and `bar.dock` inside a bar's `animation` (and their hover, press and open).
   For one bar those apply exactly like the root, which is what Studio edits now (the dock-only
   movements stay at `bar.dock`, where the host reads them). Values a file already has there still
   apply and show as inherited; Studio doesn't write them.
2. **The look and motion shared by both bars.** Before the bars' v3 schema, Studio edited the
   shared `look` and `animation` (both bars at once). Since v3, and on the tabbed v3 page too, each
   bar has its own; a change for both bars is made on each. The shared layer still applies under
   each bar's own and shows as inherited. Shared values Studio still edits: the base appearance and
   theme variables (Look for both bars) and Reduce motion.
3. **The preset-conflict dialog** is gone with the presets it served (see the Bars table).
4. **Widget permissions** (information, not a setting): the old Installed list showed what each
   widget asks for (`launch`, `network`…); the catalog doesn't.
5. **Explanations of internals** were dropped on purpose (section 3, "Plain words"): the Windows
   key hook's mechanics, the tray host's, the timer of the notifications count, file and folder
   names in labels. The behavior is unchanged.

## Rules for new settings

1. Put it where the thing is on screen. If it changes something visible, the stage must show it.
2. Common few up front, the rest under More options. If it only applies in one mode, render it
   only in that mode.
3. One label, one control, at most one short line. No internals, no file names.
4. Use the existing patterns (`Prop`, `TilePicker`, `SortableList`, `More`), never a new one-off.
5. Add it to `settingsIndex.ts` with its `data-setting` id, so search finds it.
6. Writes go through the history (`useHistory().record`), so it can be undone.
