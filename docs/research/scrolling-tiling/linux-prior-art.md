# Scrollable / infinite-canvas tiling: Linux & macOS prior art

Research slice for mypaper (native Rust Windows shell). Read on **2026-11-08** from primary sources: source code at pinned commits, official docs/wikis, maintainer comments in issues/discussions. Anything not traced to a primary source is marked **(unverified)**. Third-party press is used only for "who ships it" facts, and is labelled.

## Sources read (pinned)

| Project | Where | Commit / date read |
|---|---|---|
| niri | `github.com/niri-wm/niri` (YaLTeR/niri redirects there) — `src/layout/*.rs`, `src/handlers/*.rs`, `src/window/mod.rs`, `src/niri.rs`, `docs/wiki/*.md`, `resources/default-config.kdl` | `90d5c1bf7381` (2026-11-08). Latest release v26.04 (2026-04-25) |
| PaperWM (GNOME) | `github.com/paperwm/PaperWM` — `tiling.js`, `README.md` | `2d460db8365b` (2026-10-13) |
| PaperWM.spoon (macOS) | `github.com/mogenson/PaperWM.spoon` | `630411e7f140` (2026-10-30) |
| Karousel (KDE) | `github.com/peterfajdiga/karousel` | `baeff392e5a5` (2026-10-25) |
| Hyprland scrolling (core) | `github.com/hyprwm/Hyprland` `src/layout/algorithm/tiled/scrolling/*`; wiki `hyprwm/hyprland-wiki` `content/configuring/layouts/scrolling-layout.md` | Hyprland `a8d9a4e61dd5` (2026-11-08); wiki `ec1068fe` (2026-11-07) |
| hyprscrolling / hyprscroller / scroll | `hyprwm/hyprland-plugins`, `dawsers/hyprscroller`, `dawsers/scroll` | plugins `d2427d4d` (2026-10-20); hyprscroller archived (last push 2025-04-22) |
| papersway | CPAN `App-papersway` 3.003 docs (homepage blocks bots) | 3.003, 2026-10-15 |
| komorebi scrolling (Windows) | `github.com/LGUG2Z/komorebi` `docs/usage/scrolling-layout.md`, `komorebi/src/scrolling.rs` | `72aa4c97ad` (2026-10-31) |
| driftwm (2D canvas) | `github.com/malbiruk/driftwm` README | `e97a9d04fa95` (2026-10-28) |

Link shorthand below: `niri:` = `https://github.com/niri-wm/niri/blob/90d5c1bf7381/`, etc. Line anchors refer to these commits.

---

## 0. The ten findings that matter most for mypaper

1. **niri is the reference design.** It is the only one that is a full compositor, so it can do things the extension and script WMs can't: it never shows off-screen windows, it batches resizes across windows into one atomic update, and it can freeze any client that takes more than 300 ms. Its rules are written down as [design principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md) that are worth adopting as-is:
   - **Opening a window never resizes the others.**
   - **The focused window never moves on its own.**
   - **Actions apply immediately**, even mid-animation.
   - **Every edge case is considered**, even when the answer is "not supported".
2. **The hard platform problem is off-screen windows**, not layout. Mutter won't move a window more than 75 px off-screen, and macOS won't move one fully off-screen. Windows also shows a window on whichever monitor it overlaps. Each WM that isn't a compositor works around this:
   - **PaperWM** draws *clones* of the windows and clips them per monitor.
   - **PaperWM.spoon** parks windows in a margin at the screen edge and tells users to stack monitors vertically.
   - **Karousel** simply doesn't support multiple screens.
   - **komorebi on Windows** parks windows with 5 px still visible, or hides them when another monitor is next to that edge.

   See §3.
3. **Which windows float automatically:**
   - **niri** floats windows that have a parent, and windows with a fixed height (min == max).
   - **Hyprland** floats windows that have a parent or a fixed size on either axis. For X11 it also floats modal and transient windows, `task_dialog`/`pop-up` roles and override-redirect windows.
   - **PaperWM** tiles only NORMAL, non-transient windows that aren't on all workspaces.
   - **Karousel** also needs a window to be movable *and* resizable, non-modal, and not fullscreen-sized at map time.

   Every project then adds a window-rule escape hatch and ships a few per-app rules by default. This is unavoidable.
4. **When a window's minimum size is bigger than its column**, every serious implementation lets the window win:
   - **niri** clamps the column to the largest min-width (min beats max). A column wider than the view is left-aligned.
   - **Karousel** clamps it but caps it at the tiling-area width.
   - **komorebi** learns the real width after a resize and widens the column, never beyond the screen and never shrinking back.
   - **PaperWM** re-lays-out up to 5 times until it settles.
