# Swaps a fresh release build into the repo's target\release and restarts mypaper,
# with mypaper down only for the copy (build first, elsewhere):
#   $env:CARGO_TARGET_DIR = "C:\mypaper-targets\main"; cargo build --release --workspace
#   powershell -NoProfile -File scripts/deploy-local.ps1 [-From C:\mypaper-targets\main\release]
param([string]$From = "C:\mypaper-targets\main\release")
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$lock = "C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock"
$destination = Join-Path $repo 'target\release'
$names = "mypaper-core", "mypaper-wallpaper", "mypaper-shell", "mypaper-studio"
# Validate and sign before stopping a working desktop. No Rust build is started by deployment.
foreach ($exe in $names) {
  if (-not (Test-Path -LiteralPath "$From\$exe.exe")) { throw "Missing build: $From\$exe.exe" }
}
$identity = & "$PSScriptRoot\package\build.ps1" -ExternalLocation $destination

# Serialize with anything else that restarts mypaper (stale after 5 minutes).
while (-not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue)) {
  if (((Get-Date) - (Get-Item $lock).CreationTime).TotalMinutes -gt 5) { [IO.Directory]::Delete($lock); continue }
  Start-Sleep 5
}
try {
  # Inspect origins before every restart; another worktree may own the current desktop.
  Get-Process mypaper-* -ErrorAction SilentlyContinue | Select-Object ProcessName, Id, Path | Format-Table -AutoSize
  Add-Type @'
using System; using System.Runtime.InteropServices;
public static class DeployQuit { [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint t, uint m, IntPtr w, IntPtr l); }
'@
  # WM_QUIT to core: it stops its children and restores the desktop and taskbar.
  foreach ($c in Get-Process mypaper-core -ErrorAction SilentlyContinue) {
    foreach ($t in $c.Threads) { [DeployQuit]::PostThreadMessage([uint32]$t.Id, 0x12, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  }
  # An open Studio window belongs to the user: leave it running and skip its exe (locked) this time.
  $copy = $names
  if (Get-Process mypaper-studio -ErrorAction SilentlyContinue) {
    Write-Warning "Studio is open; leaving it running and not updating mypaper-studio.exe"
    $copy = $names | Where-Object { $_ -ne "mypaper-studio" }
  }
  $deadline = (Get-Date).AddSeconds(20)
  while ((Get-Process $copy -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
  if (Get-Process $copy -ErrorAction SilentlyContinue) { throw "mypaper did not stop" }

  New-Item -ItemType Directory -Force $destination | Out-Null
  foreach ($exe in $copy) {
    if ([IO.Path]::GetFullPath("$From\$exe.exe") -ne [IO.Path]::GetFullPath("$destination\$exe.exe")) {
      Copy-Item -LiteralPath "$From\$exe.exe" -Destination "$destination\$exe.exe" -Force
    }
  }
  foreach ($plugin in "weather", "rss") {
    if (Test-Path "$From\$plugin.exe") { Copy-Item "$From\$plugin.exe" "$repo\plugins\$plugin\$plugin.exe" -Force }
  }
  & "$PSScriptRoot\package\register.ps1" -PackagePath $identity -ExternalLocation $destination -Development
  Start-Process -FilePath "$destination\mypaper-core.exe" -WorkingDirectory $destination -WindowStyle Hidden
  Start-Sleep 6
  Get-Process mypaper-* | Select-Object ProcessName, Id, @{ n = 'MB'; e = { [math]::Round($_.WorkingSet64 / 1MB) } } | Format-Table -AutoSize
} finally {
  # A failed registration/copy must still bring back core (unpackaged snapshots remain supported).
  if (-not (Get-Process mypaper-core -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath "$destination\mypaper-core.exe")) {
    Start-Process -FilePath "$destination\mypaper-core.exe" -WorkingDirectory $destination -WindowStyle Hidden
  }
  if ([IO.Path]::GetFullPath($lock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Invalid desktop lock path' }
  [IO.Directory]::Delete($lock)
}
