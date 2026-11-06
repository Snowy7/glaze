param(
    [string]$Exe = 'C:\mypaper-targets\studio-panel\target\debug\mypaper-studio.exe',
    [string]$Shots = 'C:\mypaper-targets\studio-panel\shots'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class StudioPanelCheck {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
}
'@
[void][StudioPanelCheck]::SetProcessDpiAwarenessContext([IntPtr](-4))

function SettingsHashes {
    $result = @{}
    foreach ($name in 'config.json','bars.json','widgets.json','studio.json','integrations.json','studio-panel-history.json') {
        $path = Join-Path (Join-Path $env:APPDATA 'mypaper') $name
        $result[$name] = if (Test-Path -LiteralPath $path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { $null }
    }
    return $result
}
function WithDesktopLock([scriptblock]$Action) {
    $lockPath = Join-Path $env:TEMP 'mypaper-tools\desktop.lock'
    [System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($lockPath)) | Out-Null
    $lock = $null
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        try { $lock = [System.IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None'); break }
        catch [System.IO.IOException] { Start-Sleep -Milliseconds 200 }
    }
    if (!$lock) { throw 'The desktop lock is busy; no input or capture was attempted.' }
    try { & $Action } finally { $lock.Dispose() }
}
function OwnElement([string]$Prefix) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($script:window)
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $matches = @($all | Where-Object { $_.Current.Name.StartsWith($Prefix) -and $_.Current.IsEnabled })
    if ($matches.Count -ne 1) {
        $names = @($all | ForEach-Object { $_.Current.Name })
        throw "Expected one '$Prefix' control; found $($matches.Count). Available: $($names -join ' | ')"
    }
    return $matches[0]
}
function InvokeOwn([string]$Prefix) {
    WithDesktopLock {
        $element = OwnElement $Prefix
        if ($element.Current.ProcessId -ne $script:process.Id) { throw 'UI Automation target is not our process.' }
        $rect = $element.Current.BoundingRectangle
        $point = New-Object StudioPanelCheck+Point
        $point.X = [int]($rect.Left + $rect.Width / 2)
        $point.Y = [int]($rect.Top + $rect.Height / 2)
        $owner = [uint32]0
        [void][StudioPanelCheck]::GetWindowThreadProcessId([StudioPanelCheck]::WindowFromPoint($point), [ref]$owner)
        if ($owner -ne $script:process.Id) { throw 'WindowFromPoint is not our panel; no control was invoked.' }
        $pattern = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
    }
    Start-Sleep -Milliseconds 350
}
function CaptureOwn([string]$Name) {
    WithDesktopLock {
        $script:process.Refresh()
        if ($script:process.HasExited) { throw 'Our process closed before capture.' }
        $owner = [uint32]0
        [void][StudioPanelCheck]::GetWindowThreadProcessId($script:window, [ref]$owner)
        if ($owner -ne $script:process.Id) { throw 'Window handle is no longer ours.' }
        $client = New-Object StudioPanelCheck+Rect
        [void][StudioPanelCheck]::GetClientRect($script:window, [ref]$client)
        $bitmap = New-Object System.Drawing.Bitmap($client.Right, $client.Bottom)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $dc = $graphics.GetHdc()
        try {
            if (![StudioPanelCheck]::PrintWindow($script:window, $dc, 3)) { throw 'PrintWindow failed.' }
        } finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
        try { $bitmap.Save((Join-Path $Shots $Name), [System.Drawing.Imaging.ImageFormat]::Png) }
        finally { $bitmap.Dispose() }
        return @{ name = $Name; width = $client.Right; height = $client.Bottom }
    }
}

$before = SettingsHashes
$existing = @(Get-Process -Name 'mypaper-studio' -ErrorAction SilentlyContinue | Select-Object Id,Path,MainWindowTitle)
if ($existing | Where-Object { $_.MainWindowTitle -like '*Panel*' }) { throw 'Another panel is already running; it has not been touched.' }
$previousSwitch = $env:MYPAPER_STUDIO_PANEL
$env:MYPAPER_STUDIO_PANEL = '1'
$script:process = $null
$measurements = @()
try {
    $script:process = Start-Process -FilePath $Exe -PassThru
    # A debug Rust executable has a console subsystem; WaitForInputIdle is not supported.
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        $script:process.Refresh()
        if ($script:process.HasExited) { throw 'Our panel exited at startup.' }
        if ($script:process.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    $script:window = $script:process.MainWindowHandle
    if ($script:window -eq [IntPtr]::Zero) { throw 'Our process has no panel window.' }
    Start-Sleep -Milliseconds 700
    $measurements += CaptureOwn 'live-root.png'
    InvokeOwn 'Settings '
    InvokeOwn 'Keyboard '
    $measurements += CaptureOwn 'live-keyboard.png'
    InvokeOwn 'Back to Settings'
    InvokeOwn 'General '
    $measurements += CaptureOwn 'live-general.png'
    InvokeOwn 'Done'
    if (!$script:process.WaitForExit(5000)) { throw 'Done did not close our panel.' }
} finally {
    # Never enumerate or close other Studio windows. Clean up only the exact process we created.
    if ($script:process -and !$script:process.HasExited) {
        [void]$script:process.CloseMainWindow()
        if (!$script:process.WaitForExit(3000)) { Stop-Process -Id $script:process.Id }
    }
    $env:MYPAPER_STUDIO_PANEL = $previousSwitch
}
$after = SettingsHashes
$changed = @($before.Keys | Where-Object { $before[$_] -ne $after[$_] })
$report = @{ process = $script:process.Id; closed = $script:process.HasExited; existing = $existing; captures = $measurements; changedSettings = $changed }
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Shots 'live-panel-check.json') -Encoding utf8
$report | ConvertTo-Json -Depth 5
if ($changed.Count) { throw 'Settings changed during a navigation-only check.' }
