# Implementing the native plan: rules for every agent

The plan is `plans/council/native-plan.md`, approved by a three-model council. **Read it in full
first.** Treat its "User decisions" as fixed. Glass research is in `plans/council/liquid-glass-research.md`.

## Who you are working for
Alex: keep things simple (YAGNI), use type safety, add short comments on functions and modules, keep
docs in sync, and write focused tests (no smoke tests). The rebuild is **destructive**: delete what
you replace, and keep no fallbacks or dual paths. Performance is the product: every rule and budget in
the plan is a merge gate, so measure it and do not estimate it.

## Workspace
- Windows, Rust nightly with Cranelift (`rust-toolchain.toml`, `.cargo/config.toml`). A failing test
  aborts the process, so run tests per crate.
- Check only what you changed: `cargo check -p <crate>`, `cargo clippy -p <crate>`, `cargo test -p <crate>`.
  Avoid full-workspace builds unless your slice needs one.
- Commit to **your own branch** in your worktree, in small, coherent commits. **Do not push, and do not
  merge.** The orchestrator merges.
- Use pnpm only where the TS shells still exist; never npm or yarn.

## Ownership (avoid collisions)
Each task lists the files it owns. Do not edit anything outside that list. If you need an outside
change, make the smallest possible edit and call it out in your report, with its path and reason, so
the merge can handle it.

## Live testing on the real desktop
- Only one agent may run Glaze on the user's desktop at a time. Take the lock before you start
  anything that runs Glaze or touches the shell or desktop:
  `%LOCALAPPDATA%\Temp\glaze-tools\desktop.lock`. Create it as a directory; if it
  already exists, wait. It is considered stale after 5 minutes. Release it in a `finally` block.
  `measure.ps1` in that folder takes the lock itself, so don't hold the lock while calling it.
- Measure with `%LOCALAPPDATA%\Temp\glaze-tools\measure.ps1 -Label <name> -Seconds 60`
  (CPU, GPU and RAM per process). For harness numbers, use `tools/perf`.
- Whatever you start, you stop. Always leave the desktop as you found it: the stock taskbar shows and
  the user's Glaze is running. The panic hotkey is Ctrl+Alt+Shift+Backspace.
- Any headless browser or helper process you start must be killed, with its profile deleted, in a
  `finally` block.

## Reporting
End with a short report covering:
- what landed, as commits;
- **the measured numbers against the plan's gates**, as a table;
- what failed or was skipped, and why;
- any edits outside your ownership list;
- the API you exposed for the next slices: the exact types and messages.
