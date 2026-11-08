# Swaps a fresh release build into the repo's target\release and restarts mypaper,
# with mypaper down only for the copy (build first, elsewhere):
#   $env:CARGO_TARGET_DIR = "C:\mypaper-targets\main"; cargo build --release --workspace
#   powershell -NoProfile -File scripts/deploy-local.ps1 [-From C:\mypaper-targets\main\release]
param([string]$From = "C:\mypaper-targets\main\release", [switch]$ValidateOnly)
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$lock = "C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock"
$destination = Join-Path $repo 'target\release'
$names = "mypaper-core", "mypaper-wallpaper", "mypaper-shell", "mypaper-studio"
function Start-DesktopCore {
  # Deploy is often run from an elevated terminal; the desktop must stay at medium integrity.
  Start-Process -FilePath 'runas.exe' -ArgumentList '/trustlevel:0x20000', "`"$destination\mypaper-core.exe`"" -WorkingDirectory $destination -WindowStyle Hidden
}
# Validate and sign before stopping a working desktop. No Rust build is started by deployment.
function Assert-FreshBinary([string]$Name) {
  $binary = Get-Item -LiteralPath "$From\$Name.exe"
  $unitName = $Name.Replace('-', '_')
  # Cargo can relink an older cached unit over release\*.exe without compiling anything.
  # Ignore examples and test harnesses: only compare the package's actual binary units.
  $units = @(Get-ChildItem -Path "$From\build\$Name\*\out\$unitName.exe" -File -ErrorAction SilentlyContinue)
  $newest = $units | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
  if ($newest -and $newest.LastWriteTimeUtc -gt $binary.LastWriteTimeUtc -and
      (Get-FileHash -LiteralPath $newest.FullName).Hash -ne (Get-FileHash -LiteralPath $binary.FullName).Hash) {
    throw "Stale build: $($binary.FullName) predates $($newest.FullName). Rebuild $Name before deploying."
  }
  $depInfo = "$From\$Name.d"
  if (-not (Test-Path -LiteralPath $depInfo)) { throw "Missing dep-info: $depInfo; rebuild $Name before deploying." }
  # Cargo's top-level dep-info includes local transitive dependencies. Also check the newest
  # unit's inputs, since the stale unit may not yet know about a newly added module.
  $depFiles = @($depInfo)
  if ($newest) { $depFiles += [IO.Path]::ChangeExtension($newest.FullName, '.d') }
  foreach ($depFile in $depFiles | Select-Object -Unique) {
    if (-not (Test-Path -LiteralPath $depFile)) { throw "Missing dep-info: $depFile" }
    $inputs = (Get-Content -LiteralPath $depFile -TotalCount 1) -replace '^.*?:\s+', ''
    foreach ($inputPath in [regex]::Split($inputs, '(?<!\\)\s+')) {
      if (-not $inputPath) { continue }
      $source = $inputPath.Replace('\ ', ' ')
      if (-not [IO.Path]::IsPathRooted($source)) { $source = Join-Path $repo $source }
      if (-not (Test-Path -LiteralPath $source) -or (Get-Item -LiteralPath $source).LastWriteTimeUtc -gt $binary.LastWriteTimeUtc) {
        throw "Stale build: $($binary.FullName) is older than, or missing, input $source. Rebuild $Name before deploying."
      }
    }
  }
  Write-Host "Fresh build: $Name ($($binary.LastWriteTimeUtc.ToString('o')))"
}
foreach ($exe in $names) {
  if (-not (Test-Path -LiteralPath "$From\$exe.exe")) { throw "Missing build: $From\$exe.exe" }
}
foreach ($exe in $names + @('mypaper-files', 'mypaper-index', 'mypaper-plugins', 'mypaper-native-host', 'mypaper-styler', 'weather', 'rss')) {
  if (Test-Path -LiteralPath "$From\$exe.exe") { Assert-FreshBinary $exe }
}
if ($ValidateOnly) { return }
$identity = & "$PSScriptRoot\package\build.ps1" -ExternalLocation $destination

# A long-running owner may still be live: never steal its directory based on age.
$lockDeadline = (Get-Date).AddMinutes(2)
while (-not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue)) {
  if ((Get-Date) -ge $lockDeadline) { throw "Desktop lock is busy: $lock" }
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
    Write-Warning "Studio is open; leaving it running (its exe is swapped below)"
    $copy = $names | Where-Object { $_ -ne "mypaper-studio" }
  }
  $deadline = (Get-Date).AddSeconds(20)
  while ((Get-Process $copy -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
  if (Get-Process $copy -ErrorAction SilentlyContinue) { throw "mypaper did not stop" }

  New-Item -ItemType Directory -Force $destination | Out-Null
  # An open Studio keeps running, but its exe is still replaced: a running exe can be renamed,
  # so the old one moves aside and its "Restart Studio" (or the next launch) gets this build.
  # Without this an old Studio lingered and misread newer files (2026-11-05: version 2 layouts).
  if ($copy -notcontains "mypaper-studio" -and (Test-Path -LiteralPath "$From\mypaper-studio.exe") -and
      [IO.Path]::GetFullPath("$From\mypaper-studio.exe") -ne [IO.Path]::GetFullPath("$destination\mypaper-studio.exe")) {
    Get-ChildItem -LiteralPath $destination -Filter "mypaper-studio.exe.old-*" -ErrorAction SilentlyContinue |
      ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }
    $aside = "$destination\mypaper-studio.exe.old-$(Get-Date -Format yyyyMMddHHmmss)"
    try {
      Move-Item -LiteralPath "$destination\mypaper-studio.exe" -Destination $aside -Force
      Copy-Item -LiteralPath "$From\mypaper-studio.exe" -Destination "$destination\mypaper-studio.exe" -Force
      Write-Warning "Studio is open on the old build; restart it to update (the new exe is in place)"
    } catch {
      if (-not (Test-Path -LiteralPath "$destination\mypaper-studio.exe") -and (Test-Path -LiteralPath $aside)) {
        Move-Item -LiteralPath $aside -Destination "$destination\mypaper-studio.exe" -Force
      }
      Write-Warning "Could not swap the open Studio's exe ($_); it stays on the old build"
    }
  }
  foreach ($exe in $copy) {
    if ([IO.Path]::GetFullPath("$From\$exe.exe") -ne [IO.Path]::GetFullPath("$destination\$exe.exe")) {
      Copy-Item -LiteralPath "$From\$exe.exe" -Destination "$destination\$exe.exe" -Force
    }
  }
  # Optional native services and on-demand apps ship beside core; none starts at deployment.
  foreach ($exe in "mypaper-files", "mypaper-index") {
    if ((Test-Path -LiteralPath "$From\$exe.exe") -and
        [IO.Path]::GetFullPath("$From\$exe.exe") -ne [IO.Path]::GetFullPath("$destination\$exe.exe")) {
      if (Get-Process $exe -ErrorAction SilentlyContinue) {
        # A running optional app can finish on its old image; the next launch gets this build.
        $aside = "$destination\$exe.exe.old-$(Get-Date -Format yyyyMMddHHmmss)"
        if (Test-Path -LiteralPath "$destination\$exe.exe") { Move-Item -LiteralPath "$destination\$exe.exe" -Destination $aside -Force }
      }
      Copy-Item -LiteralPath "$From\$exe.exe" -Destination "$destination\$exe.exe" -Force
    }
  }
  foreach ($plugin in "weather", "rss") {
    if (Test-Path "$From\$plugin.exe") { Copy-Item "$From\$plugin.exe" "$repo\plugins\$plugin\$plugin.exe" -Force }
  }
  # The plugins this build ships: target\release\plugins, found before the repo's plugins\
  # (files::bundled_plugins), with each plugin's manifest, component and command only. The SDK
  # examples stay in the repo and are never installed for the user (hello registers a global
  # shortcut and a desktop-menu item). Copies, not junctions: cleaning target\ must never reach
  # into the repo. A rebuilt .wasm goes live with the next deploy.
  $examples = "hello", "hello-ts"
  $bundle = Join-Path $destination 'plugins'
  if (Test-Path -LiteralPath $bundle) { Remove-Item -LiteralPath $bundle -Recurse -Force }
  foreach ($dir in Get-ChildItem -LiteralPath "$repo\plugins" -Directory) {
    if ($examples -contains $dir.Name -or -not (Test-Path -LiteralPath "$($dir.FullName)\plugin.json")) { continue }
    $to = New-Item -ItemType Directory -Force (Join-Path $bundle $dir.Name)
    Get-ChildItem -LiteralPath $dir.FullName -File |
      Where-Object { $_.Name -eq 'plugin.json' -or $_.Extension -in '.wasm', '.exe' } |
      Copy-Item -Destination $to.FullName -Force
  }
  # The WebAssembly plugin runtime (docs/plugins.md), and the bundled components' AOT cache
  # (scripts/build-plugins.ps1 builds the .wasm files into their plugin folders).
  # mypaper-native-host runs plugins' own native modules, only while such a plugin is active.
  if (Test-Path -LiteralPath "$From\mypaper-plugins.exe") {
    foreach ($exe in "mypaper-plugins", "mypaper-native-host") {
      if ((Test-Path -LiteralPath "$From\$exe.exe") -and
          [IO.Path]::GetFullPath("$From\$exe.exe") -ne [IO.Path]::GetFullPath("$destination\$exe.exe")) {
        Copy-Item -LiteralPath "$From\$exe.exe" -Destination "$destination\$exe.exe" -Force
      }
    }
    Start-Process -FilePath "$destination\mypaper-plugins.exe" -ArgumentList "--compile", "`"$bundle`"" -Wait -WindowStyle Hidden
  }
  # The experimental app styler (docs/windows-styling.md, "App styler"): the helper core runs only
  # while experimental.app_styler is on, and the TAP DLL it loads into Explorer and Settings. Apps
  # load a hash-named copy of the DLL, so replacing this one is safe while they hold an old build.
  foreach ($file in "mypaper-styler.exe", "mypaper_app_styler.dll") {
    if ((Test-Path -LiteralPath "$From\$file") -and
        [IO.Path]::GetFullPath("$From\$file") -ne [IO.Path]::GetFullPath("$destination\$file")) {
      Copy-Item -LiteralPath "$From\$file" -Destination "$destination\$file" -Force
    }
  }
  & "$PSScriptRoot\package\register.ps1" -PackagePath $identity -ExternalLocation $destination -Development
  Start-DesktopCore
  Start-Sleep 6
  Get-Process mypaper-* | Select-Object ProcessName, Id, @{ n = 'MB'; e = { [math]::Round($_.WorkingSet64 / 1MB) } } | Format-Table -AutoSize
} finally {
  # A failed registration/copy must still bring back core (unpackaged snapshots remain supported).
  if (-not (Get-Process mypaper-core -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath "$destination\mypaper-core.exe")) {
    Start-DesktopCore
  }
  if ([IO.Path]::GetFullPath($lock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Invalid desktop lock path' }
  [IO.Directory]::Delete($lock)
}
