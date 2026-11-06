# Experimental unwinding build for transaction-panic tests, never a deployment prerequisite.
# Production ships in the normal panic=abort workspace build; core restarts a crashed helper.
param([string]$TargetDir = 'C:\mypaper-targets\index-unwind')
$ErrorActionPreference = 'Stop'
$oldTarget = $env:CARGO_TARGET_DIR
$oldPanic = $env:CARGO_PROFILE_RELEASE_PANIC
try {
  $env:CARGO_TARGET_DIR = $TargetDir
  $env:CARGO_PROFILE_RELEASE_PANIC = 'unwind'
  cargo build --release -p mypaper-index
  if ($LASTEXITCODE -ne 0) { throw 'Index release build failed' }
} finally {
  $env:CARGO_TARGET_DIR = $oldTarget
  $env:CARGO_PROFILE_RELEASE_PANIC = $oldPanic
}
