# Files

`glaze-files` is an on-demand native file browser built on the Glaze kit. It is its own
process, launched only when a folder is opened in it. Closing its windows releases the process,
workers, thumbnails, OLE registrations and glass requests. It installs no background service.

```powershell
target\release\glaze-files.exe "C:\Projects"
target\release\glaze-files.exe "shell:Downloads"
```

The window follows the active panel style, light/dark scheme, accent, font and material from
`config.json`, `bars.json` and the wallpaper. Glass uses the existing wallpaper engine's overlay
wire and a software alpha canvas in our own window; solid styles and an unavailable engine use
the kit's opaque surface. No vendor GPU device is created in Files. `--light` and `--dark` are
run-only appearance overrides, useful when checking a look without changing the desktop.
`--no-activate` opens a test instance without changing keyboard focus.

Hiding or minimizing Files suspends its overlay capture without unregistering the plate or
discarding its last composition surface. Restore shows that retained frame immediately,
then starts capture again; it does not replay the initial glass fade. Before the first glass
frame, the software canvas shows the theme's flat surface tint. An engine readiness message
replaces it with glass once, without polling. Edge hover/pressed washes are clipped by stillkit
to the same surface radius.

The input-free [resume comparison](../crates/files/evidence/drive-glass/resume.json) uses three
trials per condition, hiding for 3.8 seconds so the former capture path is cold. Median first
glass was 304.4 ms idle / 534.3 ms with four busy CPU workers; the retained frame appeared in
7.1 / 5.7 ms, and fresh glass in 47.3 / 48.8 ms. These are owned 640×420 overlay measurements
including `DwmFlush`, comparing the former Close/Open lifecycle with Visibility suspension.
The [This PC capture](../crates/files/evidence/drive-glass/this-pc.png) and
[corner detail](../crates/files/evidence/drive-glass/hover-corner.png) use the production view
in a no-activate fixture, verified by PrintWindow and UIA on its own PID.
The [production Files check](../crates/files/evidence/drive-glass/live-resume.json) returned its
retained frame in 9.9 ms after hiding and 7.5 ms after minimizing, without activation and with
the same plate HWND. Plate discovery verifies the engine PID, class and client property;
it tolerates hidden IME helpers between the client and its backdrop.

The final shared release deployment passed the stale-input guard and matched all four deployed
binary hashes. All four config backups were restored byte for byte
([deployment verification](../crates/files/evidence/drive-glass/deployed.json)). Shell private
memory was 20.95 MiB before / 20.69 MiB after. The 30-second CPU samples were 0.16% / 0.36% of one
core amid concurrent dock changes; this does **not** establish the full shell performance gate.
Bar redraw median/p95 and menu-open/startup/wake budgets still need an isolated comparison.

## Browsing

- Tabs have independent history, scroll, selection and search. `Ctrl+T` opens a tab, `Ctrl+W`
  closes it, and `Ctrl+Tab` / `Ctrl+Shift+Tab` switch tabs. Middle-click a folder to open it in
  a tab; middle-click a tab to close it. Drag a tab out of its strip to make a new window.
  Launching without a path restores up to 32 saved tabs and the active tab; an explicit path
  opens that folder in a fresh window. Only the active tab has a directory watcher.
- Back, Forward, Up, clickable breadcrumbs and an editable address (`Ctrl+L`). Paths, relative
  paths and shell namespace locations are accepted. Home, Desktop, Downloads, Documents,
  Pictures, This PC, mounted drives and pinned folders are available in the sidebar.
- Details has Name, Date modified, Type and Size columns. Click a heading to sort; drag a
  heading boundary to resize. Folders stay together and names use natural ordering.
- This PC always uses quiet drive tiles, with a capacity bar and available/total space.
  Each drive's `GetDiskFreeSpaceExW` query runs on an independent one-shot worker, so a
  slow network mapping or removable device cannot delay other capacities or navigation.
  The accent bar turns red at 90% used; unavailable media says **Space unavailable**.
- Icons requests shell thumbnails through `IShellItemImageFactory`. Shell display names, type
  descriptions and icons are also fetched in the background. A bounded 384-entry decoration
  cache, additionally capped at 16 MiB, keeps scrolling from retaining thumbnails for every
  visited file.
- Folder enumeration uses `FindFirstFileExW` metadata without opening every child. The first
  128 entries appear before enumeration finishes. Enumeration, sorting, search and shell image
  extraction run off the drawing thread. Both views build paint operations only for visible
  rows, with one extra row at each edge.
