param(
    [ValidateSet(4,10,30)][int]$Count = 4,
    [switch]$Idle,
    [switch]$Off,
    [switch]$Crash,
    [switch]$Cases,
    [switch]$Benchmark,
    [string]$Label = 'tiling'
)
$ErrorActionPreference = 'Stop'
if (!('GlazeTilingForeground' -as [type])) {
    Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class GlazeTilingForeground{[DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();}'
}
$taskTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'C:\glaze-targets\tiling' }
$fixture = Join-Path $taskTarget 'release\examples\glaze-tiling-fixture.exe'
$outputDir = Join-Path (Join-Path $taskTarget 'evidence') ($Label + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
# Shared with every desktop-changing tool; see AGENTS.md.
$desktopLock = Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock'
if (!(Test-Path -LiteralPath $fixture)) { throw 'Build the release tiling_fixture example in the task target directory first.' }
New-Item -ItemType Directory -Path $outputDir | Out-Null
$foreground = [GlazeTilingForeground]::GetForegroundWindow()
New-Item -ItemType Directory -Path $desktopLock -ErrorAction Stop | Out-Null
$launcher = $null
try {
    $mode = if ($Crash) { '--crash' } elseif ($Cases) { '--cases' } elseif ($Benchmark) { '--bench' } else { '--run' }
    $fixtureArgs = $mode + ' "' + $outputDir + '" ' + $Count
    if ($Idle) { $fixtureArgs += ' idle' } else { $fixtureArgs += ' exercise' }
    if ($Off) { $fixtureArgs += ' off' }
    $restrictedCommand = '"' + $fixture + '" ' + $fixtureArgs
    $launcher = Start-Process -FilePath "$env:SystemRoot\System32\runas.exe" -ArgumentList ('/trustlevel:0x20000 "' + $restrictedCommand + '"') -WindowStyle Hidden -PassThru
    $deadline = (Get-Date).AddSeconds(95)
    $recovering = $false
    while (!(Test-Path -LiteralPath (Join-Path $outputDir 'done.txt'))) {
        if ($Crash -and !$recovering -and (Test-Path -LiteralPath (Join-Path $outputDir 'crash-ready.txt'))) {
            Start-Sleep -Milliseconds 200
            $recoverCommand = '"' + $fixture + '" --recover "' + $outputDir + '"'
            $launcher = Start-Process -FilePath "$env:SystemRoot\System32\runas.exe" -ArgumentList ('/trustlevel:0x20000 "' + $recoverCommand + '"') -WindowStyle Hidden -PassThru
            $recovering = $true
        }
        if (Test-Path -LiteralPath (Join-Path $outputDir 'error.txt')) { throw (Get-Content -Raw -LiteralPath (Join-Path $outputDir 'error.txt')) }
        if ((Get-Date) -gt $deadline) { throw 'Fixture exceeded the desktop-lock deadline.' }
        Start-Sleep -Milliseconds 500
    }
    Get-Content -Raw -LiteralPath (Join-Path $outputDir 'report.json')
    Get-Content -LiteralPath (Join-Path $outputDir 'fixture.log') | Select-Object -Last 8
    Write-Output ('Evidence: ' + $outputDir)
}
finally {
    try {
        # Only this run's fixture and child: never terminate another fixture or the live shell.
        Get-CimInstance Win32_Process -Filter "Name = 'glaze-tiling-fixture.exe'" | Where-Object {
            $_.ExecutablePath -eq $fixture -and $_.CommandLine -and $_.CommandLine.Contains($outputDir)
        } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        if ($launcher -and !$launcher.HasExited) { Stop-Process -Id $launcher.Id -Force -ErrorAction SilentlyContinue }
    }
    finally {
        $after = [GlazeTilingForeground]::GetForegroundWindow()
        [ordered]@{ before = $foreground.ToInt64(); after = $after.ToInt64(); unchanged = ($foreground -eq $after) } |
            ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputDir 'foreground.json')
        if ((Resolve-Path -LiteralPath $desktopLock).Path -ne $desktopLock) { throw 'Unexpected desktop lock path.' }
        Remove-Item -LiteralPath $desktopLock -Force
        if ($after -ne $foreground) { throw 'Foreground changed; this live run is rejected.' }
    }
}
