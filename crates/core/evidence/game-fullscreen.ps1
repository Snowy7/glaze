# Owns only the fullscreen fixture and the shared empty lock directory.
param([ValidateSet('start', 'stop')][string]$Action)
$ErrorActionPreference = 'Stop'
$fixtureExe = 'C:\mypaper-targets\game-quiet-fixture\release\examples\game_quiet.exe'
$probeDir = 'C:\mypaper-targets\game-quiet-live'
$desktopLock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$record = Join-Path $probeDir 'fixture.json'
if ([IO.Path]::GetFullPath($desktopLock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Unexpected lock path' }
New-Item -ItemType Directory -Path $desktopLock -ErrorAction Stop | Out-Null
try {
  if ($Action -eq 'start') {
    $fixture = Start-Process -FilePath $fixtureExe -ArgumentList '180' -WindowStyle Normal -PassThru -RedirectStandardOutput (Join-Path $probeDir 'fullscreen.log') -RedirectStandardError (Join-Path $probeDir 'fullscreen-error.log')
    @{ id = $fixture.Id; started = $fixture.StartTime.ToUniversalTime().Ticks } | ConvertTo-Json | Set-Content -LiteralPath $record
    Start-Sleep -Seconds 12
    Get-Content -LiteralPath (Join-Path $probeDir 'fullscreen.log')
    $signal = Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'mypaper/game-activity.json') -Raw | ConvertFrom-Json
    if (-not $signal.activity.pause_background -or $signal.activity.foreground_pid -ne $fixture.Id) {
      Stop-Process -InputObject $fixture
      throw 'Owned fixture did not become the active fullscreen signal'
    }
    $signal | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'game-fullscreen-signal.json') -Encoding utf8
    powershell -NoProfile -File 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $probeDir 'fullscreen.png')
  } else {
    $owner = Get-Content -LiteralPath $record -Raw | ConvertFrom-Json
    $fixture = Get-Process -Id $owner.id -ErrorAction SilentlyContinue
    if ($fixture) {
      if ($fixture.Path -ne $fixtureExe -or $fixture.StartTime.ToUniversalTime().Ticks -ne $owner.started) { throw 'Fixture identity changed; refusing to stop it' }
      Stop-Process -InputObject $fixture
    }
    Start-Sleep -Seconds 2
    Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'mypaper/game-activity.json') -Raw
    powershell -NoProfile -File 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $probeDir 'restored.png')
  }
} finally {
  # Delete only our empty lock directory; never recursively remove desktop files.
  [IO.Directory]::Delete($desktopLock)
}
Get-Date -Format o