- Hidden and protected files follow Explorer's current `Hidden` and `ShowSuperHidden` settings.
- Native directory notifications refresh the open folder after external changes. The watch
  sleeps on kernel events and closes on navigation; there is no polling timer.
- Every row replacement remaps selected items, keyboard focus and the Shift-selection anchor
  by path. Refresh, sorting and search cannot redirect Enter or previews to a shifted row;
  a missing focused path clears focus. Tabs restore selection with their matching snapshot,
  then reconcile paths on refresh. A replacement cancels an item/rubber-band gesture and closes
  the kit context menu, whose hit targets belonged to the old layout. Rename carries its path
  and is dismissed if that path disappears. Thumbnail requests/replies use `(path, size)` keys
  and navigation epochs, and OLE drag sources snapshot paths before entering the drag loop.
  The [focused regression](../crates/files/evidence/selection-refresh/regression.json) first
  reproduced Enter resolving to `b.exe` instead of `c.txt`, then passed insertion, sorting,
  range-anchor and removal cases. All 20 Files tests pass with an isolated scratch profile.
  After the shared release deployment, a live scratch fixture used harmless directories named
  `a.txt`, `b.exe`, `c.txt` and an externally inserted `0setup.msi`. Inspected DPI-aware captures
  show [the original selection](../crates/files/evidence/selection-refresh/select.png),
  [the watcher refresh](../crates/files/evidence/selection-refresh/refreshed.png), and
  [Enter navigating into c.txt](../crates/files/evidence/selection-refresh/enter.png).
  The [deployment hashes](../crates/files/evidence/selection-refresh/deployed.json) match the
  approved `C:\glaze-targets\main` build. The fixture's separate profile leaves the user's
  Files session/preferences untouched; desktop actions hold the lock individually.
  A later shared deployment replaced the on-disk executables after the Enter check. The
  [preserved running image](../crates/files/evidence/selection-refresh/shared-deployment.json)
  hashes exactly to this test's deployed Files executable; the test process then closed normally.
- Arrow keys, Home/End, Page Up/Down, type-to-select, Enter, Backspace, Alt+arrows, mouse Back/
  Forward, Ctrl/Shift selection and rubber-band selection are supported. `F5` refreshes.

`Ctrl+C/X/V`, `F2`, `Delete`, `Shift+Delete` and `Ctrl+Shift+N` use Windows' `IFileOperation`.
Rename selects the filename stem and preserves its extension unless the user edits it.
Windows owns progress, conflicts, elevation, recycle behavior and undo records. The clipboard
uses the shell's `IDataObject`, including its standard file-drop formats and preferred drop
effect. Dragging uses OLE `DoDragDrop` and an `IDropTarget` on our window.

The kit context menu puts common actions first. **Show more options** opens the actual shell
`IContextMenu`, with dynamic submenus and owner-drawn extensions. A search selection spanning
different parent folders uses the first item's shell menu, because there is no shared parent.

Pins, the chosen view, column widths, tabs, active tab and preview preference are stored in `%APPDATA%\glaze\files.json`:

```json
{
  "pins": ["C:\\Projects"],
  "view": "Details",
  "widths": [400, 190, 170, 110],
  "tabs": ["C:\\Projects", "shell:Downloads"],
  "active_tab": 0,
  "preview": true
}
```

## Search and folder sizes

Files calls only the bounded [index pipe client](index.md), from a background worker:
`glaze_index::pipe::request(&Request::Search { query, limit: 2000 }, 250)` and
`Request::Sizes { path, limit: 4096 }`. A running index searches its indexed files; otherwise
search filters the current directory. The status bar says **Indexed files** or **This folder**.
Folder sizes are the index's cached logical sizes, not a recursive scan performed by Files.
Missing index results leave directory sizes blank. No index process is started by Files.
Both calls have a 250 ms deadline. A response with `scanning: true` contains cached results
while the helper reconciles its snapshot. Sizes includes the requested folder followed by
its immediate children; Files associates each logical byte count with the returned path.

Index sizes retain the `Hit.size_unknown` and `Hit.excluded` flags in the browser's entries,
folder cache and inactive tabs. A lower bound reads **≥ 4.2 GB**; an excluded folder reads **—**
with the tooltip **Not indexed (excluded)**. Only exact sizes get a relative size bar. Size
sorting puts exact sizes before unknown sizes in both directions, including when an uncertain
folder would otherwise precede an exact file. Indexed search hits take precedence over older
cached metadata, and local search retains the current folder's size flags.

Selection totals also preserve uncertainty: exact and lower-bound byte counts are added, but
any lower-bound, excluded or unmeasured item makes the total a lower bound. A selection containing
only excluded folders reads **—**. No recursive work is started to fill these gaps.

