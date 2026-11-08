# Brief: files-app (our own file browser, experimental)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/ui.md` (The kit), `docs/glass.md`,
and `docs/research/explorer-and-files.md` §1 and §3 (why: Explorer's file list and nav tree are
DirectUI/Win32 and can't be restyled without paint hooks; a browser we own can look however the user
wants, with no injection).

## Goal

`glaze-files`: a native file browser built on the kit, launched on demand (its own process,
nothing resident when closed), that looks like the rest of Glaze and follows the active look
(tokens, glass/solid surface, light/dark, family). Tonight's scope is a solid, fast, good-looking
v1, not Explorer parity:

- Window with our title area, back/forward/up, a breadcrumb address bar you can also type into
  (paths and `shell:` locations), a search field, and a sidebar (Home, Desktop, Downloads,
  Documents, Pictures, drives, pinned folders saved in `%APPDATA%\glaze\files.json`).
- The folder view: Details (name, modified, type, size; sortable; resizable columns) and Icons
  (thumbnails through `IShellItemImageFactory`, off the UI thread, cached). Real shell names and
  icons (`IShellItem`, `SHGetFileInfo` / system image list), hidden files per Explorer's setting.
  Large folders (100k entries) must open and scroll smoothly: virtualize rows, enumerate off the UI
  thread, show the first page at once.
- Keyboard and mouse like Explorer: arrows, type-to-select, Enter, Backspace, F2 rename, Delete
  (Recycle Bin) / Shift+Delete, Ctrl+C/X/V, Ctrl+Shift+N, Ctrl+L, Alt+arrows, multi-select with
  Ctrl/Shift/rubber band, drag and drop in and out (OLE `IDropTarget` / `DoDragDrop`).
- File operations through `IFileOperation` (Windows' own progress, conflicts and undo), so copies
  behave exactly like Explorer's.
- The right-click menu: our kit menu with the common verbs first, then "Show more options" with the
  shell's own `IContextMenu` for everything else.
- Search: when `glaze-index` is running (another agent builds it tonight: `crates/index`,
  `docs/index.md`, pipe `\\.\pipe\glaze-index`), search and folder sizes come from it; otherwise
  search filters the current folder. Only use its pipe protocol; don't edit `crates/index`.
- Experimental "Open folders with Glaze Files": `experimental.files_default` in `config.json`
  (default `false`) writes the per-user HKCU folder/Win+E keys from the research (§3), journaled
  through the existing undo journal in `crates/tweaks` (value, type and absence; compare before
  restore), and restores them when switched off or on uninstall. Implement and unit-test the
  journaling; turn it on live only briefly to verify Win+E and a folder double-click, then switch it
  back off.
- Launch points: the launcher's Files/folder results and the desktop menu can open folders in it
  (additive, behind the same flag).

Cost: 0 when closed; when open and idle, 0 % CPU (no timers), and say what private memory it takes
with a 10k-entry folder open.

## You own

New `crates/files` (binary `glaze-files`; add it to the workspace in its own commit), additive
`crates/ui` components (new only, per LIVE.md: other agents use the kit too), additive fields in
`crates/config`, a small additive entry point in `crates/tweaks` for the folder-routing journal, the
launch hooks in `crates/shell/src/flyouts` and `crates/shell/src/desktop_menu` (additive, small
commits), `scripts/deploy-local.ps1` (bundle the exe, additive), and a new `docs/files.md`. Not
`crates/studio`.

## How

- `CARGO_TARGET_DIR=C:\glaze-targets\files` for check/test; `C:\glaze-targets\main` for deploy
  builds.
- Look at DPI-aware captures of your own window (Details, Icons, light and dark, a big folder, the
  menu) before calling anything done, and compare them with the bars and flyouts so it looks like
  the same product: calm, flat translucent tint, no outlines on glass, no gloss.
- Test file operations only inside a scratch folder you create (e.g. `C:\glaze-targets\files\scratch`),
  never in the user's own folders.

## Stop when

v1 above works live and looks right, or after 3.5 hours. Commit as you go.

## Report

What works, captures (paths), measured cost, what's left for v2, commits.
