# Runs only after deployment, under the shared desktop lock. Every live file is restored
# byte for byte in finally; the theme under test is a disposable copy outside the checkout.
param([string]$Repo = 'D:\Coding\projects\desktop\wallpaper')
$ErrorActionPreference = 'Stop'
$lock = (Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
if (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) { throw 'Desktop lock busy; retry later' }
$configPath = Join-Path $env:APPDATA 'glaze\config.json'
$barsPath = Join-Path $env:APPDATA 'glaze\bars.json'
$palettePath = Join-Path $env:LOCALAPPDATA 'glaze\palette.json'
$activityPath = Join-Path $env:LOCALAPPDATA 'glaze\game-activity.json'
$logPath = Join-Path $env:LOCALAPPDATA 'glaze\glaze.log'
$backup = @{}
$result = [ordered]@{ at = (Get-Date).ToString('o') }
$probeTheme = Join-Path $env:TEMP ('glaze-fswatch-theme-' + [Guid]::NewGuid().ToString('N'))
$backupDir = Join-Path $env:TEMP ('glaze-fswatch-backup-' + [Guid]::NewGuid().ToString('N'))
$menuProbe = Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\fswatch-probe-' + [Guid]::NewGuid().ToString('N') + '.tmp')
$utf8 = New-Object Text.UTF8Encoding $false
Add-Type @'
using System.Runtime.InteropServices;
public static class WatchLiveFile {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool MoveFileEx(string from, string to, uint flags);
}
'@
function Read-Bytes($Path) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    try {
        $memory = New-Object IO.MemoryStream
        $stream.CopyTo($memory)
        return ,$memory.ToArray()
    } finally { $stream.Dispose(); if ($memory) { $memory.Dispose() } }
}
function Write-Bytes($Path, [byte[]]$Bytes) {
    $tempPath = "$Path.fswatch-probe"
    [IO.File]::WriteAllBytes($tempPath, $Bytes)
    $until = (Get-Date).AddSeconds(4)
    do {
        if ([WatchLiveFile]::MoveFileEx($tempPath, $Path, 9)) { return }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $until)
    throw "Atomic replace failed for $Path (Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
}
function Read-Text($Path) { return $utf8.GetString((Read-Bytes $Path)) }
function Write-Json($Path, $Value) {
    Write-Bytes $Path ($utf8.GetBytes(($Value | ConvertTo-Json -Depth 50)))
}
function Wait-For($Label, [scriptblock]$Check) {
    $until = (Get-Date).AddSeconds(12)
    while ((Get-Date) -lt $until) {
        if (& $Check) { $result[$Label] = $true; return }
        Start-Sleep -Milliseconds 100
    }
    throw "Live check timed out: $Label"
}
function Log-Tail {
    $text = Read-Text $logPath
    return $text.Substring([Math]::Min($script:logStart, $text.Length))
}
function Accent-Is([uint32]$Color) {
    return [uint32]([long](Get-ItemPropertyValue 'HKCU:\Software\Microsoft\Windows\DWM' AccentColor) -band 0xffffffffL) -eq $Color
}
try {
    New-Item -ItemType Directory -Path $backupDir | Out-Null
    foreach ($path in @($configPath, $barsPath, $palettePath, $activityPath)) {
        $backup[$path] = Read-Bytes $path
        [IO.File]::WriteAllBytes((Join-Path $backupDir ([IO.Path]::GetFileName($path))), $backup[$path])
    }
    $logStart = (Read-Text $logPath).Length
    $beforeIds = @(Get-Process glaze-core,glaze-shell,glaze-index | Sort-Object ProcessName | ForEach-Object { "$($_.ProcessName):$($_.Id)" })
    $config = [Text.Encoding]::UTF8.GetString($backup[$configPath]) | ConvertFrom-Json
    $hadIndexConfig = $null -ne $config.PSObject.Properties['index']
    $originalIndexConfig = $config.index
    $theme = [Text.Encoding]::UTF8.GetString($backup[$palettePath]) | ConvertFrom-Json
    Copy-Item -LiteralPath $theme.theme -Destination $probeTheme -Recurse
    $config.theme = $probeTheme
    $config.look.windows.accent = '#1267ab'
    Write-Json $configPath $config
    Wait-For 'config_hot_reload_core' { Accent-Is 0xffab6712L }
    Wait-For 'config_theme_switch' { (Get-Content -LiteralPath $palettePath -Raw | ConvertFrom-Json).theme -eq $probeTheme }

    # Only the theme manifest changes here. Its new bar appearance should be visible live.
    $manifestPath = Join-Path $probeTheme 'theme.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $manifest | Add-Member -Force NoteProperty bars ([pscustomobject]@{ appearance = [pscustomobject]@{ surface='solid'; tint='#8d2465'; opacity=1; text='light'; scale=1.25 } })
    Write-Json $manifestPath $manifest
    Start-Sleep -Seconds 2
    & "$Repo\tools\perf\capture.ps1" -Out (Join-Path $PSScriptRoot 'live-theme.png')
    $result['theme_edit_captured'] = $true

    # An invalid bars config is rejected by the real reload path without destroying surfaces.
    $bars = [Text.Encoding]::UTF8.GetString($backup[$barsPath]) | ConvertFrom-Json
    $bars | Add-Member -Force NoteProperty fswatch_probe $true
    Write-Json $barsPath $bars
    Wait-For 'bars_hot_reload_rejected_unknown_field' { (Log-Tail) -match 'bars.json:.*fswatch_probe' }
    Write-Bytes $barsPath $backup[$barsPath]

    # Rejecting a new invalid glob proves index config reload without triggering full-drive
    # reconciliation or replacing the live rule set during the other agent's work.
    $config | Add-Member -Force NoteProperty index ([pscustomobject]@{ exclude=@('[fswatch-probe'); include=@() })
    Write-Json $configPath $config
    Wait-For 'index_config_hot_reload_rejected_invalid_glob' { (Log-Tail) -match 'invalid index rules:.*fswatch-probe' }
    if ($hadIndexConfig) { $config.index = $originalIndexConfig }
    else { $config.PSObject.Properties.Remove('index') }
    $config.look.windows.accent = 'auto'
    Write-Json $configPath $config
    Start-Sleep -Milliseconds 500
    $palette = Get-Content -LiteralPath $palettePath -Raw | ConvertFrom-Json
    $palette.accent = '#b42dc8'
    Write-Json $palettePath $palette
    Wait-For 'palette_hot_reload_core' { Accent-Is 0xffc82db4L }

    $activity = [Text.Encoding]::UTF8.GetString($backup[$activityPath]) | ConvertFrom-Json
    $activity.activity.pause_background = $true
    $activity.activity.reason = 'presentation'
    Write-Json $activityPath $activity
    Wait-For 'game_activity_pause_shell' { (Log-Tail) -match 'game background pause: true' }
    Write-Bytes $activityPath $backup[$activityPath]
    Wait-For 'game_activity_resume_shell' { (Log-Tail) -match 'game background pause: false' }

    $launcherLogStart = (Read-Text $logPath).Length
    [IO.File]::WriteAllText($menuProbe, 'temporary directory notification probe', $utf8)
    Wait-For 'start_menu_hot_reload_launcher' {
        $text = Read-Text $logPath
        $text.Substring([Math]::Min($launcherLogStart, $text.Length)) -match 'launcher: \d+ apps indexed'
    }
    Remove-Item -LiteralPath $menuProbe
    $afterIds = @(Get-Process glaze-core,glaze-shell,glaze-index | Sort-Object ProcessName | ForEach-Object { "$($_.ProcessName):$($_.Id)" })
    $result['processes_survived_without_restart'] = ($beforeIds -join ',') -eq ($afterIds -join ',')
    $result['processes'] = $afterIds
} catch {
    $result['error'] = $_.ToString()
    throw
} finally {
    try {
    if (Test-Path -LiteralPath $menuProbe) { Remove-Item -LiteralPath $menuProbe }
    # Restore config first, let the real engine return to its theme, then restore its palette.
    foreach ($path in @($configPath, $barsPath, $activityPath)) {
        if ($backup.ContainsKey($path)) { Write-Bytes $path $backup[$path] }
    }
    Start-Sleep -Seconds 4
    if ($backup.ContainsKey($palettePath)) { Write-Bytes $palettePath $backup[$palettePath] }
    Start-Sleep -Seconds 1
    $restored = [ordered]@{}
    foreach ($path in $backup.Keys) {
        $current = Read-Bytes $path
        $restored[$path] = [Convert]::ToBase64String($current) -eq [Convert]::ToBase64String($backup[$path])
    }
    $result['restored_byte_for_byte'] = $restored
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'live-results.json') -Encoding utf8
    Log-Tail | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'live-log.txt') -Encoding utf8
    & "$Repo\tools\perf\capture.ps1" -Out (Join-Path $PSScriptRoot 'live-restored.png')
    # These are the literal resolved directories created by this script, never user paths.
    if ((Test-Path -LiteralPath $probeTheme) -and [IO.Path]::GetFullPath($probeTheme).StartsWith([IO.Path]::GetFullPath($env:TEMP) + '\')) {
        Remove-Item -LiteralPath $probeTheme -Recurse -Force
    }
    } finally { [IO.Directory]::Delete($lock, $false) }
}