The internal value is `model::Size::{Exact(u64), LowerBound(u64), Excluded}`; missing index
results remain `None`. Focused pure tests cover flag conversion, labels, exact-only bar inputs,
both sorting directions, metadata precedence and mixed selection totals. The manual
`glaze-files-size-proof` example renders the production view with isolated metadata, independent
of the live helper's current generation; run it only under the desktop lock. The inspected
[capture](../crates/files/evidence/size-flags/exact-lower-excluded.png) shows all three states,
the excluded tooltip, and a lower-bound selection total.

## Previews and toolbar

`Alt+P` toggles the preview pane. Images use a bounded 768-pixel shell thumbnail; text and code
read only the first 64 KiB, recognize UTF-8 and UTF-16 BOMs, and render in Cascadia Mono. Long
lines wrap to the pane and the text scrolls. Preview reading and image extraction run on a lazy
worker, released when the preview closes. No content is executed.

Other formats try the registered `IPreviewHandler` through `CLSCTX_LOCAL_SERVER`, using the
shell's surrogate, read-only initialization and `SetWindow`/`SetRect`. Files never loads a
third-party preview DLL into its own process. A missing or non-surrogated handler leaves an
unavailable-preview message. The PDF handler works on this machine; its Word RTF handler does
not offer this route. The retained handler is unloaded on selection change or close.

`Space` opens a large quick-look overlay. `Space` or `Esc` closes it; arrow keys and the overlay's
buttons move between files, skipping directories. The preview content is shared with the pane.

