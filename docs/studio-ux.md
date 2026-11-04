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
| **Bars** | "My top bar and dock." | Live bars: presets, then click the bar or a module to edit it (phase 2, section 6). |
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
│     stage tools: monitor · zoom              │  ▸ More options       │
└──────────────────────────────────────────────┴───────────────────────┘
```

- **Stage.** A monitor-shaped frame showing the actual desktop at the actual geometry: the
  theme's picture, the widgets at their cells, the bars where they sit. Hover outlines what can be
  selected; click selects; Esc or a click on empty space goes back to the page's default
  selection. Widgets drag on the real grid and snap to cells; a drop that would overlap is
  refused. Below 1100 px the inspector moves under the stage.
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

| Layer | Renderer | Data |
|---|---|---|
| Wallpaper | the theme's preview picture (shader, scene and web themes have no in-page renderer; the engine is native) | `list_themes` |
| Widgets | **the real widget pages**, each in a sandboxed iframe at its real size, the whole stage scaled | Studio acts as the widget shell: it answers the SDK's wire protocol (`docs/widgets.md`) with sample data (stats, media, theme), the instance's settings and the merged look. `launch`, file pickers and plugins do nothing in the preview. |
| Bars | phase 1: a silhouette from the resolved `bars.json` (shape, position, height, the modules' icons). Phase 2: the real bars page (see 6) | `get_bars` + `bars_catalog` |
| Windows | a sample window and menu drawn with the theme's values | `get_style`, `config.desktop_menu` |

Widget pages are files on disk; Studio serves them through Tauri's asset protocol with a folder
URL (so relative links resolve) and allows each widget folder in the asset scope when listing
widgets. The page CSP adds `frame-src` for the asset origin. Geometry is the host's: monitor size
and DPI scale and work area come from `list_monitors`, the cell size from `widgets.json` (`cell: 0`
reads the desktop icon spacing, like the host).

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
| Widgets → overlay toggle | Widgets, nothing selected → More options |
| Edit layout on desktop | Widgets page bar ("Arrange on desktop") |
| Settings → Playback | Settings → Power; Pause also on Desktop |
| Settings → Startup | Settings → Startup |
| Settings → Engine, Components, panic hotkey | Settings → Engine (More options: parts, panic hotkey) |
| Settings → Desktop extras (menu, classic menu) | Windows → Right-click menu |
| Bars (9 tabs) | Bars (phase 2 rebuild; phase 1 keeps the tabs inside the new shell) |

The full checklist, setting by setting, is in section 7.

## 6. Bars (phase 2 design)

The bars agent is splitting the config into a **top bar** and a **bottom bar**, each with its own
design, look, motion and modules. The page is built on that schema:

- **Stage:** the desktop with both bars live, rendered by the **real bars page** in iframes (one
  per bar), fed by Studio over `postMessage`.
- **Default inspector (nothing selected): Presets.** Big tiles, each a real render of the preset
  on your wallpaper. Under them: Top bar on/off, Bottom bar on/off, and the Windows taskbar choice
  (Keep / Hide when unused / Hide), in words.
- **Click a bar:** its inspector: Design (strip / floating / islands / dock / taskbar as tiles),
  Position, Size, and More options (margins, spacing, width, monitors, auto-hide). Then Look
  (material tiles incl. Liquid Glass, tint, corners; More: borders, glow, shadow, per-state item
  colors, theme variables) and Motion (presets: Calm / Snappy / Bouncy / Off; More: per-interaction
  curves with a curve picker).
- **Modules, by direct manipulation:** modules are dragged *within the bar on the stage*. Hovering
  the bar shows its three zones (start / middle / end) as faint drop areas; a drag shows a caret
  where it will land. "Add module" opens a picker (icons with names) whose items are dragged onto
  the bar or clicked to append to the selected zone. Dragging a module off the bar removes it
  (undoable). A separator is a module in the picker ("Gap").
- **Click a module:** its inspector: what a click / right click / scroll does (an action picker
  with plain names: "Open Start", "Play or pause"…), its icon and style, and for a custom module
  the command. A dock-style bar shows the pinned apps as a `SortableList`, and dragging icons on
  the stage reorders them too.
- **Hidden, not explained:** a bar design without modules (a floating dock) simply has no module
  zones on the stage and no "Add module"; a design without magnify has no magnify settings.
- **Menus, icons:** the flyouts (Start, calendar, quick settings) are modules: their options open
  from the module's inspector. Icon packs are a picker in the bar's Look.

**Host hook needed (bars agent):** an embed mode in `bars/shell/src/host.ts`: with `?embed`, use
`window.parent` as the transport (`parent.postMessage` out, `message` events from `parent` in)
instead of `chrome.webview` or the mock. Studio then answers `hello` (resolved config, look,
backdrop = the theme's picture) and pushes `config`/`appearance` on every edit, plus sample
`apps`/`clock`/`media`/… events. About 15 lines in the shell; everything else is Studio's.

## 7. Settings coverage

Every setting Studio could change before the redesign, and where it is now. (Bars: phase 2.)

See the table in the implementation report; it is kept in `apps/studio/src/lib/settingsIndex.ts`
as data: every entry there is a searchable setting with its page and section.

## Rules for new settings

1. Put it where the thing is on screen. If it changes something visible, the stage must show it.
2. Common few up front, the rest under More options. If it only applies in one mode, render it
   only in that mode.
3. One label, one control, at most one short line. No internals, no file names.
4. Use the existing patterns (`Prop`, `TilePicker`, `SortableList`, `More`), never a new one-off.
5. Add it to `settingsIndex.ts` with its `data-setting` id, so search finds it.
6. Writes go through the history (`useHistory().record`), so it can be undone.
