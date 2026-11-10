# Brief: files-app round 2 (an Explorer you'd actually switch to)

Read `plans/native/LIVE.md`, `docs/files.md` (v1, commits a2d74f7..c0d6b41),
`docs/ui.md` (The kit), `docs/glass.md`. Same ownership as round 1 (`briefs/files-app.md`), plus the small
Win+E edit below. `crates/shell` is also being edited by the shell-idle agent: keep your shell edits
tiny, in separate functions, and commit them immediately.

## Goal

1. **Win+E** opens Glaze Files when `experimental.files_default` is on: Glaze's own Win-key hook
   (`crates/shell/src/flyouts` winkey provider; it runs while the taskbar is hidden, which is the user's
   setup) catches Win+E and opens Files, and passes it through untouched when the flag is off or the
   hook isn't installed. The registry route stays for folder double-clicks. Test with the flag on
   briefly, then put it back off, restoring the journal exactly as round 1 did.
2. **Tabs**: Ctrl+T / Ctrl+W / Ctrl+Tab, middle-click a folder to open it in a new tab, drag a tab out to
   a new window, tabs restored on reopen (`files.json`). Calm kit tabs, no outlines on glass.
3. **Preview pane** (Alt+P): images, text/code (first 64 KB, monospace), and the shell's preview
   handlers for others where it's cheap. **Quick look** (Space): a large preview overlay; Space/Esc
   closes it, arrows move between files.
4. **Polish what v1 shows**: the toolbar's circled icon buttons look heavy: make them quiet kit
   icon buttons (no circles at rest, a soft fill on hover), with tooltips. Size column for folders from
   the index (round 1 has cached sizes: show them in Details with a subtle bar). "Open in Terminal"
   and "Copy path" in the menu. Status bar: selection count and size.
5. **Speed**: opening a folder shows its first rows within one frame of the click for cached folders.
   Measure first paint for a 10k folder and a 100k folder, and say what you measured.

Cost rules unchanged: 0 when closed, 0 % CPU when idle, report private MB for 10k entries with two
tabs and the preview pane open.

## How / stop

As round 1: scratch folders only, DPI-aware captures (Details, Icons, tabs, preview, quick look, light
and dark) that you look at yourself, tests for the pure parts. `CARGO_TARGET_DIR=C:\glaze-targets\files`.
If the shared build is broken by someone else's in-progress file, wait and retry; don't edit it.
Stop when done, or after 3 hours. Report as round 1.