5. **Dialogs open as floating, centred over their parent, and on the parent's workspace** (niri). Popups are constrained *horizontally to the window's own width*, so they stay visible when the window is scrolled flush against the screen edge. Floating "inside" the scrolling strip was considered and rejected except for modal dialogs ([niri#1116](https://github.com/niri-wm/niri/issues/1116)).
6. **Fullscreen is a normal column you can scroll away from** in niri and in Hyprland's "layout-aware" fullscreen. Pain point: entering fullscreen expels the window from a multi-window column, and leaving doesn't put it back ([niri#426](https://github.com/niri-wm/niri/issues/426), still open). New windows never steal focus from an active fullscreen window.
7. **Monitor hotplug:**
   - **niri:** workspaces move to a surviving monitor and remember their *original* monitor, so reconnecting restores them. Opening a window on a workspace makes it adopt its current monitor (to avoid surprises). Windows survive having zero monitors.
   - **PaperWM:** shares one GNOME workspace pool across monitors.
   - **Karousel:** single-screen only.
8. **Throttle off-screen windows.** niri sends frame callbacks to non-visible windows only ~1×/s. This saves power but made screencasts of off-screen windows stutter ([niri#539](https://github.com/niri-wm/niri/issues/539)). It fits mypaper's "net-negative resources" rule well.
9. **2D canvases exist but are young.**
   - **driftwm** (Rust/Smithay, created 2026-02-22, ~1.9k★) is the serious one: windows at their real size on a 2D plane, the screen is a camera that pans and zooms, windows that snap together form implicit clusters, and each monitor is a separate viewport onto the same canvas.
   - **On Windows**, Apeiron and WinCanvas fake zoom with DWM thumbnails, because "Windows can't scale real windows".
10. **Distros:**
    - **KaOS** made niri + Noctalia its default from 2026.02 (KaOS's own announcement).
    - **CachyOS** offers niri in its installer.
    - **Omarchy** is Hyprland with the *dwindle* layout by default; scrolling is a commented-out option.
    - **Hyprland** moved scrolling into core on 2026-02-21 and dropped the hyprscrolling plugin on 2026-05-12.

---

## 1. niri in depth

### 1.1 Model

- **Hierarchy:** monitor → vertical list of **dynamic workspaces** → each workspace has a **scrolling layout** (an endless horizontal row of columns), plus its own **floating layer** that doesn't scroll ([Workspaces.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Workspaces.md), [Floating-Windows.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Floating-Windows.md)).
- **Columns** hold one or more tiles stacked vertically, or are **tabbed**: all tabs share one size, and a tab indicator is clickable ([Tabs.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Tabs.md)).
- **Workspaces:** there is always one empty workspace at the bottom of each monitor. Empty workspaces in the middle vanish when you leave them. Workspaces have **no fixed index**: "focus-workspace 2" means "whatever is second on this monitor right now". *Named* workspaces persist ([Workspaces.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Workspaces.md)).
- **Coordinates:** logical pixels per output. Fractional gaps and borders are rounded to physical pixels ([Configuration:-Layout.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Layout.md)).

### 1.2 Widths, heights, gaps, struts

- **`preset-column-widths`** (default ⅓, ½, ⅔), cycled with Mod+R / Mod+Shift+R. **`default-column-width`** applies to new windows. An empty `{}` lets the window choose (niri sends width 0). This is ambiguous in the protocol, and some clients misread it ([Configuration:-Layout.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Layout.md)).
- **Proportion vs fixed:** proportions size the *tile* (including niri's border) and account for gaps, so four `0.25` columns fit exactly. `fixed N` sizes the *window itself*. "Testing a specific client size" is the stated reason ([Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md#window-layout); code: [`workspace.rs#L915-L936`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/workspace.rs#L915-L936)).
- **Column width = clamp(resolved, largest min-width of its tiles, smallest max-width)**, where `max_width = max(max_width, min_width)`, so **min wins**. Height distribution:
  - fixed or preset heights are subtracted first;
  - a tile with min == max height becomes fixed;
  - auto heights are distributed by weight in a loop that bumps any tile whose minimum isn't met and starts again;
  - **max height isn't respected for auto tiles**, a documented simplification;
  - in tabbed mode, the tallest tab minimum is capped at the working-area height.

  Source: [`scrolling.rs#L4594-L4860`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L4594-L4860); the `max-height` caveat is in [Window-Rules § Size Overrides](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md#size-overrides).
- **A window smaller than its tile** (it refused the size) is **centred inside the tile**. A fullscreen tile is at least the view size, with a black backdrop behind it ([`tile.rs#L780-L820`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tile.rs#L780-L820); [Fullscreen-and-Maximize.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Fullscreen-and-Maximize.md)).
- **"Larger than the screen → align to the top-left"**, because the top-left usually holds the important content ([Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md#window-layout)). In code, a column wider than the view is always left-aligned ([`scrolling.rs#L5633-L5662`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L5633-L5662)).
- **Struts** are outer margins. Left/right struts make the neighbouring column "peek" in. Negative struts plus gaps emulate inner-only gaps ([Configuration:-Layout.md § struts](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Layout.md)).
- **Since 25.11**, layout settings can be overridden per output and per named workspace (same doc).

### 1.3 The view (what scrolls, and when)

- **`center-focused-column`:**
  - `never` (default): scroll just enough;
  - `always`;
  - `on-overflow`: centre only if the new column doesn't fit on screen together with the column focus came from.
- **`always-center-single-column`:** a lone column is centred ([Configuration:-Layout.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Layout.md)).
- **Fit algorithm:**
  - if the column is already fully visible (with padding = min(gaps, half the leftover space)), don't move;
  - otherwise align whichever edge needs **less motion**;
  - a column wider than the view is left-aligned;
  - fullscreen columns align to the view, not the working area;
  - maximized columns use the parent area with no padding.

  Source: [`scrolling.rs#L575-L700`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L575-L700), [`#L5633-L5662`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L5633-L5662).
- **"Focus follows view" on gestures:** at the end of a touchpad swipe, niri projects where the gesture would stop with deceleration, snaps to the nearest column edge (or centre, in centring modes), and **activates that column**. Snapping never goes past the first or last column. Cancelling a swipe isn't supported, "just like GNOME Shell" ([`scrolling.rs#L3257-L3400`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L3257-L3400)).
- **Invisible state, kept to a minimum:** niri remembers **one** previous view position per workspace (for dialogs and fullscreen) and forgets it when focus changes ([Design-Principles § invisible state](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md)).
- **focus-follows-mouse** has `max-scroll-amount`, e.g. `"0%"` means "never scroll just because the mouse hovered" ([Configuration:-Input.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Input.md)). Without it, CSD resize margins that "peek out" at the monitor edge cause accidental focus ([FAQ.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/FAQ.md)).

### 1.4 New windows: where they go and whether they get focus

- **Placement:**
  - a window with a parent → `AddWindowTarget::NextTo(parent)`;
  - otherwise the workspace/output named by a window rule;
  - otherwise the active monitor.

  The parent is only followed if it is on the same output the window was configured for ([`handlers/compositor.rs#L177-L215`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/handlers/compositor.rs#L177-L215)).
- **Scrolling insert:** a new column goes at **active index + 1** (right of the focused column). Columns on the far side animate; the view position before opening is remembered (`activate_prev_column_on_removal`) ([`scrolling.rs#L999-L1060`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L999-L1060)). Inserting directly into an existing column is *not* offered: computing the initial size and handling the column disappearing are hard, and users' wishes differ ([FAQ.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/FAQ.md)).
- **Focus policy:**
  - a valid xdg-activation token → focus;
  - otherwise `Smart`:
    - auto-placed windows are activated **unless the active window is fullscreen** (don't steal from fullscreen);
    - `NextTo` windows are activated **only if the parent is the active window**;
    - explicit-column inserts are not activated.
  - A debug flag forces strict "no focus".

  Source: [`compositor.rs#L152-L175`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/handlers/compositor.rs#L152-L175), [`workspace.rs#L634-L725`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/workspace.rs#L634-L725). The rules `open-focused true/false` and `on-xdg-activate ignore|set-urgent|focus` ("next release" at this commit) override it ([Window-Rules.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md)).
- **Opening rules** (applied once, at the initial configure): `default-column-width`, `default-window-height`, `open-on-output` (falls back to the focused output; no focus if the output isn't focused), `open-on-workspace`, `open-maximized`, `open-maximized-to-edges`, `open-fullscreen` (true/false: can *block* a requested fullscreen), `open-floating` (true/false: can *disable* auto-floating), `open-focused`, `default-floating-position` with `relative-to` anchors ([Window-Rules.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md)).
- **Late requests aren't covered:** clients that ask to maximize *after* the initial configure slip past the open-rules ([Fullscreen-and-Maximize.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Fullscreen-and-Maximize.md)). Firefox's Bitwarden popup sets its title late, so title rules miss it and you need an IPC script ([FAQ.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/FAQ.md)).

### 1.5 Dialogs, popups, transient/parented windows, floating

- **Auto-float:** a window floats if it has a parent, or if its **min height == max height** (fixed-height, e.g. splash screens), unless the `open-floating` rule says otherwise ([`window/mod.rs#L376-L396`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/window/mod.rs#L376-L396)). Width alone does not trigger it.
- **Dialog position:** centred over its parent's tile, then clamped to the working area. If the parent is itself floating, the dialog goes directly above it in z-order ([`workspace.rs#L682-L705`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/workspace.rs#L682-L705)). Tests check that raising a parent raises its children and descendants (`stacking_add_parent_brings_up_child`, [`tests.rs#L3224`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tests.rs#L3224)).
- **A dialog that is not floating** (rule says tile) opens as a new column **right of its parent** (`add_tile_right_of`, same lines).
- **Floating layer:**
  - always above tiled windows; each workspace has its own;
  - new floating windows open centred; windows moved out of tiling open near where they were on screen;
  - each window remembers its last floating position;
  - `switch-focus-between-floating-and-tiling` (Mod+Shift+V); while floating has focus, the directional binds act on floating windows.

  Source: [Floating-Windows.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Floating-Windows.md), [Window-Rules § default-floating-position](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md).
- **Popups/menus** in the scrolling layout are constrained **horizontally to the window's own width** and vertically to the whole working area, so they stay visible when the window scrolls flush to an edge. IME popups get the whole parent area instead ([`scrolling.rs#L2634-L2668`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L2634-L2668); [`handlers/xdg_shell.rs#L1285-L1315`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/handlers/xdg_shell.rs#L1285-L1315)).
- **Floating inside the strip** ("scrolls with the columns") was rejected in general for two reasons: such windows get lost or appear to jump when columns to their left close, and there's no keyboard model for reaching them. The open proposal is: *modal* dialogs only, pinned to the parent, which takes focus; if the dialog is bigger than the parent, treat its min size as the parent's and widen the column ([niri#1116](https://github.com/niri-wm/niri/issues/1116), maintainer, 2025-02-11).
- **Not supported:** sticky / always-on-top / all-workspaces windows (the FAQ points to [niri#932](https://github.com/niri-wm/niri/issues/932); IPC workarounds). Minimize: "Where would it go? We don't have a dock … You've got infinite space" ([discussion #3982](https://github.com/niri-wm/niri/discussions/3982), YaLTeR, 2026-05-05).

### 1.6 Fullscreen, maximize, windowed fullscreen

- **Three "big" modes:**
  - `maximize-column` (Mod+F): full width, keeps gaps, struts and borders, may hold several windows;
  - `maximize-window-to-edges` (Mod+M, since 25.11): the window knows it's maximized, covers struts and gaps but not the bar; several are allowed only in a tabbed column;
  - `fullscreen-window` (Mod+Shift+F): black backdrop if the window stays smaller.

  Source: [Fullscreen-and-Maximize.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Fullscreen-and-Maximize.md).
- **A fullscreen window is a normal tile.** Only "when the view is stationary on a focused fullscreen window" do the top layer-shell layer and floating windows hide. Fullscreening a floating window moves it into the strip; unfullscreening returns it to floating ([Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md); test `restore_to_floating_persists_across_fullscreen_maximize`).
- **Windowed fullscreen** (`toggle-windowed-fullscreen`, 25.05): the window *thinks* it's fullscreen but keeps its tile size. Useful for screencasting browser presentations ([Fullscreen-and-Maximize.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Fullscreen-and-Maximize.md)).
- **Known gap:** fullscreening a window that shares a column expels it; unfullscreening doesn't put it back. The maintainer's latest idea is to auto-switch the column to tabbed while fullscreen instead of expelling ([niri#426](https://github.com/niri-wm/niri/issues/426), comments 2024-06-06 and 2025-12-08).
- **Games:**
  - the wiki recommends **gamescope** for games that misbehave on "non-stacking desktop environments";
  - pointer confinement is up to the client: "It's just how the client decides to do it" ([niri#2672](https://github.com/niri-wm/niri/issues/2672));
  - Steam windows stealing focus or moving over games is blamed on X11 activation through xwayland-satellite ([niri#2386](https://github.com/niri-wm/niri/issues/2386)).

  Wiki: [Application-Issues.md § Fullscreen games](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Application-Issues.md).

### 1.7 Closing

- **Closing the focused column:** if it was opened right of the previous column and focus hasn't moved since, niri returns to that column **and restores the old view offset**. Otherwise it focuses the column now at the same index (the right neighbour), or the last column. Closing a column to the left keeps the focused window where it is on screen ([`scrolling.rs#L1196-L1268`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L1196-L1268)).
- **Closing a tile inside a column:** the remaining tiles animate up; a lone survivor's weight resets to 1; any interactive resize on it is cancelled (`#L1074-L1170`).
- **Close animation:** a snapshot is taken *before* the window unmaps, so it animates even after the client is gone ([`compositor.rs#L262-L275`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/handlers/compositor.rs#L262-L275)).

### 1.8 Multi-monitor, hotplug, DPI

- **Each monitor has its own workspaces.** Workspaces can move between monitors. Mental model from the wiki: "My first monitor contains these workspaces… I can move workspace Y to my second monitor" ([Workspaces.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Workspaces.md)).
- **Disconnect:**
  - the workspaces go to the monitor first in their `original_outputs` list, otherwise the primary;
  - the last active workspace of each output name is remembered;
  - with **zero** monitors, every workspace and window lives on in `MonitorSet::NoOutputs`.

  Source: [`layout/mod.rs#L878-L951`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L878-L951).
- **Reconnect:** workspaces whose preferred output is the returning one move back, and so does that output's previously active workspace ([`#L760-L876`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L760-L876)). Output IDs are upgraded from connector name to make/model/serial when known ([`workspace.rs#L479-L545`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/workspace.rs#L479-L545)).
- **Surprise control:** a regular workspace that gets a new window forgets its original monitor. Named workspaces never forget ([Workspaces.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Workspaces.md), [Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md)). Property tests cover the ordering cases, e.g. `workspace_output_fallback_promotes_preferred_in_reverse_connection_order` and `removing_all_outputs_preserves_empty_named_workspaces` ([`tests.rs#L2480`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tests.rs#L2480)).
- **DPI:** all layout is in each output's logical pixels. Moving a tile to another output calls `update_config(view_size, scale)` and sends the new preferred scale. Proportional widths re-resolve against the new output; fixed widths stay in logical px ([`mod.rs#L4029-L4044`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L4029-L4044)).
- **No global coordinate system across monitors.** Layout code doesn't know where monitors sit, so a window dragged across monitors "teleports" instead of animating (FIXME at [`mod.rs#L3998-L4003`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L3998-L4003); also the FAQ's reason for avoiding built-in Xwayland).
- **Open requests:**
  - vertical layout for portrait monitors ([niri#1071](https://github.com/niri-wm/niri/issues/1071), open, among the most-upvoted);
  - proportional cursor mapping between mismatched monitors (discussion #4324).

### 1.9 Input: keys, gestures, interactive move/resize, DnD

- **Key grammar** (from the default config, "largely from PaperWM"):
  - a key that *focuses* somewhere, plus **Ctrl**, *moves* the window or column there;
  - **Shift** is the alternative: across monitors for focus/move, height instead of width for resizes;
  - Mod+U/I (workspaces) sit one row above Mod+J/K (windows).

  Source: [Design-Principles § Default config](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md), [`default-config.kdl#L420-L651`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/resources/default-config.kdl#L420-L651). The table in §5.1 lists them.
- **Mouse:**
  - Mod+LMB moves a window; **right-click while moving** toggles whether it drops into floating or tiling;
  - Mod+RMB resizes;
  - **double-click** a top/bottom resize edge resets the height to auto; a left/right edge toggles full width;
  - Mod+MMB drag pans horizontally or switches workspaces vertically;
  - Mod+wheel switches workspaces with `cooldown-ms=150`; Mod+Shift+wheel scrolls columns.

  Source: [Gestures.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Gestures.md), default config.
- **Touchpad:** 3-finger horizontal swipe = scroll the view; 3-finger vertical = switch workspace; 4-finger vertical = overview. Touchscreen and tablet: Mod+drag moves a window; dragging a *tiled* window horizontally scrolls instead ([Gestures.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Gestures.md)).
- **Interactive move internals** ([`layout/mod.rs#L94-L100`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L94-L100), [`#L3940-L4080`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L3940-L4080)):
  - a tiled window **rubber-bands** until the pointer has moved 256 px, then pops out of the layout;
  - when it pops out, fullscreen and maximize are cleared and it renders at 0.75 alpha;
  - an **insert hint** shows where it will land;
  - only one move at a time, by design ([Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md));
  - client-initiated moves (titlebar drag via CSD) go through the same path.
- **DnD:**
  - the view scrolls at monitor edges (30 px band, 100 ms delay, up to 1500 px/s);
  - in the overview, workspaces switch at the top/bottom edges;
  - **hold over a window to activate it**, or over a workspace to switch to it;
  - the hot corner works mid-drag, so mouse-only DnD across workspaces is possible.

  Source: [Configuration:-Gestures.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Gestures.md), [Overview.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Overview.md).

### 1.10 Overview and Alt-Tab

- **Overview** (25.05): the whole stack of workspaces zooms out (default `zoom` around 0.5, configurable). It opens with:
  - a **Mod tap** (a release-bind on bare Mod: [`default-config.kdl#L400-L409`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/resources/default-config.kdl#L400-L409));
  - the top-left hot corner;
  - a 4-finger swipe.
- **In the overview:**
  - every keybinding still works;
  - the mouse drags windows, right-drag scrolls, the wheel switches workspaces without Mod;
  - you can drop a window *between* workspaces to create a new one;
  - background/bottom layer-shell surfaces zoom with the workspaces; top/overlay surfaces stay above, so "put your bar on the top layer" ([Overview.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Overview.md)).
- **Keyboard focus while open** goes to a synthetic `KeyboardFocus::Overview`; windows get no keys ([`niri.rs#L1417-L1442`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/niri.rs#L1417-L1442)). A window that opens meanwhile is added to the layout as usual, but gets keys only after the overview closes (inferred from the code; not tested live). DnD-hold activation closes the overview in sync with the workspace switch ([`mod.rs#L2715-L2738`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L2715-L2738)).
- **Recent-windows switcher** (Alt-Tab, 25.11): MRU list with previews; 150 ms open delay; 750 ms debounce before a window counts as "used"; `filter="app-id"` on Alt+\` ([Configuration:-Recent-Windows.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Recent-Windows.md)).

### 1.11 Animation, transactions, performance

- **Animations are springs or easing curves:**
  - view, window movement, window resize, workspace switch and overview all default to **critically damped springs** (damping 1.0, stiffness 800–1000);
  - open/close use 150 ms easing;
  - springs carry the finger's velocity when a swipe is released;
  - overdamped springs are numerically unstable, so they're advised against.

  Source: [Configuration:-Animations.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Animations.md).
- **Many windows animate as one:** each column/tile animates its own *offset* toward the new layout (`animate_move_x_from`), so inserting or removing one column moves the others with one shared config ([`scrolling.rs#L1030-L1041`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/scrolling.rs#L1030-L1041)). When a resize moves the view, both use the `window-resize` config ("synchronized animations").
- **Resize animation:** only user-initiated resizes animate, and only above 10 px ([`mod.rs#L94`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/mod.rs#L94)). It cross-fades a snapshot of the old buffer while the client catches up (`ResizeAnimation` holds a snapshot plus an offscreen buffer, [`tile.rs#L143-L160`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tile.rs#L143-L160)).
- **Transactions:** resizes across several windows (a column, tabs) are committed atomically. A **300 ms deadline** releases them if a client is slow ([`utils/transaction.rs#L15-L18`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/utils/transaction.rs#L15-L18)).
- **Timing:** one `LazyClock` timestamp per event-loop iteration, overridden to the predicted presentation time when rendering, plus a slowdown factor ([Development:-Animation-Timing.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Animation-Timing.md)). Disabled animations still "run" with 0 duration, which avoids edge cases; disabled eye-candy costs nothing ([Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md)).
- **Off-screen throttling:** frame callbacks go to non-visible surfaces only ~once a second (`FRAME_CALLBACK_THROTTLE = 995 ms`, [`niri.rs#L204-L207`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/niri.rs#L204-L207)). Side effect: windows that are screencast while off-screen stutter ([niri#539](https://github.com/niri-wm/niri/issues/539), open).

### 1.12 IPC and window rules

- **IPC:** JSON lines over a Unix socket. An event stream sends the full state first, then deltas. It is not always atomic: a window can briefly reference a workspace that was just removed ([IPC.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/IPC.md)). The FAQ answers many requests ("open in same column", sticky) with "script it over IPC".
- **Rules:**
  - matched by `app-id`/`title` regex, `is-active`, `is-focused`, `is-active-in-column`, `is-floating`, `is-window-cast-target`, `is-urgent`, `at-startup` (first 60 s);
  - `match` directives are OR-ed; matchers inside one directive are AND-ed; `exclude` subtracts;
  - rules apply in config order, later ones override, and nothing can be "unset";
  - opening properties apply once, dynamic ones continuously.

  Source: [Window-Rules.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md).
- **Title/app-id are not double-buffered** in Wayland, so title-based screencast block-out can leak a frame ([Window-Rules § block-out-from](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Configuration:-Window-Rules.md)).
- **Shipped per-app rules:** WezTerm's zero-size configure bug, and Firefox Picture-in-Picture floating ([`default-config.kdl#L314-L329`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/resources/default-config.kdl#L314-L329)).

### 1.13 How niri tests the layout

[`src/layout/tests.rs`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tests.rs) (about 4,000 lines) uses **proptest**. Random `Op` sequences cover:
- add/remove output, with scales 1–5;
- windows with arbitrary min/max sizes and parent IDs, including parent loops;
- gestures, struts, gaps, centring modes and tab-indicator positions.

It asserts invariants after every op (`operations_dont_panic`, [`#L1777`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tests.rs#L1777)), plus dozens of named regression cases (`large_max_size`, `parent_id_causes_loop`, `interactive_move_drop_on_other_output_during_animation`, …). The same model (pure layout state, synthetic clock) suits mypaper.

### 1.14 Known pain points (from issues)

| Pain point | Source |
|---|---|
| Off-screen windows refresh at 1 Hz in screencasts | [#539](https://github.com/niri-wm/niri/issues/539) (open) |
| Unfullscreen doesn't restore a window to its column | [#426](https://github.com/niri-wm/niri/issues/426) (open) |
| No sticky / always-on-top / all-workspaces windows | [#932](https://github.com/niri-wm/niri/issues/932) (open, 2nd most upvoted) |
| No vertical strip for portrait monitors | [#1071](https://github.com/niri-wm/niri/issues/1071) (open) |
| People coming from static-workspace WMs want static workspaces | [disc. #2274](https://github.com/niri-wm/niri/discussions/2274) |
| Accidental focus at monitor edges via CSD margins | [#4621](https://github.com/niri-wm/niri/issues/4621), [FAQ](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/FAQ.md) |
| Resize steps pile up when a window hits its min size | [#3116](https://github.com/niri-wm/niri/issues/3116) (open) |
| Chromium `beforeunload` dialog invisible until a resize | [#4494](https://github.com/niri-wm/niri/issues/4494) (closed) |
| Width/height rules for an app-id also hit its dialogs | [#3895](https://github.com/niri-wm/niri/issues/3895) (closed) |
| Clients asking for maximize/fullscreen *after* the initial configure bypass rules | [Fullscreen-and-Maximize.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Fullscreen-and-Maximize.md) |
| X11 apps that position their own windows/bars break; run them in a nested compositor | [Xwayland.md](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Xwayland.md) |
| Games: cursor escapes to other monitors; launchers steal focus | [#2672](https://github.com/niri-wm/niri/issues/2672), [#2386](https://github.com/niri-wm/niri/issues/2386) |
| No minimize | [disc. #3982](https://github.com/niri-wm/niri/discussions/3982) |

---

## 2. The other implementations

### 2.1 PaperWM (GNOME Shell extension, the original)

- **Model:**
  - one scrollable strip per GNOME workspace;
  - new windows go right of the active one (configurable: left/start/end) and take full height;
  - Super+I/O absorb into and expel from a column;
  - a separate **scratch layer** (floating, above tiling) is the "escape hatch"; hiding it **minimizes** its windows;
  - a "window position bar" in the top bar shows where the view sits in the strip;
  - most bindings preview while Super is held and commit on release; Esc aborts.

  Source: [README](https://github.com/paperwm/PaperWM/blob/2d460db8365b/README.md).
- **Monitors:** the workspace stack is **shared** across monitors. Another monitor just shows another workspace, and "windows [are] resized vertically as necessary when workspace is displayed on another monitor" (README). Long-standing requests: [#165](https://github.com/paperwm/PaperWM/issues/165) (split one workspace across monitors), [#753](https://github.com/paperwm/PaperWM/issues/753) (one panorama strip across monitors), [#1049](https://github.com/paperwm/PaperWM/issues/1049).
- **Off-screen workaround:** "Mutter prevents windows from being placed further off the screen than 75 pixels" (`stack_margin = 75`, [`tiling.js#L25-L26`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L25-L26)). PaperWM therefore renders **Clutter clones** of every tiled window inside a per-monitor clipped container and scrolls the container. Real windows are hidden during animation and moved into place when it ends ([`tiling.js#L55-L75`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L55-L75)). Windows past the edge are "stacked" in the margin; a click overlay turns a click there into activation only ([`stackoverlay.js`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/stackoverlay.js)).
- **Which windows tile:** not transient, `window_type == NORMAL`, not on all workspaces, not scratch ([`tiling.js#L4048-L4066`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L4048-L4066), [`#L3445-L3474`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L3445-L3474)).
- **Sizes:**
  - the window's own frame width *is* the column width, capped at work area − 2·`minimum_margin` ([`#L720-L739`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L720-L739));
  - **non-resizable windows are only moved**, and their real size feeds the layout;
  - when an X11 window answers with a different size, the column is laid out again, up to 5 times ("Bail at fixpoint") ([`#L593-L620`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L593-L620), [`#L750-L762`](https://github.com/paperwm/PaperWM/blob/2d460db8365b/tiling.js#L750-L762)).
- **Focus modes:** DEFAULT (free scroll), CENTER, EDGE (snap to the nearest edge; a lone window is centred). `preferredWidth` window property (README).
- **GNOME settings it turns off at runtime:** `edge-tiling` (native half-snap), `attach-modal-dialogs` ("visual glitching"), `workspaces-only-on-primary` (README § Managed Gnome Shell Settings). The README also lists incompatible extensions: desktop icons (they create invisible windows), rounded corners, Dash to Panel.
- **Gestures:** 3-finger up = GNOME overview, down = workspace stack, left/right = scroll (README).
- **Pain points:** breakage on every GNOME release (the top issues are GNOME 40/42/49/50 support), touchpad gestures broken on GNOME 49 ([#1092](https://github.com/paperwm/PaperWM/issues/1092)), multi-monitor overview ([#728](https://github.com/paperwm/PaperWM/issues/728)), tabbed columns wanted ([#1012](https://github.com/paperwm/PaperWM/issues/1012)).

### 2.2 PaperWM.spoon (macOS, Hammerspoon)

- **Model:** a PaperWM-style strip per Mission Control space. It requires "Displays have separate Spaces" and *not* auto-rearranging spaces. Extras: accordion "stacked" columns with a title-bar peek (`accordion_peek = 36`, `stack_min_height`); `split_screen`; mouse drag/lift; swipe scrolling ([README](https://github.com/mogenson/PaperWM.spoon/blob/630411e7f140/README.md)).
- **Limits** (README § Limitations):
  - "MacOS does not allow a window to be moved fully off-screen. Windows that would be tiled off-screen are placed in a margin on the left and right edge… still visible and clickable";
  - dragging between spaces/screens is hard to detect;
  - "**Arrange screens vertically** to prevent windows from bleeding into other screens".
- **Filtering:**
  - windows that have a zoom button but aren't maximizable are ignored (fixed-size, so it floats them), with a user override;
  - Apple apps with native tabs show each tab as a separate window, so they're refused ([`windows.lua#L176-L201`](https://github.com/mogenson/PaperWM.spoon/blob/630411e7f140/windows.lua#L176-L201)).

### 2.3 Karousel (KDE KWin script)

- **Model:**
  - columns on a strip per virtual desktop; windows centred when they fit; the user owns widths;
  - three scroll behaviours: **lazy** (default), **centered**, **grouped**;
  - stacked columns (only the active window shown, Meta+X);
  - "squeeze left/right column onto the screen";
  - "equalize visible columns".

  Source: [README](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/README.md), [`config/definition.ts`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/config/definition.ts).
- **Limits:**
  - "Doesn't support multiple screens"; you get one grid and a "Move Karousel grid to the current screen" action;
  - per-monitor virtual desktops are missing in KWin ([KDE bug 107302](https://bugs.kde.org/show_bug.cgi?id=107302));
  - off-screen windows would show on the neighbouring monitor, and scripts can't set `stackingOrder`.

  Source: maintainer comments, 2023-07-07 to 2026-05-05, in [karousel#3](https://github.com/peterfajdiga/karousel/issues/3).
- **Tileability:**
  - `canTileEver` = (movable && resizable) || fullscreen, not a popup, not in a deny list;
  - `shouldTile` = normalWindow && !transient && !modal && managed && pid > −1 && !fullScreen && **not fullscreen-sized geometry** && no float rule.

  Source: [`Clients.ts#L7-L12`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/world/Clients.ts#L7-L12), [`WindowRuleEnforcer.ts#L13-L24`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/rules/WindowRuleEnforcer.ts#L13-L24). Windows that only turn out untileable after leaving fullscreen are floated then ([`Tiled.ts#L174-L182`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/world/clientState/Tiled.ts#L174-L182)). A default rule list ships for plasmashell, polkit, kcalc, krunner, yakuake, Steam Big Picture, Zoom, JetBrains splash… (`definition.ts`).
- **Column width:** clamped to [max of the windows' min widths, capped at the tiling-area width; min of their max widths, never below min] ([`Column.ts#L79-L101`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/layout/Column.ts#L79-L101)).
- **App-initiated resizes are honoured:** the column follows the app. Karousel **gives up after 4 external geometry changes per 1000 ms** and lets the client keep its frame, which stops resize fights ([`Tiled.ts#L6`, `#L116`, `#L162-L171`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/world/clientState/Tiled.ts); test [`externalResize.ts`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/tests/flows/externalResize.ts)).
- **Hybrid with native snap (unique):** a window the user quick-tiles with KWin becomes **Pinned**. Pinned windows carve their rectangles out of the screen, and the strip uses the **largest free rectangle** that's left ([`PinManager.ts#L15-L40`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/world/PinManager.ts#L15-L40), [`Tiled.ts#L186-L193`](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/src/lib/world/clientState/Tiled.ts#L186-L193)). Directly relevant to "replace or coexist with Windows Snap".
- **Layering and drag:** `tiledKeepBelow` (on by default) keeps tiled windows below and floating ones above. `untileOnDrag` (on by default): dragging a tiled window's titlebar floats it (`definition.ts`; tests `dragTiled.ts`).

### 2.4 Hyprland: hyprscrolling, then core `scrolling`; hyprscroller, then scroll

- **History:**
  - hyprscrolling was the official plugin;
  - the "Rethonk layouts" rewrite ([Hyprland PR #12890](https://github.com/hyprwm/Hyprland/pull/12890), merged 2026-02-21, shipped in v0.54.0 on 2026-02-27) checked off "Migrate scroll into core", keeping the `plugin:hyprscrolling:` variables under `scrolling:`, and added `direction` left/right/up/down per workspace;
  - hyprland-plugins then removed unmaintained plugins (commit [`3aa21f2e`](https://github.com/hyprwm/hyprland-plugins/commit/3aa21f2e), 2026-05-12), and hyprscrolling is gone from the repo.
- **Config** ([wiki](https://github.com/hyprwm/hyprland-wiki/blob/ec1068fe/content/configuring/layouts/scrolling-layout.md)):
  - `column_width` 0.5;
  - **`fullscreen_on_one_column` true** (a lone column fills the screen);
  - `focus_fit_method` center|fit (default fit);
  - `follow_focus` 0/1/2;
  - `follow_min_visible` 0.4 (soft focus scrolls only if at least 40 % of the window is visible; "hard input" always scrolls);
  - `explicit_column_widths` "0.333, 0.5, 0.667, 1.0";
  - `wrap_focus` / `wrap_swapcol` true;
  - `direction`.
- **Layout messages:** `consume`, `expel`, `fit active|visible|all|toend|tobeg|expand`, `inhibit_scroll`, `move ±px|±col`, `promote`, `swapcol`; window rule `scrolling_width`.
- **Layout-aware fullscreen:** you can scroll away from a fullscreen window, but by default focus moves to the *other monitor* unless `binds.movefocus_cycles_fullscreen` is set.
- **Code** ([`ScrollingAlgorithm.cpp`](https://github.com/hyprwm/Hyprland/blob/a8d9a4e61dd5/src/layout/algorithm/tiled/scrolling/ScrollingAlgorithm.cpp)):
  - **new window** → a new column after the focused window's column (`newTarget`, L725-L787);
  - **tiled drag-drop:** within 30 % of a target's edge → new column; otherwise insert into that column above/below by cursor y;
  - **mouse-down focus doesn't move the view, only click-up does** (L635-L663);
  - after removing the last column, the offset is clamped to avoid "cringe extra space" (L789-L820);
  - guards against zero or negative areas while a monitor reconnects after suspend (L951, L1989).
- **Auto-float is core, for every layout:**
  - Wayland: `suggestsFloat = parent || FIXED_SIZE` (min == max on *either* axis) ([`WaylandBackend.cpp#L27-L39`](https://github.com/hyprwm/Hyprland/blob/a8d9a4e61dd5/src/desktop/view/window/WaylandBackend.cpp#L27-L39));
  - X11 adds modal, transient, `task_dialog`/`pop-up` role, override-redirect, floating atoms ([`X11Backend.cpp#L84-L95`](https://github.com/hyprwm/Hyprland/blob/a8d9a4e61dd5/src/desktop/view/window/X11Backend.cpp#L84-L95)).
- **hyprscroller** (dawsers) was archived: "Hyprland was starting to limit what I could add through a plugin… too brittle and dependant on hacks" ([README](https://github.com/dawsers/hyprscroller/blob/7e63677d2cfa/README.md)). It continues as **scroll**, a sway fork with only a scrolling layout ([README](https://github.com/dawsers/scroll)). scroll adds:
  - per-window content scaling and **workspace scaling** ("scale the workspace to any scale and continue working");
  - overview and jump (easymotion-style) modes;
  - Lua scripting;
  - "spaces" (saved arrangements);
  - per-output portrait/landscape orientation;
  - **virtual outputs** that split ultrawides;
  - four fullscreen modes (workspace, global, application, layout).

### 2.5 papersway (sway/i3, Perl over i3-IPC)

- Runs as the bar's `status_command` and drives sway/i3 over IPC.
- `--cols=N` (minimum and default 2) or `--cols-min-width=N` (scaled by output scale).
- Commands: `fresh-workspace`, `bury-column`, `absorb-expel`, `cols incr/decr`, `width toggle`.
- You must remove split/layout/focus-parent bindings. Version 3.003 (2026-10-15) fixes i3 reusing container IDs.

Source: [CPAN docs](https://metacpan.org/dist/App-papersway/view/bin/papersway). The homepage blocks bots. *How* it hides off-screen windows isn't documented there **(unverified)**.

### 2.6 komorebi `Scrolling` layout (Windows; verified to exist)

This is the closest prior art to mypaper's platform: a tiling WM that has to work with ordinary Win32 windows.

- **History:** added 2025-05-17 (`b4e61b07` "feat(wm): add scrolling layout"); heavily reworked 2026-10-30 (shared `komorebi-scrolling` crate with property tests).
- **Behaviour** ([docs](https://github.com/LGUG2Z/komorebi/blob/72aa4c97ad/docs/usage/scrolling-layout.md)):
  - `columns` (default 3) sets the column width; a lone window is ⅓ wide, at the left;
  - `center_focused_column` Never/Always/OnOverflow (borrowed from niri);
  - consume/expel; column stacks;
  - **closing returns focus and scroll to the column a window was opened next to, if focus hasn't moved**;
  - `pan` without moving focus;
  - touchpad panning via `whkd` reading the raw touchpad, because "Windows does not give three-finger gestures to programs".
- **Min width:** after a resize komorebi reads the real width; if the window didn't shrink, the column widens to fit, never past the screen and never shrinking back.
- **Off-screen windows** ([`scrolling.rs#L49-L140`](https://github.com/LGUG2Z/komorebi/blob/72aa4c97ad/komorebi/src/scrolling.rs#L49-L140)):
  - **parked** at the edge they left by, with 5 px still visible and their size kept, so returning needs no resize;
  - if another monitor touches that edge, the window is **hidden** instead;
  - a partly visible window is **clipped narrower** to fit its own work area;
  - borders on parked windows show as a thin line at the screen edge.

### 2.7 2D "infinite canvas" WMs

| Project | Status (primary source) | How the 2D canvas works |
|---|---|---|
| **driftwm** (Wayland, Rust/Smithay) | Created 2026-02-22, ~1.9k★, pushed 2026-11-05. "Experimental software, primarily built with AI." Packaged on AUR and Nix ([README](https://github.com/malbiruk/driftwm/blob/e97a9d04fa95/README.md)) | Windows keep their real size on a 2D plane; the screen is a camera that pans with momentum and **zooms**. Edges snap and snapped windows become an **implicit cluster** that moves and resizes together. "Fit window" stands in for maximize (zoom back to 1, fill the screen, neighbours shift out). Fullscreen ends at the next canvas action. **Each monitor is a separate viewport on the same canvas**, with outlines showing where the others look. Rules: `widget` (pinned to the canvas, below windows) and `pinned_to_screen` (ignores pan/zoom: PiP, call bars). Bookmarks, exported to bars as ext-workspace. Optional suspend-on-close placeholders and session restore |
| **Phantomat** (Hyprland plugin/scripts) | Created 2026-09-24, 247★ ([README](https://github.com/kaolti/phantomat)) | One plane; all screens form "one desk", so a window can straddle the seam; a lens-style zoom-out with type-to-find; built on hyprland-scroll-overview |
| **Infinidesk** (wlroots, C) | Small, 12★ ([m31-galaxy/Infinidesk](https://github.com/m31-galaxy/Infinidesk)) | Pan/zoom, edge snapping (per search-result summary, **(unverified)** in detail) |
| **hevel** (neuswc) | sourcehut `~dlm/hevel` | Infinite plane, mouse-chord driven, Plan-9/rio lineage (search-result description only, **(unverified)**) |
| **vxwm** (X11, dwm fork) | [codeberg](https://codeberg.org/wh1tepearl/vxwm) | "Infinite tags": the screen is a viewport onto a continuous canvas; `homecanvas` returns to the origin |
| **Apeiron** (Windows) | Created 2026-10-30 ([README](https://github.com/hectoragofi/apeiron)) | "Windows can't scale real windows, and apps repaint late when moved." While you gesture, a fullscreen overlay of **1:1 live DWM thumbnails** pans and zooms; real windows snap to the thumbnails' positions once you release near 100 %. The overlay covers only the primary monitor; elevated windows need an elevated Apeiron |
| **WinCanvas** (Windows) | 10★ ([README](https://github.com/sermtech/wincanvas)) | A switcher, not a WM: DWM thumbnails on a pan/zoom canvas across virtual desktops; "pin mode" punches a hole through to the live window |

**Model differences:**
- 1D scrolling (niri, PaperWM, Karousel, Hyprland, komorebi) keeps **one obvious navigation axis**, so keyboard focus left/right is unambiguous and the layout re-flows deterministically.
- 2D canvases trade that for spatial freedom. They need directional "nearest window" search, bookmarks or anchors, zoom-to-fit, and snapping clusters to stay navigable, and windows can overlap by choice.
- niri's maintainer rejected free floating inside the strip for exactly the navigation and "jumping" reasons ([#1116](https://github.com/niri-wm/niri/issues/1116)).

### 2.8 Who ships it (distros)

| Distro | What | Source |
|---|---|---|
| KaOS | **niri + Noctalia replaced Plasma on the ISO from 2026.02** ("Instead, you will find a niri/Noctalia based system", niri 25.11) | [KaOS 2026.02 announcement](https://kaosx.us/news/2026/kaos02/). Later releases 2026.09 (niri 26.04) per [9to5linux](https://9to5linux.com/kaos-linux-drops-kde-plasma-after-12-years-for-niri-noctalia-to-escape-systemd) (secondary) |
| CachyOS | niri offered in the installer ("scrollable tiling… fluid window management") | [CachyOS wiki](https://wiki.cachyos.org/installation/desktop_environments/) |
| Omarchy (DHH) | **Hyprland**; default `layout = "dwindle"` ([`default/hypr/looknfeel.lua#L21`](https://github.com/basecamp/omarchy/blob/7bec2a5894/default/hypr/looknfeel.lua#L21)); user config has a commented-out `layout = "scrolling"` "niri-like side-scrolling layout" ([`config/hypr/looknfeel.lua`](https://github.com/basecamp/omarchy/blob/7bec2a5894/config/hypr/looknfeel.lua)) | branch `quattro`, `7bec2a5894` (2026-11-08) |
| PikaOS, RakuOS, StratOS, Akira, RengeOS… | niri editions or defaults | search-engine summary only **(unverified)** |
| Arch, Fedora, openSUSE | niri in official repos; maintainer COPR (`niri.spec.rpkg` in-tree) | **(unverified)** beyond the in-tree spec |

### 2.9 Model comparison

| | niri | PaperWM | PaperWM.spoon | Karousel | Hyprland `scrolling` | komorebi `Scrolling` | driftwm |
|---|---|---|---|---|---|---|---|
| Kind | compositor | GNOME extension | Hammerspoon script | KWin script | compositor layout | Win32 WM | compositor |
| Strip per | workspace, per monitor | GNOME workspace (shared pool) | macOS space | virtual desktop (1 screen) | workspace | workspace, per monitor | one global 2D canvas |
| Axis | horizontal (vertical requested) | horizontal | horizontal | horizontal | any of 4 directions | horizontal | 2D + zoom |
| Off-screen windows | not rendered; frame callbacks ~1 Hz | clones, clipped; ≤75 px off | parked in edge margin | (single screen) | not rendered | parked 5 px / hidden / clipped | not rendered |
| Lone window | normal width, optional centring | its own width | default_width | centred | **fills screen** by default | ⅓, left | native size |
| Tabbed columns | yes | requested (#1012) | accordion stack | stacked | groups (core) | stack | — |
| Floating | per-workspace layer above | scratch layer (minimizes on hide) | float toggle | float + keepAbove | core floating | float rules | everything floats |
| Native snap | n/a | disabled | n/a | **quick-tile = Pinned; carves space** | n/a | n/a | snap clusters |

---

## 3. Edge-case catalogue

"Best" is my recommendation for a Windows shell, given the prior art.

| Case | niri | Others | Best behaviour for mypaper |
|---|---|---|---|
| **Dialogs (owned/transient)** | Float by default, centred over the parent tile, clamped to the work area; activated only if the parent is active; stacked above the parent; rule can tile them (then right of the parent) ([§1.5](#15-dialogs-popups-transientparented-windows-floating)) | PaperWM/Karousel never tile transients; Hyprland floats parent‖fixed-size; komorebi uses its global float rules | niri's rule: Win32 *owned* windows (`GW_OWNER`) and modal dialogs float centred over the owner, on the owner's workspace, z-ordered with it, gaining focus only if the owner had it. Later, try niri#1116: pin modals to the owner so they scroll with it |
| **Popups, menus, tooltips** | Not windows; constrained horizontally to the window's own width, vertically to the work area; IME gets the parent area | n/a (the platform positions them) | On Win32, menus are separate top-levels (`#32768`, `WS_POPUP`, `WS_EX_TOOLWINDOW`): never manage them. If the owner is scrolled partly off-screen, make sure it lands fully on screen before menus open (auto-scroll on activation) |
| **Fixed-size windows** (calculators, some installers) | Fixed *height* auto-floats; a fixed width alone still tiles, centred in its tile | Hyprland floats if either axis is fixed; PaperWM.spoon ignores non-maximizable windows; Karousel needs resizable; PaperWM tiles them as-is and only moves them | Float when not resizable on either axis (no `WS_THICKFRAME`, or min == max from `WM_GETMINMAXINFO`), with a rule override. Hyprland's either-axis rule is the safer default |
| **Min size bigger than the slot** | Column width = max of the mins (min beats max); a column wider than the view is left-aligned; heights: min wins, the column may overflow | Karousel caps at the tiling-area width; komorebi widens after observing, never shrinks back, never wider than the screen; PaperWM uses the real width and re-lays-out ≤5× | Measure, don't trust: after a resize, read the actual rect and grow the column (komorebi), cap at the work area, left-align when still too wide (niri). Remember a learned min per window |
| **Apps that fight resizes / snap to grid / resize themselves** | 300 ms transaction deadline; window centred in its tile if smaller; resizes ≤10 px don't animate | Karousel follows external resizes but gives up after 4 per second; PaperWM bails after 5 fixpoint tries | Karousel's rate limiter plus komorebi's measure-after-move. Treat a window's own resize as a width request unless it keeps fighting |
| **Fullscreen apps / games** | Fullscreen is a normal tile you can scroll away from; covers floats and the top layer while stationary; new windows don't steal focus; gamescope recommended | Hyprland layout-aware fullscreen; Karousel skips arranging fullscreen windows and toggles keepAbove/Below; driftwm leaves fullscreen on any canvas action | Exclusive-fullscreen / borderless-fullscreen games: **hands off**. Take them out of the strip while fullscreen, suppress the shell overlays, never steal focus (niri's Smart rule). Restore the window to its old column afterwards (niri#426 shows users want this) |
| **Maximize request** | `maximize-window-to-edges` honoured; open-rule can block it at map time, not later | PaperWM's Super+F is width-only; driftwm "fit" | Map `SC_MAXIMIZE` / titlebar double-click to "maximize column to edges" while staying in the strip; restore to the previous width |
| **PiP / always-on-top** | Not supported (#932); default rule floats Firefox PiP; `on-xdg-activate` can stop it stealing focus | driftwm `pinned_to_screen`; Karousel `floatingKeepAbove` option | `WS_EX_TOPMOST` windows: never tile, keep them screen-fixed above the strip, survive workspace switches. Ship PiP rules for Firefox/Chrome |
| **Splash screens** | Fixed-height auto-float; `open-focused false` example for GIMP | Karousel default rule for JetBrains splash | Float windows without a caption/frame that are fixed-size; don't focus them; per-app list |
| **Multi-window apps / same-app new window** | A window with no parent opens right of the focused column, not next to its siblings | PaperWM Super+Return opens a new window of the same app; komorebi: right of focus | Right of the focused column. Siblings stay where they are; "open in the same column" is a rule-driven extra, not the default |
| **New window while in the overview** | Joins the layout; keyboard stays on the overview until it closes (from code) | — | Insert as usual, animate it in inside the overview, and focus it when the overview closes, unless focus-stealing rules say no |
| **Closing the focused column** | Back to the column it was opened from, with the old view, if focus hasn't moved; else the right neighbour; closing to the left keeps the focused window still | komorebi copies the same rule; Hyprland clamps the offset to avoid empty space | Copy niri exactly |
| **Monitor hotplug** | Workspaces move to a survivor and remember their original output (by make/model/serial); restored on reconnect; a new window resets the memory; zero monitors is fine | PaperWM: monitors-changed fires many times, polls 5× when there's no primary; Hyprland guards against zero sizes during reconnect | Copy niri (including the "forget on new window" rule). Debounce `WM_DISPLAYCHANGE` bursts; key outputs by EDID/device path, not index |
| **DPI differences** | Logical px per output; proportions re-resolve; preferred scale sent on move | Per-monitor scaling in each DE | Store widths as proportions (or logical px) per output; resize after `WM_DPICHANGED` lands; expect PerMonitorV2 apps to resize themselves |
| **Very wide windows** (wider than the screen) | Left-aligned; view fit uses left-align | komorebi: starts at the left edge; Karousel caps at the tiling area | Left-align and allow scrolling within the column |
| **Windows requesting their own position** | Impossible on Wayland; X11 apps that position bars "won't behave", so use a nested compositor | PaperWM/Karousel re-lay-out on geometry change; Karousel rate-limits | Win32 apps call `SetWindowPos` freely: treat a move of a tiled window as a no-op and re-apply the layout, with Karousel's rate limiter. Apps that keep doing it are floated by rule |
| **User drags a title bar** | 256 px rubber band, then the window pops out of the layout, semi-transparent with an insert hint; right-click toggles floating vs tiling | Karousel `untileOnDrag` floats it; Hyprland: drop near a target's edge (30 %) = new column, else join the column; PaperWM.spoon lift modifier | niri's rubber band plus an insert hint, Hyprland's edge rule for dropping. On Windows, intercept `WM_ENTERSIZEMOVE` / move-size loops, or show a thumbnail proxy while dragging |
| **Off-screen windows** | Not drawn; ~1 Hz frame callbacks | Clones (PaperWM); edge park (macOS, komorebi 5 px); hide if a monitor is adjacent (komorebi) | Windows *can* place windows off-monitor but shows them on any monitor they overlap. Cloak / hide or park them and render thumbnails for the scroll animation (Apeiron's trick); throttle them |
| **Screen sharing an off-screen window** | Stutters at 1 Hz (#539) | — | Don't throttle windows that are being captured, if detectable (unverified on Win32) |
| **Minimize** | Not supported (#3982) | PaperWM scratch hide = minimize; scroll: optional minimize to scratchpad | mypaper has a dock, so minimize must exist: take the column out of the strip and close the gap (niri's "focused window doesn't move" still applies) |
| **Focus stealing** | xdg-activation tokens with a timeout; Smart: don't steal from fullscreen; `on-xdg-activate` rule | Hyprland `follow_min_visible` 0.4 for soft focus | Treat `SetForegroundWindow` from a background app as urgent (flash in the dock) unless the user just launched it |
| **Focus-follows-mouse at edges** | CSD margins peek out, so accidental focus; `max-scroll-amount="0%"` | Hyprland: mouse-down never scrolls, click-up does | Never scroll on hover; scroll on click-up |
| **Portrait monitors** | Requested (#1071) | Hyprland `direction` up/down; scroll per-output orientation | Per-output strip direction, cheap to add if the layout is axis-agnostic from the start |
| **Native Snap coexistence** | n/a | PaperWM disables edge-tiling; Karousel turns quick-tiled windows into *pinned* carve-outs | Either disable Snap while active (PaperWM) or use Karousel's pin model: a Snap-tiled window shrinks the strip's area |

---

## 4. UX

### 4.1 Keybinding conventions (defaults)

| Action | niri | PaperWM | Karousel | Hyprland `scrolling` |
|---|---|---|---|---|
| Focus column left/right | Mod+H/L, Mod+←/→ | Super+←/→, Super+,/. | Meta+A/D | user-bound `layoutmsg focus` |
| Focus window up/down | Mod+J/K | Super+↑/↓ | Meta+W/S | — |
| Move column / window | Mod+Ctrl+dir | Super+Ctrl+dir | Meta+Shift+dir; column Meta+Ctrl+Shift | `swapcol` |
| Consume / expel | Mod+[ / Mod+], Mod+, / Mod+. | Super+I / Super+O | via move left/right | `consume`, `expel` |
| Cycle preset widths | Mod+R (back: Mod+Shift+R) | Super+R | Meta+R | `colresize +conf` |
| Full width / maximize / fullscreen | Mod+F / Mod+M / Mod+Shift+F | Super+F / — / Shift+Super+F | — | `fit active` / fullscreen |
| Centre column | Mod+C (all visible: Mod+Ctrl+C) | Super+C | Meta+Alt+Return | `center` |
| Workspace down/up | Mod+U/I (Ctrl moves the column there) | Super+PgDn/PgUp; MRU Super+\` | desktop actions | Hyprland workspaces |
| Toggle floating | Mod+V; focus floating/tiling Mod+Shift+V | Super+Ctrl+Esc (scratch) | Meta+Space | `togglefloating` |
| Tabbed / stacked column | Mod+W | — | Meta+X | groups |
| Overview | tap Mod, hot corner, 4-finger swipe | GNOME overview | — | — |

Sources: [niri default-config](https://github.com/niri-wm/niri/blob/90d5c1bf7381/resources/default-config.kdl#L420-L651), [PaperWM README](https://github.com/paperwm/PaperWM/blob/2d460db8365b/README.md), [Karousel README](https://github.com/peterfajdiga/karousel/blob/baeff392e5a5/README.md), [Hyprland wiki](https://github.com/hyprwm/hyprland-wiki/blob/ec1068fe/content/configuring/layouts/scrolling-layout.md).

Pattern worth copying: niri's modifier grammar (focus → **+Ctrl** = move, **+Shift** = across monitors or the height variant). PaperWM's "preview while Super is held, commit on release, Esc aborts" is also good.

### 4.2 Gestures

| | niri | PaperWM | driftwm | komorebi (Windows) |
|---|---|---|---|---|
| 3-finger horizontal | scroll the strip, snap to column, focus it | scroll | pan the canvas | pan via whkd (raw touchpad) |
| 3-finger vertical | workspace switch | down = workspace stack, up = overview | pan | — |
| 4-finger | vertical = overview | — | navigate / zoom-to-fit | — |
| Pinch | per-window `pinch-sensitivity` rule | — | zoom | — |
| Mouse | Mod+MMB drag pans; Mod+wheel workspaces (150 ms cooldown) | wheel on top bar scrolls | Mod+drag pans, wheel zooms | `pan` bound to Alt+wheel |

Windows itself reserves 3/4-finger gestures. komorebi has to read the touchpad itself and tell users to set Windows' own gestures to "nothing" ([komorebi docs](https://github.com/LGUG2Z/komorebi/blob/72aa4c97ad/docs/usage/scrolling-layout.md)).

### 4.3 What users praise and complain about

**Praise:**
- New windows don't disturb existing ones; one or two windows on screen with the rest "scrolled outside" means fewer workspaces are needed ([Workspaces.md § Example workflow](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Workspaces.md), [Design-Principles](https://github.com/niri-wm/niri/blob/90d5c1bf7381/docs/wiki/Development:-Design-Principles.md)).
- The scrolling model "the ideal way to manage windows on large and small screens"; config edits apply on save; mouse and touchpad usable; comfortable after about two days ([LWN, 2025-07-07](https://lwn.net/Articles/1025866/), secondary).
- Being a full compositor rather than a GNOME/KDE add-on is a draw: "would love to replace my usage of PaperWM with something not gnome-based" ([karousel#3](https://github.com/peterfajdiga/karousel/issues/3)); "without having to deal with the bloat that comes with using GNOME" ([niri disc. #3982](https://github.com/niri-wm/niri/discussions/3982)).

**Complaints:**
- Learning curve and static-workspace habits ([#2274](https://github.com/niri-wm/niri/discussions/2274)).
- Missing sticky/PiP windows ([#932](https://github.com/niri-wm/niri/issues/932)) and minimize ([#3982](https://github.com/niri-wm/niri/discussions/3982)).
- Unfullscreen loses the column ([#426](https://github.com/niri-wm/niri/issues/426)).
- Portrait monitors ([#1071](https://github.com/niri-wm/niri/issues/1071)).
- Multi-monitor is the weak point everywhere except niri (PaperWM [#165](https://github.com/paperwm/PaperWM/issues/165) and [#728](https://github.com/paperwm/PaperWM/issues/728); Karousel [#3](https://github.com/peterfajdiga/karousel/issues/3); PaperWM.spoon "arrange screens vertically").
- Extension/plugin WMs break on host upgrades (PaperWM's top issues are GNOME version support; hyprscroller was abandoned over plugin API churn).
- X11 apps and games (niri FAQ/Xwayland, #2386, #2672).

---

## 5. Lessons for a Win32 implementation

These follow from the sources above. The Windows-specific claims are not verified here; another slice covers them.

- mypaper is in the same position as PaperWM, the spoon and komorebi: it **can't stop windows from being visible** where they physically are. So:
  - (a) cloak or park off-strip windows;
  - (b) animate scrolling with **thumbnail proxies** (PaperWM clones, Apeiron's DWM thumbnails), not by moving real HWNDs each frame;
  - (c) move the real windows once, at rest.

  This also fits "RAM/CPU count": nothing re-lays-out per frame.
- **Adopt niri's invariants as tests:** opening a window never resizes the others; the focused window never moves on its own; one remembered view per workspace; proptest over random op sequences including monitor add/remove and arbitrary min/max sizes ([`tests.rs`](https://github.com/niri-wm/niri/blob/90d5c1bf7381/src/layout/tests.rs)).
- **Classify windows at creation with a float heuristic plus a rules list.** Every project ships per-app rules (niri: WezTerm, Firefox PiP; Karousel: 13 defaults).
- **Late changes are a known hole:** maximize after map, titles set after map. React to them (Karousel's `followCaption`) rather than relying only on open-time rules.

## Unverified items (collected)

- papersway's mechanism for hiding off-screen windows.
- Infinidesk and hevel feature details (from search summaries, not repos).
- Non-KaOS/CachyOS distro niri editions (PikaOS, RakuOS, StratOS, Akira, RengeOS…).
- niri's behaviour for a window opened *while the overview is open*: inferred from code, not observed.
- Whether Win32 lets mypaper detect that a window is being captured (§3, screen-sharing row).
