# Brief: index round 2 (small while you type)

Read `plans/native/LIVE.md`, `docs/index.md` and its evidence (round 1,
up to 8b45c29). Same ownership as round 1 (`briefs/index.md`). Not `crates/shell` beyond the launcher's
index call site.

## Where it is

11.4 M entries over C/D/E, settled 17 MiB private / 8 MiB working set, 0 % idle CPU: good. But
**while the user types a search the working set is ~860-880 MiB** (every query scans every name),
released 2 s after the last query, so the first query after that takes 92-125 ms. During a generation
swap the working set peaks at 1.36 GiB (2.25 GiB in an earlier run). Full scans take 3-4 minutes
per big volume under load.

## Goal (RAM counts as much as CPU)

1. **Index what people search for.** Default excludes for build output, caches and system stores:
   `node_modules`, `target` (when it has a `CACHEDIR.TAG` or `.rustc_info.json`), `.git` internals,
   `__pycache__`, `.venv`/`venv`, `.gradle`, `obj`/`bin` under a `.csproj` folder, `$Recycle.Bin`,
   `System Volume Information`, `Windows\WinSxS`, `Windows\Installer`, `AppData\Local\Temp`,
   browser/package caches (`AppData\Local\**\Cache*`, `npm-cache`, `pnpm-store`, `.cargo\registry`,
   `.rustup\toolchains`), `C:\glaze-targets`. Make it `index.exclude` / `index.include` in
   `config.json` (globs; defaults built in; the user's list layers on top), and apply it during scan
   and to live changes. Folder sizes must still be right: excluded subtrees count toward their
   parent's size as one aggregate (scan their size cheaply or mark them "excluded, size unknown";
   pick the honest option and document it).
2. **A search structure that touches little memory.** Queries should read a compact, case-folded
   names-only column (plus a small prefilter such as per-block character bitmaps or trigram
   postings) instead of every record. Targets: active query working set <= 150 MiB on the full
   remaining index, first query after an idle trim <= 40 ms, warm full-pipe p95 <= 20 ms.
3. **Generation swap peak working set <= 256 MiB** (stream it, don't map whole generations).
4. Re-measure everything round 1 measured, on the live machine, and update `docs/index.md`.

## How / stop

As round 1: tests for the pure parts (exclude matching, size aggregation, prefilter correctness:
it may never drop a true match), live measurements with the deployed build, desktop lock one action
at a time. If the user's foreground app (T3 Code) blocks guarded launcher captures, use the pipe CLI
for evidence and say so. `CARGO_TARGET_DIR=C:\glaze-targets\index`. Stop when the targets are met or
after 3 hours. Report: numbers before/after, the exclude list, commits.
