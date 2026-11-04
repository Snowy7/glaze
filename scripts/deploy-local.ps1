# Swaps a fresh release build into the repo's target\release and restarts mypaper,
# with mypaper down only for the copy (build first, elsewhere):
#   $env:CARGO_TARGET_DIR = "C:\mypaper-targets\main"; cargo build --release --workspace
#   pnpm -C widgets/shell build; pnpm -C bars/shell build
#   powershell -NoProfile -File scripts/deploy-local.ps1 [-From C:\mypaper-targets\main\release]
param([string]$From = "C:\mypaper-targets\main\release")
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$lock = "C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock"

# Serialize with anything else that restarts mypaper (stale after 5 minutes).
while (-not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue)) {
  if (((Get-Date) - (Get-Item $lock).CreationTime).TotalMinutes -gt 5) { Remove-Item $lock -Recurse -Force; continue }
  Start-Sleep 5
}
try {
  Add-Type @'
using System; using System.Runtime.InteropServices;
public static class DeployQuit { [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint t, uint m, IntPtr w, IntPtr l); }
'@
  # WM_QUIT to core: it stops its children and restores the desktop and taskbar.
  foreach ($c in Get-Process mypaper-core -ErrorAction SilentlyContinue) {
    foreach ($t in $c.Threads) { [DeployQuit]::PostThreadMessage([uint32]$t.Id, 0x12, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  }
  $names = "mypaper-core", "mypaper-wallpaper", "mypaper-widgets", "mypaper-bars"
  $deadline = (Get-Date).AddSeconds(20)
  while ((Get-Process $names -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
  if (Get-Process $names -ErrorAction SilentlyContinue) { throw "mypaper did not stop" }

  New-Item -ItemType Directory -Force "$repo\target\release" | Out-Null
  foreach ($exe in $names) { Copy-Item "$From\$exe.exe" "$repo\target\release\$exe.exe" -Force }
  foreach ($plugin in "weather", "rss") {
    if (Test-Path "$From\$plugin.exe") { Copy-Item "$From\$plugin.exe" "$repo\plugins\$plugin\$plugin.exe" -Force }
  }
  Start-Process -FilePath "$repo\target\release\mypaper-core.exe" -WorkingDirectory "$repo\target\release"
  Start-Sleep 6
  Get-Process mypaper-* | Select-Object ProcessName, Id, @{ n = 'MB'; e = { [math]::Round($_.WorkingSet64 / 1MB) } } | Format-Table -AutoSize
} finally { Remove-Item $lock -Recurse -Force -ErrorAction SilentlyContinue }
