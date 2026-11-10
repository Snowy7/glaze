# Glaze

## Commands

Run the complete v0.1 gate from the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1 -Target C:\glaze-targets\tests
```

The gate installs locked pnpm dependencies, typechecks/tests the TypeScript SDK, builds all nine
WASM plugins against canonical host WIT, and runs debug workspace tests and the six component
crates' native tests. It checks plugin host linker compatibility and Hello's lifecycle in an
isolated profile, then runs stillkit's all-target check and golden/serde workspace tests, the CPU
executable's DLL import check, and workspace Clippy (including stillkit GPU code). Clippy warnings
remain warnings. LLVM is used for deterministic Windows assertion failures. LLVM's `llvm-readobj.exe`
must be on PATH or installed in `C:\Program Files\LLVM\bin`.

`-StillkitPath` selects the sibling stillkit checkout and overrides Glaze's local Cargo patch
for this run. `-Target` defaults to `CARGO_TARGET_DIR`, then `C:\glaze-targets\check`.
Each suite gets a separate directory beneath it. Existing ignored live-desktop fixtures remain
opt-in; the gate does not run them or bless golden images.
APPDATA and LOCALAPPDATA use a fresh profile beneath the target directory for Rust tests and host checks,
so test drafts and caches do not enter the running desktop's configuration directories.

When agents share a checkout, run the script in a `git archive HEAD` snapshot of Glaze beside
an archived stillkit checkout, with only your changes applied. This excludes other agents'
uncommitted edits. Junction the live themes directory only if archived media is unavailable.
Use a fresh snapshot directory or refresh source timestamps when replacing a snapshot;
restoring older timestamps can make Cargo reuse a stale test binary.

## Desktop lock

Tools that change or restart the desktop share `%LOCALAPPDATA%\Temp\glaze-tools\desktop.lock`.
Create the directory atomically, wait while another tool owns it, and delete only your own lock
after cleanup. PowerShell resolves it with `Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock'`;
Rust resolves `LOCALAPPDATA` at runtime.

## Golden images

Set `GLAZE_BLESS=1` for fixture updates. Since the UI extraction, image comparisons delegated
to stillkit also require `STILLKIT_BLESS=1`. Run only the affected golden tests in an isolated
APPDATA/LOCALAPPDATA profile, inspect every changed PNG, and unset both flags before the full gate.