Toolbar icons use the kit's quiet icon button: no circle or outline at rest, a rounded hover wash,
and tooltips. The menu includes **Copy path** (selected paths, or the current location) and
**Open in Terminal** (Windows Terminal's `-d` argument). Selection count and known bytes appear
in the status bar; selections with uncached directory sizes explicitly say so. Details shows a
subtle relative size bar for the index's cached directory sizes.

A bounded folder cache holds up to eight locations / 120,000 entries using shared row snapshots.
Returning to a cached folder paints its rows immediately, then reconciles directory contents on
the background worker. Failed loads do not enter the cache. Inactive tab snapshots retain their
own rows until that tab closes; images remain bounded by the separate decoration cache.

## Experimental folder routing

Manual launches work independently. This optional flag writes the per-user folder/Win+E
overrides and adds launch points to the launcher and desktop menu:

```json
{ "experimental": { "files_default": true } }
```

The default is `false`. While the existing shell Windows-key hook is installed (native taskbar
hidden and the launcher Windows-key option enabled), Win+E launches Files directly. Its atomic
gate follows config changes and also checks for the Files executable. With the flag off, other
modifiers held, a fullscreen app, a hung shell, or no installed hook, the chord passes to Windows.
The folder registry route remains for folder double-clicks.

Core reconciles the flag at startup and on config changes. Enabling
requires `glaze-files.exe` beside core. No Explorer restart or elevation is requested.

The experiment writes the default command and `DelegateExecute` under these HKCU locations:

- `Software\Classes\Folder\shell\open\command`
- `Software\Classes\Folder\shell\explore\command`
- `Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command`

`%LOCALAPPDATA%\glaze\undo-folders.json` records original values, registry types, absent
values, created keys and Glaze's last written values **before** applying changes. Disabling
restores a value only if it is still ours; another application's later change is preserved.
Empty keys created by this experiment are pruned. The journal survives a process restart.

For recovery or before removing an installation:

```powershell
target\release\glaze-files.exe --restore-routing
```

This disables the config flag and restores the journal. `scripts/package/unregister.ps1` runs
it before unregistering, and refuses to proceed with a pending journal if Files is missing.
Pass `-AppDirectory <installed directory>` for an installation outside this checkout.
Windows provides no supported default-file-manager selector: this HKCU workaround does not
replace common file dialogs or applications that explicitly launch `explorer.exe`.

## Verification

Focused tests cover history/selection, virtualization, natural names, path/name validation,
shell copy/move/rename/create/recycle in a private scratch tree, and ownership-aware journal
restoration. The kit's alpha-canvas test checks that native client geometry stays unchanged
across alpha rendering and a return to the opaque path.

Round-one live checks on 6 November 2026 passed create, rename with extension preservation, shell-format
clipboard copy/cut/paste, recycle, external directory notifications, This PC navigation, indexed
search and OLE copy/Shift-move between two Files windows. Operations touched only private scratch
trees under `C:\glaze-targets\files\scratch`. The original nine Files tests, two folder-routing tests and
the alpha-canvas test pass. Files has no clippy warnings; a separate existing bars hook warning
remains in the dependency build.

The routing check briefly enabled the flag, double-clicked a shortcut to a scratch folder and
opened the deployed Files executable. Disabling restored all six registry values, their types
and key absence exactly, removed the journal, and left the flag off. **Win+E still opened
Explorer on this machine**, including after an association-cache notification and a test with
the reference command's expandable-string type. Those round-one observations left the
registry-only Win+E route unresolved; the round-two hook check below closes that issue.

The [interaction record](../crates/files/evidence/verification.json),
[OLE record](../crates/files/evidence/ole.json),
[search record](../crates/files/evidence/search.json),
[routing record](../crates/files/evidence/routing.json) and
[cost record](../crates/files/evidence/cost.json) retain the results.

| Observation | Result |
|---|---:|
| Closed Files processes | 0 |
| 10k-entry folder, 30-second idle CPU, percent of one core | 0.00% |
| Same sample, private memory / working set | 27 / 59 MiB |
| Same sample, Files GPU engines / GPU memory | 0 / 0 MiB |
| 10k folder, first 128 / complete enumeration | 54 / 62 ms |
| 10k folder after UI Automation interaction, private memory | 31.64 MiB |
| 100k folder, first 128 / complete enumeration | 259 / 326 ms |
| 100k Details view, accessible visible file rows | 17 |

These are individual observations on this desktop with warm filesystem caches and other builds
running, not latency percentiles. The idle sample used the night plan's `measure.ps1 -Seconds 30`.
GPU counters describe the Files process; its glass is supplied by the existing wallpaper engine.
All test instances ran from a private executable copy, and were closed afterward.

DPI-aware captures, inspected against the kit's flat glass treatment:

- [Dark Details](../crates/files/evidence/details-dark.png)
- [Light Details](../crates/files/evidence/details-light.png)
- [Light Icons](../crates/files/evidence/icons-light.png)
- [Dark thumbnail](../crates/files/evidence/thumbnails-dark.png)
- [100k folder at the last row](../crates/files/evidence/100k-end.png)
- [Common context menu](../crates/files/evidence/context-dark.png)
- [Indexed search and cached sizes](../crates/files/evidence/index-search.png)
- [OLE destination after copy and move](../crates/files/evidence/ole-destination.png)

## Round-two verification (6 November 2026)

The focused suite now has **14 Files tests** (including restore defaults/bounds, cache invalidation,
64 KiB decoding and Unicode wrapping) and **8 Windows-key machine tests**. Files checks and clippy
pass. The full workspace release build completed after the other agents' kit edits settled.

Win+E **passed with the flag on** through the installed shell hook; folder-shortcut double-clicks
opened Files through the registry route. With the flag off, Win+E opened Explorer and created no
Files process. All six registry values/types and key absence matched the pre-test snapshot; the
journal was removed and the flag left off. The [routing record](../crates/files/evidence/round2/routing.json)
and [final state](../crates/files/evidence/round2/final-state.json) retain those assertions.

Live scratch-folder checks passed Ctrl+T/W/Tab/Shift+Tab, middle-click folder opening, tab tear-out,
two-folder session restore, text/code and image previews, quick-look arrows/Space/Esc, and the shell's
PDF preview handler. The RTF handler was unavailable through the surrogate and showed the expected
fallback. Copy path was checked against clipboard text (then restored), long previews were scrolled
to their last line, and the nonzero cached folder-size bar was inspected. Terminal menu presence
and alias resolution were checked; launching a terminal was not exercised during UI automation.
Test windows and the preview surrogate were gone after close; preferences were restored.

| Measured observation | Result |
|---|---:|
| 10k entries in each of two tabs, preview open; 30-second idle CPU (one core) | **0.00%** |
| Same sample, private / working set | **31 / 62 MiB** |
| Same sample, Files GPU activity / GPU memory | **0 / 0 MiB** |
| Closed Files processes / preview surrogates | **0 / 0** |
| 10k first visit in an open window, first 128 / full enumeration | 3 / 11 ms |
| 10k first content paint / cached Back paint | **8.2 / 4.3 ms** |
| 100k first visit, first 128 / full enumeration | 202 / 268 ms |
| 100k first content paint / cached Forward paint | **207.1 / 3.4 ms** |
| 100k cached tab switch; visible rows drawn | 7.5 ms; 17 rows |

Paint timestamps run from navigation entry to software display-list presentation, including the
worker and UI queue; they are not enumeration-only timestamps. Cached rows appear before background
reconciliation completes. These are individual desktop observations with uncontrolled filesystem
caches and background builds, not percentiles or a cold-process startup guarantee. The idle sample
used the night plan's 30-second `measure.ps1`. See the [cost](../crates/files/evidence/round2/cost.json),
[latency](../crates/files/evidence/round2/latency.json),
[tear-out](../crates/files/evidence/round2/tear-out.json),
[session](../crates/files/evidence/round2/session-saved.json), and
[closed](../crates/files/evidence/round2/closed.json) records.

DPI-aware captures inspected directly:

- [Dark Details/code preview](../crates/files/evidence/round2/details-preview-dark.png)
- [Dark tabs](../crates/files/evidence/round2/tabs-dark.png)
- [Middle-click folder tab](../crates/files/evidence/round2/middle-folder-tab.png)
- [Dark image preview](../crates/files/evidence/round2/image-preview-dark.png)
- [Light Icons/image preview](../crates/files/evidence/round2/icons-preview-light.png)
- [Light Details/code preview](../crates/files/evidence/round2/details-preview-light.png)
- [Dark Icons](../crates/files/evidence/round2/icons-dark.png)
- [Dark code quick look](../crates/files/evidence/round2/quick-look-code-dark.png)
- [Dark image quick look](../crates/files/evidence/round2/quick-look-image-dark.png)
- [Light quick look](../crates/files/evidence/round2/quick-look-light.png)
- [Wrapped text at the last line](../crates/files/evidence/round2/long-line-end.png)
- [Cached folder-size bar and selected bytes](../crates/files/evidence/round2/folder-size-bar.png)
- [Context menu](../crates/files/evidence/round2/context-dark.png)
- [Shell PDF preview](../crates/files/evidence/round2/shell-handler-pdf.png)
- [10k entries, two tabs and preview](../crates/files/evidence/round2/10k-two-tabs-preview.png)
- [100k Details](../crates/files/evidence/round2/100k-details.png)

## Remaining work

The registry-only Win+E issue recorded above is addressed by the opt-in shell hook.
Further work can add archive views, drag edge scrolling and broader network/cloud testing. A shell menu
for selections across different parent folders currently uses the first item. Search is by
filename through the index, with current-folder filtering when its pipe is unavailable.

## Implementation commits

The workspace crate is `a2d74f7`; the browser/services are `7dce8b2`; configuration is `046f7a2`;
routing and ownership-aware undo are `e4d8770`; launcher routing is `fad7184` (preserved through
the index agent's `404bd02` formatting); deployment, desktop-menu and the small core config
entry point are `057d7ac`; unregister recovery is `3bed611`.

The opt-in app alpha canvas is `14a9ff9`, glass integration is `4de4f43` and activation handling
is `0630d18`. Follow-up fixes include modifier snapshots `aa7ee2d`, shell path normalization
`139afda`, look/DPI/size sorting `9b239e5`, directory notifications `c5fd09a`, accessibility
`fec2247`, thumbnail bounds `895042b`, extension-preserving rename `d878940` and operation/
navigation ordering `f2c7a50`. The additive RichEdit helpers are `3c48247` and `06eb79d`.

Outside the brief's main crate, the core change only reconciles the routing journal at startup
and config reload; package unregistration restores it before removing identity. Studio was not
edited.


Round two: `cf7214f` adds Win+E to the existing hook in `crates/bars/src/flyouts/winkey.rs` and
its tiny shell dispatch/config gate. `76e7c63` adds the kit's
`Frame::quiet_icon_button(id, rect, icon, label, enabled, selected)` without changing old components.
`8e22dba` adds tabs, restore, the bounded folder cache and image/text previews; `33255b2` adds
surrogated shell previews. Follow-ups are `6b61233` (capture/readability), `1f18b7a` (text wrapping /
failed-cache exclusion), `766e928` (complete snapshots / selected-folder terminal target),
`f11396a` (Terminal alias discovery), and `f6756ad` (unique preview-close accessibility ID).

The hook exposes `WinKey::set_files_enabled(bool)` and the pure
`Machine::on_key_with_files(vk, down, ours, files) -> Action`; `Target::Files` reuses the hook's
mask/release/cancellation path. The only outside-main-crate edit for that route is the existing
provider in `crates/bars`; no core, config or Studio source change was needed in this round.
The 250 ms index pipe deadlines from the coordination change are preserved.

The workspace was deployed before the 06:16 hold. Afterward only the closed, on-demand Files
executable received its final small fixes; core and index were not restarted. The
[installed record](../crates/files/evidence/round2/installed.json) identifies that executable and
confirms unchanged core/index PIDs. All test windows were closed and the user's original Files
preferences were restored exactly.
