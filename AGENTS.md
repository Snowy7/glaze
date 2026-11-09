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

When agents share a checkout, run the script in a `git archive HEAD` snapshot of Glaze beside
an archived stillkit checkout, with only your changes applied. This excludes other agents'
uncommitted edits. Junction the live themes directory only if archived media is unavailable.
Use a fresh snapshot directory or refresh source timestamps when replacing a snapshot;
restoring older timestamps can make Cargo reuse a stale test binary.
