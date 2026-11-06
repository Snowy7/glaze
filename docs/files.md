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
  cache keeps scrolling from retaining thumbnails for every visited file.
- Folder enumeration uses `FindFirstFileExW` metadata without opening every child. The first
  128 entries appear before enumeration finishes. Enumeration, sorting, search and shell image
  extraction run off the drawing thread. Both views build paint operations only for visible
  rows, with one extra row at each edge.
- Hidden and protected files follow Explorer's current `Hidden` and `ShowSuperHidden` settings.
- Arrow keys, Home/End, Page Up/Down, type-to-select, Enter, Backspace, Alt+arrows, mouse Back/
  Forward, Ctrl/Shift selection and rubber-band selection are supported. `F5` refreshes.

`Ctrl+C/X/V`, `F2`, `Delete`, `Shift+Delete` and `Ctrl+Shift+N` use Windows' `IFileOperation`.
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

Manual launches work independently. This optional flag routes ordinary folder opens and Win+E
to Files, and adds launch points to the launcher and desktop menu:

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

Live captures and cost measurements are recorded after the final desktop check.
