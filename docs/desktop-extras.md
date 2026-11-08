# Desktop extras: right-click menu, cursors, classic menu, interactive wallpapers

Explorer's own UI (its menus, its title bars' contents, the taskbar's XAML) can only be restyled by
code that runs *inside* Explorer. Glaze does not do that. This document is what can be done
**outside** it, with supported or well-known APIs, always reversible (the right-click menu is on
by default; everything else is opt-in):

| Feature | Switch | API status |
|---|---|---|
| [Our own desktop right-click menu](#desktop-right-click-menu) (on by default) | `desktop_menu.enabled` in `config.json` | documented (`SetWindowsHookEx`, UI Automation, `IFolderView2`) |
| [Cursor themes](#cursor-themes) | `windows.cursors` in `theme.json` | documented (`SPI_SETCURSORS`, the Mouse control panel's registry values) |
| [Win11 classic context menu](#win11-classic-context-menu) | `classic_context_menu` in `config.json` | **undocumented** registry key, widely used |
| [Interactive wallpapers](#interactive-wallpapers) | `wallpaper.interactive` in `theme.json` | documented (Raw Input) |
| [Raised-desktop compatibility](#raised-desktop-24h2) | none | `Progman` window styles and z-order |
| [Desktop icons and icon size in the tray](#tray) | tray, **Desktop** submenu | documented (`IFolderView2`) |

Nothing here injects code into another process. The one hook (the right-click menu's) lives in
a thread of `glaze-shell`, the native shell core supervises.

## Desktop right-click menu

```json
{
  "desktop_menu": {
    "enabled": true,
    "items": ["view", "sort", "refresh", "separator", "new", "paste", "separator",
              "wallpaper", "widgets", "display", "personalize", "separator", "terminal",
              { "label": "Notes", "command": "notepad.exe", "args": ["D:\\notes.txt"] },
              "more"]
  }
}
```

A right-click on the **bare desktop** opens Glaze's menu (a themed window drawn by the native UI
layer, [ui.md](ui.md)) instead of Explorer's. A right-click on an **icon**, on any window, on the taskbar, or with **Shift** held,
always gets the native menu. **On by default** (no `desktop_menu` key, or one without `enabled`);
with `"enabled": false` nothing is installed: no hook, no thread. The Studio toggle and the tray's
**Desktop** submenu write that.

**Why it's on.** Measured on the test machine (build 26300, two 1440p monitors, injected right
clicks on the bare desktop, a poll every millisecond for the menu's window), the native Windows 11
desktop menu takes **130 to 190 ms for its first window and 205 to 320 ms for the full menu with
Glaze not running at all**, so the slowness is Windows' own (its XAML menu and the shell
extensions), not ours; with Glaze running (menu off) it's the same with a web wallpaper (about
230 ms) and 300 to 510 ms in the first seconds after start with an image wallpaper (six clicks;
a hint of load from the wallpaper's start, not a steady cost). Our menu opened in **17 to 50 ms**
(median 25) on the same clicks when core hosted it, and in **11 to 15 ms** (median 13) from the
shell ([below](#measured)).

`items` is the menu top to bottom. Leave an item out to hide it, reorder to move it. Without `items`
you get the list above (minus the custom entry). A mistyped item name is an error, like any other
config typo. Stray separators (leading, trailing, doubled) are dropped, so hiding items never leaves a
gap.

| Item | Does |
|---|---|
| `view` | Large / medium / small icons, **Show desktop icons** (`IFolderView2::SetViewModeAndIconSize`, `SetCurrentFolderFlags(FWF_NOICONS)`); check marks show the current state |
| `sort` | Name / size / item type / date modified, ascending / descending (`IFolderView2::SetSortColumns`) |
| `refresh` | `IShellView::Refresh` |
| `new` | Folder, text document: created in the Desktop folder, announced to the shell (`SHChangeNotify`) and put into rename mode (`IShellView::SelectItem(SVSI_EDIT)`), like Explorer's "New" |
| `paste` | The `paste` verb of the desktop's own background `IContextMenu`, so copy/move semantics and conflict prompts are Explorer's. Greyed out unless the clipboard holds files |
| `wallpaper` | Opens Studio |
| `widgets` | Toggles widget edit mode |
| `display`, `personalize` | `ms-settings:display`, `ms-settings:personalization` |
| `terminal` | Windows Terminal in the Desktop folder (`wt.exe -d`), else a console |
| `more` | **Show more options**: the native Windows menu |
| `separator` | A line |
| `{ "label", "command", "args" }` | Runs the command (no shell, no quoting rules). A document or URI that cannot be started directly goes to `ShellExecute` |

The desktop's view comes from the documented route `IShellWindows::FindWindowSW(CSIDL_DESKTOP)` →
`IShellBrowser` → `IShellView` → `IFolderView2`. Navigation: arrows, Enter, Esc, or the mouse.

**Look.** The menu uses the same appearance variables as the bars: the built-in defaults, under the
active theme's `bars.appearance` (`tint`, `opacity`, `radius`, `border`, `text`, `accent` (`auto`
follows the wallpaper), `font`, `scale`, `shadow`), turned into the UI layer's tokens. Differences:
`blur` has no counterpart yet (the engine's Liquid Glass comes to menus in N3), so the panel is drawn
as a nearly opaque tint (`opacity` is kept between 0.7 and 1), and `surface: none` still draws a
panel. Check marks and submenu chevrons are Segoe Fluent Icons glyphs (Segoe MDL2 Assets on
Windows 10).

### How it works

Both threads are `glaze-shell`'s (`crates/shell/src/desktop_menu/`); core only supervises it.

```text
hook thread                              the shell's UI thread
 WH_MOUSE_LL, a message loop, nothing else    COM, UI Automation, the menu windows
  │                                            │
  │  right press on the bare desktop?          │
  │   = WindowFromPoint class check            │
  │     + not in the cached icon rectangles    │
  │   yes: swallow it, post "request" ───────► confirm with UI Automation (ElementFromPoint:
  │                                            │  a ListItem is an icon); read the desktop's
  │  release of a swallowed press:             │  state for the check marks
  │   swallow it, post "release" ────────────► bare: show the menu (a pre-created window)
  │                                            │  icon (the cache was stale): replay the click
  ▼                                            ▼  to the native desktop
```

- **The callback returns in microseconds.** For mouse moves, buttons other than the right one, and
  the wheel it does nothing but pass the event on. For the right button it asks a pure decision
  (`desktop_hit::Swallower`: a window-class check, the cached icon rectangles, Shift) and posts a
  message. All slow work (COM calls to Explorer, UI Automation) happens on the shell's UI thread,
  *while the button is still down*, so the release only has to draw. Measured numbers are
  [below](#measured).
- **Icons always get the native menu.** The fast path knows where the icons are from the folder
  view (`IFolderView2::GetItemPosition` plus the view's spacing; refreshed after clicks on the
  desktop, menu commands and when the hook turns on). The rectangles agree with UI Automation's: all
  86 desktop items of the test machine lie inside their cached cells. If the cache is stale (an icon
  moved), UI Automation catches it and the swallowed right-click is **replayed** to the desktop
  (`SendInput`, marked with our own `dwExtraInfo` so the hook lets it through once).
- **Show more options** replays the right-click at the original position, the same way.
- **Off when it should be.** The hook is *removed* (not just ignored) while a fullscreen app is in
  front (WinEvent-driven, the same detection as the wallpaper's pause), the session is locked or the
  display is off.
- **Focus.** The menu is shown without activating (so it paints at once), then activated, so keys
  work and clicking elsewhere closes it. Submenus never take the focus. Because the opening click
  never reached any window, Windows sometimes refuses a plain `SetForegroundWindow` (seen live); the
  menu then attaches its input queue to the foreground window's thread for the call, which works.
  Clicking anywhere outside the menu closes it even if focus could not be taken (the hook reports it).
- **Submenus touch their parent.** A submenu opens against the parent's right edge (or its left
  edge, without room) with no gap and no overlap, so no popup ever covers another (the engine's
  glass will need that, plan N3). Over a submenu's shadow, the pointer still drives the parent.
- **No lost clicks.** The hook posts to the shell's *inbox window*, which only queues the message.
  (A thread message would be discarded if it arrived while the thread was inside a COM call to
  Explorer, losing the release of a swallowed right-click.) The menu's own windows queue their
  input the same way ([ui.md](ui.md), "Input"), and the loop handles both queues in order.
- **Pre-created windows.** Creating a layered popup takes about 20 ms on the test machine, more than
  everything else together, so the root and one submenu window are created hidden at start and
  reused. A closed menu's windows keep no bitmaps.

### Measured

Release build, Ryzen laptop, two 1440p monitors. The hook callback was timed inside it with
`QueryPerformanceCounter` while the menu lived in core; the callback moved to the shell unchanged,
and the timing instrumentation was removed with the move. "Own" is the callback's work; the time
other programs' hooks (installed after ours) take when we pass the event on is not ours to control.

| Event | Own cost, mean | Own cost, worst |
|---|---|---|
| mouse move (3000-4000 events per run) | **0.06 µs** | 0.2 to 1.5 µs |
| right button press or release (30 events: 15 clicks) | 118 to 145 µs; about 240 to 290 µs per press, under 1 µs per release | 0.24 to 0.62 ms |
| left, middle, wheel | the same as a move (a pass-through) | |

The press is the only event that does real work: `WindowFromPoint` asks the window under the cursor
to hit-test (a cross-process call), which is most of its cost. It happens once per right-click.

The menu in `glaze-shell`, 2026-11-04, injected from another process like hardware input
(`cargo run --release -p glaze-shell --example menu_live`, with the shell running):

| Menu | |
|---|---|
| on screen after a quick click (press and release together: UI Automation's check and the desktop's state are on the critical path) | median **13.1 ms**, best 11.3, worst 14.5 (14 of 15 clicks) |
| on screen after the release of a person's click (70 ms hold) | median **1.8 ms**, worst 21.4 ms |
| Escape closes it (it has the focus) | 30 of 30 |
| UI Automation confirmation (during the press; earlier measurement) | 4.6 to 5.4 ms, the first one 20 ms |

What the shell costs is in [ui.md](ui.md#measured).

### When the native menu opens instead of ours

All by design or by the limits of a hook that must stay cheap; in each, the next right-click is ours:
- On an **icon** (its whole cell, label included), on any window, with **Shift**, and on a widget.
- **While a fullscreen app is in front** (an exclusive-fullscreen game, presentation mode, or a
  window that covers its whole monitor and has no title bar: borderless games, F11 video, a
  screenshot overlay) the hook is *removed*, on every monitor. The hook comes back when the
  foreground changes, which a right-click on the desktop itself causes, but the click that caused it
  has already gone by: so the first right-click on the desktop of a second monitor, with a fullscreen
  window still in front on the first, gets the native menu (reproduced live with Windows'
  Snipping Tool overlay in front). A maximized window *with* a title bar never counts.
- The cached icon rectangles were stale (an icon moved): UI Automation notices and the click is
  replayed to the desktop, after UI Automation's answer (5 to 20 ms).
- The session is locked or the display is off.

### Limits

- Only the **bare desktop** is handled. Right-clicking a desktop *widget* is the widget's business.
- A hung window under the cursor can slow the press decision (`WindowFromPoint`); the hook never
  waits longer than Windows' own hook timeout.
- *Paste* is enabled for files only (`CF_HDROP`); Explorer can paste more.
- Menu items are English.
- A cursor that is over the desktop while another app has a modal dialog still gets our menu.
- Security tools may flag any low-level mouse hook. Turn the option off if yours does.

## Cursor themes

```json
{ "name": "Midnight", "wallpaper": { ... }, "windows": { "accent": "auto", "cursors": "glaze-dark" } }
```

`windows.cursors` takes:

- `glaze-dark` or `glaze-light`: the two built-in sets, drawn **in code** (no third-party
  assets): modern monochrome pointers (a dark one with a white edge, a light one with a dark edge),
  17 cursors each (arrow, help, working in background, busy, precision, text, handwriting,
  unavailable, the four resize arrows, move, alternate, link, person, pin), crisp at 100% to 200%
  (32, 48 and 64 px images, anti-aliased from distance fields), the two busy cursors animated
  (`.ani`, a 12-dot spinner). They are written to `%LOCALAPPDATA%\glaze\cursors\<name>` the first time
  they are used. `cargo run -p glaze-tweaks --example export_cursors -- <folder>` writes them
  wherever you like, to look at or to start your own set from.
- a **set name**: a folder under `%APPDATA%\glaze\cursors`, or under the theme folder;
- a **folder path**: absolute, or relative to the theme folder (`"cursors/mine"`).

A set folder holds `.cur` / `.ani` files named after the cursor they replace, case does not matter:
`arrow`, `help`, `appstarting` (or `working`), `wait` (or `busy`), `crosshair` (`cross`),
`ibeam` (`text`), `nwpen` (`pen`), `no` (`unavailable`), `sizens` (`vert`), `sizewe` (`horz`),
`sizenwse` (`dgn1`), `sizenesw` (`dgn2`), `sizeall` (`move`), `uparrow` (`alternate`), `hand`
(`link`), `person`, `pin`. A cursor the set lacks keeps the user's own.

**How.** The values of `HKCU\Control Panel\Cursors` (what the Mouse control panel writes: one path per
cursor as `REG_EXPAND_SZ`, the scheme name as the default value, `Scheme Source` = 1) are written,
then `SystemParametersInfoW(SPI_SETCURSORS)` reloads them. They go through the same undo journal as
the accent (`%LOCALAPPDATA%\glaze\undo.json`): each original value, with its exact registry type, is
saved first, and restored on theme change, quit, the panic hotkey and after a crash. The journal
also records which registry keys it had to *create*, and removes those again.

## Win11 classic context menu

```json
{ "classic_context_menu": true }
```

Windows 11 shows a short context menu with "Show more options" for the full one. An empty default
value at
`HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32` makes Explorer use
the full classic menu everywhere. This is **undocumented**: the CLSID is the one the new menu's
handler registers, and the trick is widely used (Nilesoft Shell and others), but Microsoft does not
promise it. It is also a global, user-visible change, so it is strictly opt-in. It is in the tray
(**Desktop**, **Classic Windows 11 menu**) and in `config.json`.

- **Journaled** in its own file, `%LOCALAPPDATA%\glaze\undo-shell.json`. Unlike the styling
  journal it **outlives the run**: the menu stays classic until you turn the option off, and turning
  it off removes exactly what was added, including the registry keys that were created. A key you had
  added by hand is left alone.
- **Explorer reads the value when it starts.** After a change Glaze shows a notice: restart
  Explorer (Task Manager, Windows Explorer, Restart) or sign out and back in. **Glaze never
  restarts Explorer.**
- By hand: `reg delete "HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}" /f`.

## Interactive wallpapers

```json
{ "wallpaper": { "kind": "web", "url": "index.html", "interactive": true } }
{ "wallpaper": { "kind": "scene", "source": "islands.glb", "interactive": true } }
```

The wallpaper sits *behind* the icons, so Windows never gives it the mouse. With `interactive`, the
engine listens through **Raw Input** (`RegisterRawInputDevices` with `RIDEV_INPUTSINK` on a hidden
message window, what Lively does): no hook, nothing swallowed, the desktop behaves as before.
Off by default; when off, nothing is registered and nothing runs.

- **Only the bare desktop.** A move is forwarded when the pointer is over the desktop layer (the same
  class check as the menu: icons or empty space, not an app, the taskbar or a widget); a *press* also
  has to be off the icons (the folder view's rectangles). A drag that started on the bare desktop
  keeps going wherever the pointer is, and its release always arrives (so the page never sees a
  stuck button). Leaving the desktop sends one "leave".
- **Coalesced to the frame rate.** Moves are queued and delivered at most every 16 ms (a timer that
  exists only while moves are pending); presses and the wheel are delivered at once, after any moves
  that came before them, so order is kept. A burst of 1000 moves becomes one.
- **Zero cost when held.** While every screen is held or covered (a fullscreen game), Raw Input is
  switched off again. With `interactive` off it is never on.
- **`web`**: moves and buttons are posted to the page's `Chrome_RenderWidgetHostHWND` (client
  coordinates) and arrive as real `mousemove`, `mousedown`, `mouseup` events. Chromium sends a
  wheel message to the window under the *real* pointer, so a posted one never reaches the page; the
  wheel is delivered as a `WheelEvent` dispatched by a one-line script (`isTrusted` is false for it).
  Windows also tracks the real pointer, which is over the desktop and not the page, so the page may
  see a `mouseleave` right after a move: do not hide things on it (`themes/interactive` ignores it).
  Right-clicks on the bare desktop reach the page as well (Raw Input cannot swallow them).
  Verified live on the test machine: moves, a click and three wheel notches arrived in
  `themes/interactive`, and a click on the menu or an icon did not.
- **`scene`**: dragging with the left button orbits the camera (and tilts it up and down; verified
  live on `islands`: a 180-pixel drag turned the view clearly); the
  parallax lean toward the pointer needs no input and works as before. An interactive scene renders
  at its fps cap even when `orbit` and `parallax` are 0, so it can react.

## Raised desktop (24H2+)

On recent Windows 11 builds Explorer composes the desktop itself: `Progman` has
`WS_EX_NOREDIRECTIONBITMAP`, and `SHELLDLL_DefView` (the icons) is a *layered child* of `Progman`
with the static wallpaper's `WorkerW` below it. Glaze already placed its windows between the two;
`DesktopHost` now also:

- **detects** it (`DesktopHost::raised()`; the engine logs the host at start);
- **verifies** the z-order after placing a window (`attached_correctly`): below the icon view, above
  every static-wallpaper `WorkerW`, and puts the window back with `keep_behind_icons` if Explorer
  moved it (call it from window events);
- **waits** up to 1.5 s for a *running* Explorer to have built its desktop (logon, right after an
  Explorer restart), where it used to fail at once. With no shell window at all it still fails at once.

Checked on the test machine (build 26300, raised): Progman's children, top to bottom, are the widget
surface, `SHELLDLL_DefView` (layered), the wallpaper windows, the static `WorkerW`.

## Tray

**Desktop** submenu: **Show desktop icons**, **Large / Medium / Small icons** (the check shows the
current size), **Glaze right-click menu**, **Classic Windows 11 menu**. The first four act on
Explorer's own desktop view (the same as Explorer's View menu); the last two write `config.json`.
Note: like Explorer's View menu, changing the icon size makes Explorer re-save its icon layout.

## Files

| What | Where |
|---|---|
| Menu config | `config.json` → `desktop_menu`, `classic_context_menu` |
| Cursor sets | `%LOCALAPPDATA%\glaze\cursors\<built-in>`, `%APPDATA%\glaze\cursors\<yours>` |
| Styling journal (accent, dark mode, cursors) | `%LOCALAPPDATA%\glaze\undo.json` |
| Classic menu journal | `%LOCALAPPDATA%\glaze\undo-shell.json` |
| Code | `crates/shell/src/desktop_menu/` (the menu), `crates/ui` (drawing it, [ui.md](ui.md)), `crates/desktop/src/{desktop_hit,desktop_view,raw_input}.rs`, `crates/tweaks/src/{cursors,cursor_art,classic_menu}.rs`, `crates/core/src/desktop_extras.rs` |
| Live harnesses | `crates/shell/examples/menu_live.rs`, `crates/desktop/examples/{view_live,icons_live}.rs`, `crates/tweaks/examples/live_tweaks.rs` |

Studio edits all of these: Settings has the desktop right-click menu (items, order, custom
commands) and the classic-menu switch, and the theme editor has the cursor picker and the
`interactive` switch for web and scene themes. The files stay the interface, so a text editor
works too.
