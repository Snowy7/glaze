# Finding 7: trip the crash guard for Explorer with two separate folder-window processes
# (explorer.exe /separate) ended within 60 s of being attached, more than 5 s apart (two
# incidents). The guard must stop styling Explorer and the *surviving* Explorer process (the
# shell, 76024) must put its look back and detach. Only the separate processes this script starts
# are ended. Holds the desktop lock per step.
param([string]$Out = "C:\mypaper-targets\app-styler\round3\crash-guard.txt")
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'mypaper\app-styler'
$tapLog = Join-Path $dir 'styler.log'
$appLog = Join-Path $env:LOCALAPPDATA 'mypaper\mypaper.log'
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
function Note([string]$m) { $l = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss.fff'), $m; Add-Content -LiteralPath $Out -Value $l; Write-Host $l }
function Take { for ($i = 0; -not (New-Item -ItemType Directory $lock -ErrorAction SilentlyContinue); $i++) { if ($i -gt 240) { throw 'desktop lock busy' }; Start-Sleep -Milliseconds 500 } }
function Drop { if (Test-Path -LiteralPath $lock) { Remove-Item -LiteralPath $lock -Recurse -Force } }

$survivors = @(Get-Process explorer | ForEach-Object Id)
Note "explorer processes before: $($survivors -join ', '); marker exists: $(Test-Path (Join-Path $dir 'explorer.off'))"
$tapMark = (Get-Content -LiteralPath $tapLog).Count
$appMark = (Get-Content -LiteralPath $appLog).Count

foreach ($round in 1, 2) {
  Take
  try {
    $known = @(Get-Process explorer | ForEach-Object Id)
    Start-Process explorer.exe -ArgumentList '/separate,C:\Windows'
    $new = $null
    for ($i = 0; $i -lt 60 -and -not $new; $i++) {
      Start-Sleep -Milliseconds 250
      $new = Get-Process explorer | Where-Object { $known -notcontains $_.Id } | Select-Object -First 1
    }
    if (-not $new) { Note "round ${round}: no separate explorer process appeared"; break }
    # Wait for the TAP in it to report.
    $attached = $false
    for ($i = 0; $i -lt 60 -and -not $attached; $i++) {
      Start-Sleep -Milliseconds 250
      $attached = [bool](Select-String -LiteralPath $tapLog -Pattern "\[tap $($new.Id)\] watching the visual tree" -SimpleMatch:$false -Quiet)
    }
    Note "round ${round}: separate explorer $($new.Id), TAP attached: $attached"
    Start-Sleep -Seconds 2
    Stop-Process -Id $new.Id -Force
    Note "round ${round}: ended $($new.Id)"
  } finally { Drop }
  Start-Sleep -Seconds 7
}
Start-Sleep -Seconds 3
Note "marker exists: $(Test-Path (Join-Path $dir 'explorer.off')); explorer processes now: $((Get-Process explorer | ForEach-Object Id) -join ', ')"
Note '--- helper log'
Get-Content -LiteralPath $appLog | Select-Object -Skip $appMark | Where-Object { $_ -match 'mypaper_styler' } | ForEach-Object { Note "  $_" }
Note '--- TAP log (survivors)'
Get-Content -LiteralPath $tapLog | Select-Object -Skip $tapMark | Where-Object { $_ -match ($survivors -join '|') -and $_ -match 'stopped|restoring|put back|detached|forgot|leaving' } | ForEach-Object { Note "  $_" }
