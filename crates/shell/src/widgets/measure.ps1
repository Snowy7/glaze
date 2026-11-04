# Run after the release mypaper-widget-fixture example has been built. No user process is stopped.
param([switch]$Interactions)
$ErrorActionPreference = 'Stop'
$fixtureWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$fixtureLock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$fixtureRoot = Join-Path $fixtureWorkspace 'artifacts\n4\live'
$fixtureExe = Join-Path $fixtureWorkspace 'target\release\examples\mypaper-widget-fixture.exe'
$fixtureOldAppData = $env:APPDATA
$fixtureOldLocal = $env:LOCALAPPDATA
$fixtureProcess = $null
$fixtureLockOwned = $false
$fixtureForcedStop = $false
function Enter-FixtureLock {
    while (-not (New-Item -ItemType Directory -Path $fixtureLock -ErrorAction SilentlyContinue)) {
        $existing = Get-Item -LiteralPath $fixtureLock -ErrorAction SilentlyContinue
        if ($existing -and ((Get-Date) - $existing.CreationTime).TotalMinutes -gt 5) {
            # Fixed, named lock directory only; never recursive and never a computed data path.
            Remove-Item -LiteralPath $fixtureLock -ErrorAction SilentlyContinue
        }
        Start-Sleep -Seconds 3
    }
}
try {
    Write-Output 'Waiting for the shared desktop lock.'
    Enter-FixtureLock
    $fixtureLockOwned = $true
    New-Item -ItemType Directory -Path "$fixtureRoot\appdata", "$fixtureRoot\local" -Force | Out-Null
    $env:APPDATA = "$fixtureRoot\appdata"
    $env:LOCALAPPDATA = "$fixtureRoot\local"
    $env:MYPAPER_N4_FIXTURE = '1'
    $env:MYPAPER_N4_DESKTOP_VISIBLE='1'
    if ($Interactions) { $env:MYPAPER_N4_INTERACTIONS='1' }
    $env:MYPAPER_WIDGETS_IGNORE_COVERED = '1'
    $fixtureProcess = Start-Process -FilePath $fixtureExe -ArgumentList '300' -WindowStyle Hidden -PassThru -RedirectStandardOutput "$fixtureRoot\stdout.log" -RedirectStandardError "$fixtureRoot\stderr.log"
    $env:APPDATA = $fixtureOldAppData
    $env:LOCALAPPDATA = $fixtureOldLocal
    Remove-Item Env:MYPAPER_N4_FIXTURE, Env:MYPAPER_WIDGETS_IGNORE_COVERED, Env:MYPAPER_N4_INTERACTIONS, Env:MYPAPER_N4_DESKTOP_VISIBLE -ErrorAction SilentlyContinue
    $readyDeadline=(Get-Date).AddSeconds(45)
    while (-not $fixtureProcess.HasExited -and (Get-Date) -lt $readyDeadline) {
        if ((Get-Content "$fixtureRoot\stdout.log" -ErrorAction SilentlyContinue) -match 'N4_READY') {break}
        Start-Sleep -Milliseconds 200
    }
    Start-Sleep -Seconds 4
    if ($fixtureProcess.HasExited) { Get-Content "$fixtureRoot\stderr.log"; throw 'Fixture exited before measurement' }
    Remove-Item -LiteralPath $fixtureLock
    $fixtureLockOwned = $false
    # measure.ps1 takes the lock itself. The separate process keeps its isolated environment.
    foreach ($run in 1..3) {
        & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\measure.ps1' -Label "n4-widgets-$run" -Seconds 60
        Copy-Item -LiteralPath "C:\Users\you\AppData\Local\Temp\mypaper-tools\measure-n4-widgets-$run.json" -Destination $fixtureRoot
    }
    $stopEvent = [System.Threading.EventWaitHandle]::OpenExisting("Local\mypaper-n4-fixture-stop-$($fixtureProcess.Id)")
    try { $stopEvent.Set() | Out-Null } finally { $stopEvent.Dispose() }
    $fixtureProcess.WaitForExit(30000) | Out-Null
    Get-Content "$fixtureRoot\stdout.log"
    Get-Content "$fixtureRoot\stderr.log" -Tail 30
} finally {
    $env:APPDATA = $fixtureOldAppData
    $env:LOCALAPPDATA = $fixtureOldLocal
    Remove-Item Env:MYPAPER_N4_FIXTURE, Env:MYPAPER_WIDGETS_IGNORE_COVERED, Env:MYPAPER_N4_INTERACTIONS, Env:MYPAPER_N4_DESKTOP_VISIBLE -ErrorAction SilentlyContinue
    if ($fixtureProcess -and -not $fixtureProcess.HasExited) {
        if (-not $fixtureLockOwned) { Enter-FixtureLock; $fixtureLockOwned = $true }
        Stop-Process -Id $fixtureProcess.Id -ErrorAction SilentlyContinue
        $fixtureForcedStop = $true
    }
    if ($fixtureForcedStop) {
        # A forced stop bypasses Rust's guards. Restore only the desktop state this fixture changed.
        $fixtureShell = New-Object -ComObject Shell.Application
        try { $fixtureShell.UndoMinimizeALL() } finally { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($fixtureShell) }
        if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
            Start-Process -FilePath (Join-Path $env:WINDIR 'explorer.exe') -WindowStyle Hidden
        }
    }
    if ($fixtureLockOwned) { Remove-Item -LiteralPath $fixtureLock -ErrorAction SilentlyContinue }
}
