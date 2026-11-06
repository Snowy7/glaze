# Brief: index (instant file search and folder sizes)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/plugins.md` (how `mypaper-plugins` is
started only when needed; copy that lifecycle), and the launcher (`crates/shell/src/flyouts`, parity
rows L01-L05, `launcher.files`).

## Goal

An Everything-class file index that makes the Start launcher's file search instant, plus folder sizes,
at almost no cost:

- **New crate `crates/index`**: a library plus the binary `mypaper-index`, its own process, started by
  core **only** when `experimental.file_index` is true in `config.json` (add that field, default
  `false`, committed on its own) and stopped when it turns false. Off = no process.
- **Scan without admin**: fixed NTFS/ReFS volumes, parallel enumeration with the fastest non-admin API
  you can measure (`NtQueryDirectoryFile` / `FindFirstFileExW` + `FIND_FIRST_EX_LARGE_FETCH` +
  `FindExInfoBasic`, or `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`); background I/O and
  CPU priority (`THREAD_MODE_BACKGROUND_BEGIN`); skip reparse points; record name, parent, size,
  modified time, attributes. MFT/USN needs admin: leave a `TODO` note, don't build it tonight.
- **Live updates**: `ReadDirectoryChangesExW` per volume root (recursive, large buffer); on overflow,
  rescan only the affected subtree. Idle = 0 % CPU: no polling timers.
- **Compact storage**: one name blob plus fixed-size records, case-folded names for search, written to
  `%LOCALAPPDATA%\mypaper\index\` so a restart loads in well under a second instead of rescanning.
  Budget: <= 40 MB private per million entries, and say what you measured.
- **Queries** over a named pipe (`\\.\pipe\mypaper-index`, current user only): substring and
  word-prefix match on names, `ext:` / `path:` / `folder:` / `file:` filters, ranked (exact, prefix,
  recently modified), first results in <= 10 ms on a million entries; a `mypaper-index --query <q>`
  CLI for testing.
- **Folder sizes**: every directory carries its subtree's logical size and file count, kept current
  incrementally; a pipe request returns them for a path and its children (biggest first).
- **Launcher**: when the index is running, the launcher's Files results come from it (instant, any
  file, a folder shows its size); otherwise today's recent-file search stays. Additive change in
  `crates/shell/src/flyouts`, nothing else of the launcher changes.

## You own

`crates/index` (new), additive edits in `crates/config` (the flag), `crates/core` (start/stop the
process, one small commit), the launcher's file-search source in `crates/shell/src/flyouts`, and a new
`docs/index.md`. Add the crate to the workspace in the root `Cargo.toml` (one line, its own commit).

## How

- Tests for the pure parts: the record store, case folding, the query parser and ranking, size
  roll-up, change application.
- Measure on this machine: full scan time per volume, entries, private MB, query latency (p50/p95),
  idle CPU over 60 s with `measure.ps1` (add `mypaper-index` to it if its `mypaper-*` match misses it),
  and the cost of a burst of file changes (e.g. a `cargo build`).
- Use `CARGO_TARGET_DIR=C:\mypaper-targets\index` for check/test; `C:\mypaper-targets\main` only for
  the deploy build. Turn the flag on in the user's `config.json` only once it's stable and measured,
  and say so in the report (they can switch it off).

## Stop when

Search and sizes work live from the launcher and the budgets hold, or after 3 hours. Commit as you go.

## Report

Design, measured numbers, what's left (admin MFT path, a Files window, Studio toggle), commits.
