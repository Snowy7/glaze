# Files

`mypaper-files` is an on-demand native file browser built on the mypaper kit. It is its own
process, launched only when a folder is opened in it. Closing its windows releases the process,
workers, thumbnails, OLE registrations and glass requests. It installs no background service.

```powershell
target\release\mypaper-files.exe "C:\Projects"
target\release\mypaper-files.exe "shell:Downloads"
```

The window follows the active panel style, light/dark scheme, accent, font and material from
`config.json`, `bars.json` and the wallpaper. Glass uses the existing wallpaper engine's overlay
wire and a software alpha canvas in our own window; solid styles and an unavailable engine use
the kit's opaque surface. No vendor GPU device is created in Files. `--light` and `--dark` are
run-only appearance overrides, useful when checking a look without changing the desktop.

## Browsing

- Back, Forward, Up, clickable breadcrumbs and an editable address (`Ctrl+L`). Paths, relative
  paths and shell namespace locations are accepted. Home, Desktop, Downloads, Documents,
  Pictures, This PC, mounted drives and pinned folders are available in the sidebar.
- Details has Name, Date modified, Type and Size columns. Click a heading to sort; drag a
  heading boundary to resize. Folders stay together and names use natural ordering.
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

Pins, the chosen view and column widths are stored in `%APPDATA%\mypaper\files.json`:

```json
{
  "pins": ["C:\\Projects"],
  "view": "Details",
  "widths": [400, 190, 170, 110]
}
```

## Search and folder sizes

Files calls only the bounded [index pipe client](index.md), from a background worker:
`mypaper_index::pipe::request(&Request::Search { query, limit: 2000 }, 100)` and
`Request::Sizes { path, limit: 4096 }`. A running index searches its indexed files; otherwise
search filters the current directory. The status bar says **Indexed files** or **This folder**.
Folder sizes are the index's cached logical sizes, not a recursive scan performed by Files.
Missing index results leave directory sizes blank. No index process is started by Files.

## Experimental folder routing

Manual launches work independently. This optional flag writes the per-user folder/Win+E
overrides and adds launch points to the launcher and desktop menu:

```json
{ "experimental": { "files_default": true } }
```

The default is `false`. Core reconciles the flag at startup and on config changes. Enabling
requires `mypaper-files.exe` beside core. No Explorer restart or elevation is requested.

The experiment writes the default command and `DelegateExecute` under these HKCU locations:

- `Software\Classes\Folder\shell\open\command`
- `Software\Classes\Folder\shell\explore\command`
- `Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command`

`%LOCALAPPDATA%\mypaper\undo-folders.json` records original values, registry types, absent
values, created keys and mypaper's last written values **before** applying changes. Disabling
restores a value only if it is still ours; another application's later change is preserved.
Empty keys created by this experiment are pruned. The journal survives a process restart.

For recovery or before removing an installation:

```powershell
target\release\mypaper-files.exe --restore-routing
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

Live checks on 6 November 2026 passed create, rename with extension preservation, shell-format
clipboard copy/cut/paste, recycle, external directory notifications, This PC navigation, indexed
search and OLE copy/Shift-move between two Files windows. Operations touched only private scratch
trees under `C:\mypaper-targets\files\scratch`. Nine Files tests, two folder-routing tests and
the alpha-canvas test pass. Files has no clippy warnings; a separate existing bars hook warning
remains in the dependency build.

The routing check briefly enabled the flag, double-clicked a shortcut to a scratch folder and
opened the deployed Files executable. Disabling restored all six registry values, their types
and key absence exactly, removed the journal, and left the flag off. **Win+E still opened
Explorer on this machine**, including after an association-cache notification and a test with
the reference command's expandable-string type. Its compatibility remains unresolved; the
folder/Win+E experiment is not fully verified.

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

## Remaining work

Win+E compatibility is the outstanding v1 routing issue. Later browser work can add tabs,
preview/archive views, drag edge scrolling and broader network/cloud testing. A shell menu
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
