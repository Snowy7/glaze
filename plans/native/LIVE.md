# Working live on `native` (shared checkout)

From 2026-11-05 every agent works directly in `D:\Coding\projects\desktop\wallpaper` on branch
`native`, several at once. No worktrees, no side branches, no merges. These rules replace the
branch/merge parts of `IMPLEMENTING.md`; everything else there (YAGNI, destructive rebuild, desktop
lock, leave the desktop restored, kill headless browsers) still applies.

## Sharing one checkout

- Edit only the paths your brief says you own. If you need a change elsewhere, make the smallest
  additive change (new function, new field with a default) and say so in your report.
- Never run `git stash`, `git reset`, `git checkout -- <path>`, `git clean`, `git rebase` or
  anything else that rewrites files or history you don't own. Never revert someone else's edit.
- Commit only your own paths, and commit often (every working step):
  `git commit -m "<type>(<area>): <what>" -- <path> <path> ...`
  (the `--` form commits those paths whatever else is staged; `git add` new files first). No push.
- Keep every saved state compiling. Change shared APIs additively first, migrate callers, then
  remove the old API in the same commit series. If `cargo check` fails in files you don't own,
  wait a minute and retry; don't fix them.
- `cargo check -p <crate>` / `cargo test -p <crate>` for the crates you touch. Cargo serializes on
  the target lock; that's expected.

## Seeing it live

- The user's desktop runs `target\release\*` from this repo. To put your work live:
  `$env:CARGO_TARGET_DIR = "C:\mypaper-targets\main"; cargo build --release --workspace`, then
  `powershell -NoProfile -File scripts/deploy-local.ps1` (takes the desktop lock, restarts
  mypaper, re-registers identity). Other agents' committed and uncommitted work ships too; that's
  fine.
- Use the desktop lock (`C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock`) for any
  synthetic input or screenshots, hold it briefly, and leave mypaper running the latest build when
  you release it.
- Verify visually with DPI-aware screenshots (PNG) of the live desktop. Look at them yourself
  before calling anything done.

## The kit (`crates/ui`) is shared

Everything is rebuilt on the kit (`docs/ui.md` › "The kit"). Several agents use it at once, so
changes to `crates/ui` are additive only: new components, icons, tokens or helpers, committed
immediately in small commits so others pick them up. Never change an existing signature or how an
existing component looks; if you think one is wrong, say so in your report instead.

## The user is at the desktop

The user works on this machine while we test. Before any synthetic click or key, check that the
target point is a mypaper window (`WindowFromPoint` → process is `mypaper-*`) and that the
foreground isn't one of their apps you'd type into; otherwise skip that step. Never minimize or
move their windows.

## Speed

The user wants this done in hours, not days. Prefer the simplest correct design, test what
matters live once, and don't bisect or build probes unless something is actually broken.
