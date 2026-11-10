# Finding 2: end glaze-core abruptly (TerminateProcess) and check that the app styler exits with
# it and Explorer/Settings get their look back; then start Glaze again the normal way.
# Only glaze-core is ended. Holds the desktop lock for the test.
param([string]$Out = "C:\glaze-targets\app-styler\round3\core-kill.txt")
$ErrorActionPreference = 'Stop'
$repo = 'D:\Coding\projects\desktop\wallpaper'
$release = Join-Path $repo 'target\release'
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$tapLog = Join-Path $env:LOCALAPPDATA 'glaze\app-styler\styler.log'
function Note([string]$m) { $l = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss.fff'), $m; Add-Content -LiteralPath $Out -Value $l; Write-Host $l }
function Procs { (Get-Process glaze-* -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) $($_.Id)" }) -join ', ' }

for ($i = 0; -not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue); $i++) {
  if ($i -gt 240) { throw 'desktop lock busy' }; Start-Sleep -Milliseconds 500
}
try {
  $core = Get-Process glaze-core
  $styler = Get-Process glaze-styler
  $line = (Get-CimInstance Win32_Process -Filter "ProcessId=$($styler.Id)").CommandLine
  Note "core $($core.Id), styler $($styler.Id) ($line)"
  Note "running: $(Procs)"
  $mark = (Get-Content -LiteralPath $tapLog).Count
  Stop-Process -Id $core.Id -Force
  $t0 = Get-Date
  while ((Get-Process -Id $styler.Id -ErrorAction SilentlyContinue) -and ((Get-Date) - $t0).TotalSeconds -lt 20) { Start-Sleep -Milliseconds 50 }
  if (Get-Process -Id $styler.Id -ErrorAction SilentlyContinue) { Note 'FAIL: the styler is still running 20 s after core was ended' }
  else { Note ("styler exited {0:N0} ms after core was ended" -f ((Get-Date) - $t0).TotalMilliseconds) }
  Start-Sleep -Seconds 2
  Note "after: $(Procs)"
  Note '--- TAP log since'
  Get-Content -LiteralPath $tapLog | Select-Object -Skip $mark | ForEach-Object { Note "  $_" }
  Note '--- starting Glaze again (as deploy-local does)'
  Start-Process -FilePath (Join-Path $release 'glaze-core.exe') -WorkingDirectory $release -WindowStyle Hidden
  $t1 = Get-Date
  while (-not (Get-Process glaze-styler -ErrorAction SilentlyContinue) -and ((Get-Date) - $t1).TotalSeconds -lt 20) { Start-Sleep -Milliseconds 200 }
  Start-Sleep -Seconds 5
  $new = Get-Process glaze-styler -ErrorAction SilentlyContinue
  if ($new) { Note "new styler $($new.Id) ($((Get-CimInstance Win32_Process -Filter "ProcessId=$($new.Id)").CommandLine))" } else { Note 'FAIL: no new styler' }
  Note "running: $(Procs)"
  $mark2 = $mark
  Get-Content -LiteralPath $tapLog | Select-Object -Skip $mark2 | Select-Object -Last 8 | ForEach-Object { Note "  $_" }
} finally {
  if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force }
}
