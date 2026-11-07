# One brief desktop action: atomic config replacement and a change in a theme subdirectory.
# Restores the exact config bytes if nothing else wrote them; otherwise restores only our FPS.
$ErrorActionPreference = 'Stop'
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
if (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) { throw 'Desktop is busy' }
$path = Join-Path $env:APPDATA 'mypaper\config.json'
$probe = $null
$temporary = "$path.watch-merge.tmp"
$written = $null
$created = $false
$out = 'C:\mypaper-targets\watch-merge\evidence\reload'
function Write-Config([string]$text) {
    [IO.File]::WriteAllText($temporary, $text, [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporary -Destination $path -Force
}
try {
    $theme = Join-Path (Resolve-Path "$PSScriptRoot\..\..\..\..") 'themes\rain-city'
    $probe = Join-Path $theme 'media\watch-merge-proof.txt'
    $original = [IO.File]::ReadAllText($path)
    $originalBytes = [IO.File]::ReadAllBytes($path)
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    if (Test-Path -LiteralPath $probe) { throw 'Theme proof path already exists' }
    $before = Get-Process mypaper-wallpaper
    $log = Join-Path $env:LOCALAPPDATA 'mypaper\mypaper.log'
    $offset = (Get-Item -LiteralPath $log).Length
    $config = $original | ConvertFrom-Json
    if ($config.theme -ne 'rain-city') { throw 'Reload proof expects the currently selected rain-city theme' }
    $fps = $config.engine.fps
    $config.engine.fps = 24
    $written = $config | ConvertTo-Json -Depth 100
    Write-Config $written
    Start-Sleep -Seconds 1
    [IO.File]::WriteAllText($probe, 'Temporary recursive watcher proof. Removed in finally.')
    $created = $true
    Start-Sleep -Seconds 2
    $after = Get-Process mypaper-wallpaper
    if ($before.Id -ne $after.Id) { throw 'Engine restarted during reload proof' }
    $stream = [IO.File]::Open($log, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $stream.Position = [Math]::Min($offset, $stream.Length)
        $reader = [IO.StreamReader]::new($stream)
        try { $changes = $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
    [IO.File]::WriteAllText((Join-Path $out 'reload.log'), $changes)
    if ($changes -notmatch 'renderer settings:.*fps: Some\(24\)') { throw 'No live config FPS update in log' }
    if ($changes -notmatch 'Rainy Hong Kong \(non-commercial\) loaded') { throw 'No recursive theme reload in log' }
    [pscustomobject]@{ engine_pid_before=$before.Id; engine_pid_after=$after.Id; config_fps=24; recursive_theme_change=$probe; utc=[DateTime]::UtcNow.ToString('o') } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $out 'reload.json') -Encoding utf8
    Write-Output "Config and recursive theme reload passed in the same engine PID $($before.Id)."
    Write-Output $changes
} finally {
    if ($created) { Remove-Item -LiteralPath $probe }
    if ($written) {
        $current = [IO.File]::ReadAllText($path)
        if ($current -eq $written) {
            [IO.File]::WriteAllBytes($temporary, $originalBytes)
            Move-Item -LiteralPath $temporary -Destination $path -Force
        }
        else {
            $config = $current | ConvertFrom-Json
            $config.engine.fps = $fps
            Write-Config ($config | ConvertTo-Json -Depth 100)
        }
    }
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    [IO.Directory]::Delete($lock)
}
