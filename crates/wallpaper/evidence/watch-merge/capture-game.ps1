param(
    [Parameter(Mandatory=$true)][string]$Label,
    [string]$Fixture = 'C:\mypaper-targets\watch-merge\debug\examples\game_primary.exe',
    [int]$Seconds = 18
)
$ErrorActionPreference = 'Stop'
$out = Join-Path $PSScriptRoot $Label
New-Item -ItemType Directory -Force -Path $out | Out-Null
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
if (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
    throw 'Desktop is busy; retry this action after the other agent releases it.'
}
$game = $null
try {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class HoldCaptureDpi {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
    [HoldCaptureDpi]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
    [HoldCaptureDpi]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
    Add-Type -AssemblyName System.Drawing
    Add-Type -AssemblyName System.Windows.Forms
    $screen = [Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
    if (-not $screen) { throw 'This proof needs an uncovered secondary monitor.' }
    $rect = $screen.Bounds
    $rows = [Collections.Generic.List[object]]::new()
    $timer = [Diagnostics.Stopwatch]::StartNew()
    function Capture([string]$name) {
        $bitmap = [Drawing.Bitmap]::new($rect.Width, $rect.Height)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($rect.X, $rect.Y, 0, 0, $bitmap.Size)
            $bitmap.Save((Join-Path $out "$name.png"), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
        $rows.Add([pscustomobject]@{ file="$name.png"; utc=[DateTime]::UtcNow.ToString('o'); elapsed_ms=$timer.ElapsedMilliseconds })
    }
    $log = Join-Path $env:LOCALAPPDATA 'mypaper\mypaper.log'
    $offset = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    Capture 'before'
    $game = Start-Process -FilePath $Fixture -ArgumentList $Seconds -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $out 'fixture.log') -RedirectStandardError (Join-Path $out 'fixture-error.log')
    $timer.Restart()
    foreach ($point in @(@('hold-start',2000), @('before-release',9000), @('released',12000))) {
        while ($timer.ElapsedMilliseconds -lt $point[1]) { Start-Sleep -Milliseconds 50 }
        if ($game.HasExited) { throw 'The game fixture exited before the release capture.' }
        Capture $point[0]
    }
    $game.WaitForExit(30000) | Out-Null
    if (-not $game.HasExited) { throw 'The game fixture did not exit on schedule.' }
    # Several frames around resume expose black/stale frame flashes, not just a settled state.
    foreach ($i in 0..5) { Capture "resume-$i"; Start-Sleep -Milliseconds 100 }
    Start-Sleep -Seconds 1
    Capture 'resumed'
    $stream = [IO.File]::Open($log, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $stream.Position = [Math]::Min($offset, $stream.Length)
        $reader = [IO.StreamReader]::new($stream)
        try { [IO.File]::WriteAllText((Join-Path $out 'engine.log'), $reader.ReadToEnd()) } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
    [pscustomobject]@{
        monitor=$screen.DeviceName; bounds=@{ x=$rect.X; y=$rect.Y; width=$rect.Width; height=$rect.Height }
        dpi_awareness='PER_MONITOR_AWARE_V2'; fixture=$Fixture; seconds=$Seconds; fixture_exit=$game.ExitCode
        captures=$rows; processes=@(Get-Process mypaper-core,mypaper-wallpaper,mypaper-shell -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,Path)
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $out 'capture.json') -Encoding utf8
    Get-Content -LiteralPath (Join-Path $out 'fixture.log')
    Get-Content -LiteralPath (Join-Path $out 'engine.log') | Select-String 'held:|released|resumed|game hold|pausing|resuming'
    Write-Output "Captured $($screen.DeviceName) $($rect.Width)x$($rect.Height) to $out"
} finally {
    if ($game -and -not $game.HasExited) { Stop-Process -Id $game.Id -ErrorAction SilentlyContinue }
    if ([IO.Path]::GetFullPath($lock) -ne 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock') { throw 'Invalid desktop lock' }
    [IO.Directory]::Delete($lock)
}
