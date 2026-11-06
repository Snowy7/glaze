# Build into the shared deployment target after the normal workspace release build.
# The workspace's panic=abort bypasses catch_unwind; this service needs transaction isolation.
param([string]$TargetDir = 'C:\mypaper-targets\main')
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
