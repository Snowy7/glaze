# Owns only the fullscreen fixture and the shared empty lock directory.
param([ValidateSet('start', 'stop', 'lifecycle')][string]$Action)
$ErrorActionPreference = 'Stop'
$fixtureExe = 'C:\mypaper-targets\game-quiet-fixture\release\examples\game_quiet.exe'
$probeDir = 'C:\mypaper-targets\game-quiet-live'
$desktopLock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$record = Join-Path $probeDir 'fixture.json'
if ([IO.Path]::GetFullPath($desktopLock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Unexpected lock path' }
New-Item -ItemType Directory -Path $desktopLock -ErrorAction Stop | Out-Null
try {
  if ($Action -eq 'lifecycle') {
    $signalPath = Join-Path $env:LOCALAPPDATA 'mypaper/game-activity.json'
    $logPath = Join-Path $env:LOCALAPPDATA 'mypaper/mypaper.log'
    $publisher = (Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json).publisher_pid
    $gameShellProcessId = (Get-Process mypaper-shell).Id
    $gameHelperProcessId = (Get-Process mypaper-plugins).Id
    $offset = (Get-Item -LiteralPath $logPath).Length
    $output = Join-Path $PSScriptRoot 'game-lifecycle.json'
    # -Wait captures the child's exit status reliably in Windows PowerShell. Each fixture
    # phase has its own five-second bound; the whole lifecycle stays below a minute.
    $fixture = Start-Process -FilePath $fixtureExe -ArgumentList @('30', ('"' + $output + '"')) -WindowStyle Normal -Wait -PassThru -RedirectStandardOutput (Join-Path $probeDir 'lifecycle.log') -RedirectStandardError (Join-Path $probeDir 'lifecycle-error.log')
    Get-Content -LiteralPath (Join-Path $probeDir 'lifecycle.log')
    if ($fixture.ExitCode -ne 0) { Get-Content -LiteralPath (Join-Path $probeDir 'lifecycle-error.log'); throw 'Lifecycle fixture failed' }
    $signal = Get-Content -LiteralPath $signalPath -Raw | ConvertFrom-Json
    if ($signal.publisher_pid -ne $publisher -or $signal.activity.pause_background) { throw 'Publisher changed or pause did not clear' }
    if ((Get-Process mypaper-shell).Id -ne $gameShellProcessId -or (Get-Process mypaper-plugins).Id -ne $gameHelperProcessId) { throw 'Shell/helper restarted during resume' }
    $stream = [IO.FileStream]::new($logPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::UTF8)
    try { $reader.BaseStream.Seek($offset, [IO.SeekOrigin]::Begin) | Out-Null; $events = $reader.ReadToEnd() } finally { $reader.Dispose() }
    $shellResumes = [regex]::Matches($events, 'mypaper_shell\] game background pause: false').Count
    $helperResumes = [regex]::Matches($events, 'mypaper_plugins::host\] game background pause: false').Count
    if ($shellResumes -lt 3 -or $helperResumes -lt 3) { throw "Missing consumer resumes: shell=$shellResumes helper=$helperResumes" }
    @{ publisher = $publisher; shell = $gameShellProcessId; helper = $gameHelperProcessId; shell_resumes = $shellResumes; helper_resumes = $helperResumes; activity = $signal.activity } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'game-lifecycle-consumers.json') -Encoding utf8
    $events | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'game-lifecycle.log') -Encoding utf8
    powershell -NoProfile -File 'C:\Users\you\AppData\Local\Temp\mypaper-tools\shotall.ps1' -Out (Join-Path $probeDir 'restored.png')
  } elseif ($Action -eq 'start') {
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
