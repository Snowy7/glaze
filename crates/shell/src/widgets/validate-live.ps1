param([switch]$Interactions,[ValidateRange(0,3)][int]$Samples=3,[switch]$Debug,[switch]$Profile)
$ErrorActionPreference='Stop'
if ($Debug -and $Samples -ne 0) {throw 'Performance samples require the release build'}
if ($Profile -and ($Samples -ne 0 -or $Interactions)) {throw 'Use -Profile -Samples 0 separately from gates/interactions'}
$widgetLiveRoot=(Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$widgetLiveLock='C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$widgetLiveEvidence=Join-Path $PSScriptRoot 'evidence'
if ($Interactions) {$widgetLiveEvidence=Join-Path $widgetLiveEvidence 'interactions'}
if ($Profile) {$widgetLiveEvidence=Join-Path $widgetLiveEvidence 'profile'}
$widgetLiveData=Join-Path $env:TEMP ('mypaper-widget-parity-'+[Guid]::NewGuid().ToString('N'))
$widgetLiveOldApp=$env:APPDATA
$widgetLiveOldLocal=$env:LOCALAPPDATA
$widgetLiveFixture=$null
$widgetLiveEngine=$null
$widgetLiveShell=$null
$widgetLiveMinimized=$false
$widgetLiveCore=$null
$widgetLiveLockOwned=$false
function Enter-WidgetLiveLock {
    while (-not (New-Item -ItemType Directory -Path $widgetLiveLock -ErrorAction SilentlyContinue)) {
        $existing=Get-Item -LiteralPath $widgetLiveLock -ErrorAction SilentlyContinue
        if ($existing -and ((Get-Date)-$existing.CreationTime).TotalMinutes -gt 5) {
            Remove-Item -LiteralPath $widgetLiveLock -ErrorAction SilentlyContinue
        }
        Start-Sleep -Seconds 2
    }
}
try {
    Write-Output 'Waiting for desktop lock for worktree engine + bar/dock + six-widget fixture.'
    Enter-WidgetLiveLock
    $widgetLiveLockOwned=$true
    New-Item -ItemType Directory -Path $widgetLiveEvidence,(Join-Path $widgetLiveData 'appdata\mypaper'),(Join-Path $widgetLiveData 'local'),(Join-Path $widgetLiveRoot 'docs\media') -Force | Out-Null
    $processes=Get-CimInstance Win32_Process | Where-Object { $_.Name -in 'mypaper-core.exe','mypaper-shell.exe','mypaper-wallpaper.exe' }
    $widgetLiveCore=$processes | Where-Object Name -eq 'mypaper-core.exe' | Select-Object -First 1
    $processes | Select-Object Name,ProcessId,ExecutablePath,CommandLine | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $widgetLiveEvidence 'original-processes.json')
    # Stop only the processes observed under this lock. The original core is restored in finally.
    foreach ($process in ($processes | Sort-Object { if ($_.Name -eq 'mypaper-core.exe') {0} else {1} })) {
        Stop-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 2
    $env:APPDATA=Join-Path $widgetLiveData 'appdata'
    $env:LOCALAPPDATA=Join-Path $widgetLiveData 'local'
    $config=@{theme=(Join-Path $widgetLiveRoot 'themes\rain-city');paused=$false;pause_on_battery=$false;pause_on_fullscreen=$false;static_snapshot=$false}
    [IO.File]::WriteAllText((Join-Path $env:APPDATA 'mypaper\config.json'),($config | ConvertTo-Json))
    # Floating Liquid bars, both displays; a paused media service and minute clocks.
    [IO.File]::WriteAllText((Join-Path $env:APPDATA 'mypaper\bars.json'),'{"preset":"macos","taskbar":{"mode":"hide"},"look":{"style":"liquid"},"top":{"monitors":"all","clock":{"seconds":false}},"dock":{"enabled":true,"monitors":"all","autohide":false,"pinned":[]},"performance":"off"}')
    $engineExe=Join-Path $widgetLiveRoot 'target\release\mypaper-wallpaper.exe'
    $widgetLiveEngine=Start-Process -FilePath $engineExe -WorkingDirectory $widgetLiveRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $widgetLiveEvidence 'engine-stdout.log') -RedirectStandardError (Join-Path $widgetLiveEvidence 'engine-stderr.log')
    $env:MYPAPER_N4_FIXTURE='1'
    $env:MYPAPER_N4_DESKTOP_VISIBLE='1'
    $env:MYPAPER_N4_BARS='1'
    $env:MYPAPER_WIDGETS_IGNORE_COVERED='1'
    if ($Interactions) {$env:MYPAPER_N4_INTERACTIONS='1'}
    if ($Profile) {$env:MYPAPER_N4_PROFILE='1'}
    $fixtureProfile=if ($Debug) {'debug'} else {'release'}
    $widgetLiveFixture=Start-Process -FilePath (Join-Path $widgetLiveRoot "target\$fixtureProfile\examples\mypaper-widget-fixture.exe") -ArgumentList '300' -WorkingDirectory $widgetLiveRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $widgetLiveEvidence 'fixture.log') -RedirectStandardError (Join-Path $widgetLiveEvidence 'fixture-stderr.log')
    Remove-Item Env:MYPAPER_N4_FIXTURE,Env:MYPAPER_N4_DESKTOP_VISIBLE,Env:MYPAPER_N4_BARS,Env:MYPAPER_WIDGETS_IGNORE_COVERED,Env:MYPAPER_N4_INTERACTIONS,Env:MYPAPER_N4_PROFILE -ErrorAction SilentlyContinue
    $readyDeadline=(Get-Date).AddSeconds(65)
    $engineRestarts=0
    do {
        if ($Interactions -and $widgetLiveEngine.HasExited -and $engineRestarts -eq 0) {
            # The engine requests a supervised restart when the interaction fixture restarts Explorer.
            $widgetLiveEngine=Start-Process -FilePath $engineExe -WorkingDirectory $widgetLiveRoot -WindowStyle Hidden -PassThru
            $engineRestarts++
        }
        if ($widgetLiveFixture.HasExited -or $widgetLiveEngine.HasExited) { throw 'Worktree fixture or engine exited. See evidence logs.' }
        Start-Sleep -Milliseconds 500
        $ready=(Get-Content -LiteralPath (Join-Path $widgetLiveEvidence 'fixture.log') -ErrorAction SilentlyContinue) -match 'N4_READY'
    } until ($ready -or (Get-Date) -gt $readyDeadline)
    if (-not $ready) {throw 'Fixture did not become ready'}
    # Idle measurements follow a full minute of service/font/cache warm-up. Cold spawn-to-bar
    # remains measured separately below; every configured widget stays visible throughout.
    if (-not $Interactions -and -not $Profile) {Start-Sleep -Seconds 60}
    Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -in $widgetLiveFixture.Id,$widgetLiveEngine.Id } | Select-Object Name,ProcessId,ExecutablePath,CommandLine | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $widgetLiveEvidence 'worktree-processes.json')
    if ($Profile) {
        $profileDeadline=(Get-Date).AddSeconds(240)
        do {
            Start-Sleep -Milliseconds 500
            $profileDone=(Get-Content -LiteralPath (Join-Path $widgetLiveEvidence 'fixture.log')) -match 'N4_PROFILE_DONE'
            if ($widgetLiveFixture.HasExited -and -not $profileDone) {throw 'Profile fixture failed'}
        } until ($profileDone -or (Get-Date) -gt $profileDeadline)
        if (-not $profileDone) {throw 'Profile exceeded its desktop window'}
    }
    if (-not $Interactions -and -not $Profile) {
        Add-Type -AssemblyName System.Windows.Forms
        Add-Type -AssemblyName System.Drawing
        $bounds=[System.Windows.Forms.SystemInformation]::VirtualScreen
        $bitmap=New-Object System.Drawing.Bitmap($bounds.Width,$bounds.Height)
        $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($bounds.X,$bounds.Y,0,0,$bounds.Size)
            $bitmap.Save((Join-Path $widgetLiveRoot 'docs\media\widgets-native-glass.png'),[System.Drawing.Imaging.ImageFormat]::Png)
        } finally {$graphics.Dispose();$bitmap.Dispose()}
        Write-Output 'Screenshot saved; starting official 60-second measurements.'
        $snapshot=[System.Threading.EventWaitHandle]::OpenExisting("Local\mypaper-n4-fixture-snapshot-$($widgetLiveFixture.Id)")
        try {$snapshot.Set() | Out-Null} finally {$snapshot.Dispose()}
        $snapshotDeadline=(Get-Date).AddSeconds(5)
        do {
            Start-Sleep -Milliseconds 25
            $checkpoint=(Get-Content -LiteralPath (Join-Path $widgetLiveEvidence 'fixture.log')) -match 'N4_CHECKPOINT'
        } until ($checkpoint -or (Get-Date) -gt $snapshotDeadline)
        if (-not $checkpoint) {throw 'Fixture did not acknowledge the measurement checkpoint'}
    }
    Remove-Item -LiteralPath $widgetLiveLock
    $widgetLiveLockOwned=$false
    foreach ($sample in $(if ($Samples -gt 0) {1..$Samples})) {
        & 'C:\Users\you\AppData\Local\Temp\mypaper-tools\measure.ps1' -Label "widget-parity-$sample" -Seconds 60
        if ($widgetLiveFixture.HasExited) {throw 'Fixture exited during a performance sample'}
        Copy-Item -LiteralPath "C:\Users\you\AppData\Local\Temp\mypaper-tools\measure-widget-parity-$sample.json" -Destination $widgetLiveEvidence
    }
    Enter-WidgetLiveLock
    $widgetLiveLockOwned=$true
    if (-not $Profile -and -not $widgetLiveFixture.HasExited) {
        $stop=[System.Threading.EventWaitHandle]::OpenExisting("Local\mypaper-n4-fixture-stop-$($widgetLiveFixture.Id)")
        try {$stop.Set() | Out-Null} finally {$stop.Dispose()}
    }
    $widgetLiveFixture.WaitForExit(20000) | Out-Null
    Get-Content -LiteralPath (Join-Path $widgetLiveEvidence 'fixture.log')
    if (-not $widgetLiveFixture.HasExited) {throw 'Fixture did not stop before startup measurements'}
    # Actual production shell, with the same six configured instances. Timestamp the spawn
    # call itself and compare the native first-present and widget-initialization markers.
    Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class WidgetStartup {
 [DllImport("kernel32.dll")] static extern void GetSystemTimePreciseAsFileTime(out long time);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent,IntPtr after,string name,string title);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window,uint message,IntPtr w,IntPtr l);
 public static Process Spawn(string exe,out long at) {
  var info=new ProcessStartInfo(exe);info.UseShellExecute=false;info.CreateNoWindow=true;
  long ticks;GetSystemTimePreciseAsFileTime(out ticks);at=(ticks-116444736000000000L)/10;
  return Process.Start(info);
 }
 public static void Close(int pid) {
  IntPtr window=IntPtr.Zero;
  while((window=FindWindowEx(new IntPtr(-3),window,"mypaper.shell.inbox",null))!=IntPtr.Zero) {
   uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner==pid){PostMessage(window,0x10,IntPtr.Zero,IntPtr.Zero);return;}
  }
 }
}
'@
    $desktop=New-Object -ComObject Shell.Application
    try {$desktop.MinimizeAll();$widgetLiveMinimized=$true} finally {[void][Runtime.InteropServices.Marshal]::ReleaseComObject($desktop)}
    $startupRows=@()
    $log=Join-Path $env:LOCALAPPDATA 'mypaper\mypaper.log'
    foreach ($run in $(if (-not $Interactions -and -not $Profile) {1..3})) {
        $lineOffset=@(Get-Content -LiteralPath $log -ErrorAction SilentlyContinue).Count
        $startedAt=[long]0
        $widgetLiveShell=[WidgetStartup]::Spawn((Join-Path $widgetLiveRoot 'target\release\mypaper-shell.exe'),[ref]$startedAt)
        $firstPdh=$null
        $firstModuleObservation=$false
        $until=(Get-Date).AddSeconds(20)
        do {
            Start-Sleep -Milliseconds 20
            $lines=@(Get-Content -LiteralPath $log -ErrorAction SilentlyContinue | Select-Object -Skip $lineOffset)
            if (-not $firstModuleObservation -and ($lines -match 'component=shell first_paint_ms=')) {
                $firstPdh=@($widgetLiveShell.Modules | Where-Object ModuleName -eq 'pdh.dll').Count -gt 0
                $firstModuleObservation=$true
            }
            $widgetMarker=$lines | Where-Object {$_ -match 'component=shell widgets_ready_ms='} | Select-Object -First 1
        } until ($widgetMarker -or $widgetLiveShell.HasExited -or (Get-Date)-gt $until)
        $first=$lines | Where-Object {$_ -match 'component=shell first_paint_ms='} | Select-Object -First 1
        $init=$lines | Where-Object {$_ -match 'component=shell widgets_init_ms='} | Select-Object -First 1
        if (-not $first -or -not $init -or -not $widgetMarker) {throw 'Missing production shell startup markers'}
        $first -match 'first_paint_ms=([0-9.]+) epoch_us=(\d+)' | Out-Null
        $age=[double]$Matches[1];$paintEpoch=[long]$Matches[2]
        $init -match 'widgets_init_ms=([0-9.]+) epoch_us=(\d+)' | Out-Null
        if ([long]$Matches[2] -le $paintEpoch) {throw 'Widgets started before the first bar present'}
        $widgetLiveShell.Refresh()
        $row=[pscustomobject]@{run=$run;configured_widgets=6;spawn_to_first_bar_ms=($paintEpoch-$startedAt)/1000.0;process_age_first_bar_ms=$age;widgets_init_ms=[double]$Matches[1];pdh_loaded_at_first_bar=$firstPdh;private_mb_after_widgets=[Math]::Round($widgetLiveShell.PrivateMemorySize64/1MB,2)}
        $startupRows+=$row
        $row | Format-List | Out-String | Write-Output
        $lines | Set-Content -Encoding UTF8 (Join-Path $widgetLiveEvidence "startup-$run.log")
        [WidgetStartup]::Close($widgetLiveShell.Id)
        $widgetLiveShell.WaitForExit(10000) | Out-Null
        if (-not $widgetLiveShell.HasExited) {throw 'Production shell failed to stop'}
    }
    if (-not $Interactions -and -not $Profile) {$startupRows | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $widgetLiveEvidence 'startup.json')}
} finally {
    if (-not $widgetLiveLockOwned) {Enter-WidgetLiveLock;$widgetLiveLockOwned=$true}
    if ($widgetLiveFixture -and -not $widgetLiveFixture.HasExited) {
        Stop-Process -Id $widgetLiveFixture.Id -ErrorAction SilentlyContinue
        $restoreShell=New-Object -ComObject Shell.Application
        try {$restoreShell.UndoMinimizeALL()} finally {[void][Runtime.InteropServices.Marshal]::ReleaseComObject($restoreShell)}
    }
    if ($widgetLiveEngine -and -not $widgetLiveEngine.HasExited) {Stop-Process -Id $widgetLiveEngine.Id -ErrorAction SilentlyContinue}
    if ($widgetLiveShell -and -not $widgetLiveShell.HasExited) {Stop-Process -Id $widgetLiveShell.Id -ErrorAction SilentlyContinue}
    if ($widgetLiveMinimized) {
        $restoreShell=New-Object -ComObject Shell.Application
        try {$restoreShell.UndoMinimizeALL()} finally {[void][Runtime.InteropServices.Marshal]::ReleaseComObject($restoreShell)}
    }
    if (Test-Path -LiteralPath (Join-Path $widgetLiveData 'local\mypaper')) {
        Get-ChildItem -LiteralPath (Join-Path $widgetLiveData 'local\mypaper') -Filter '*.log' -Recurse | Copy-Item -Destination $widgetLiveEvidence
    }
    $env:APPDATA=$widgetLiveOldApp
    $env:LOCALAPPDATA=$widgetLiveOldLocal
    Remove-Item Env:MYPAPER_N4_FIXTURE,Env:MYPAPER_N4_DESKTOP_VISIBLE,Env:MYPAPER_N4_BARS,Env:MYPAPER_WIDGETS_IGNORE_COVERED,Env:MYPAPER_N4_INTERACTIONS,Env:MYPAPER_N4_PROFILE -ErrorAction SilentlyContinue
    if ($widgetLiveCore) {
        $restoreArgs=$widgetLiveCore.CommandLine -replace '^(?:"[^"]+"|\S+)\s*',''
        if ($restoreArgs) {Start-Process -FilePath $widgetLiveCore.ExecutablePath -ArgumentList $restoreArgs -WorkingDirectory (Split-Path -Parent $widgetLiveCore.ExecutablePath) -WindowStyle Hidden | Out-Null}
        else {Start-Process -FilePath $widgetLiveCore.ExecutablePath -WorkingDirectory (Split-Path -Parent $widgetLiveCore.ExecutablePath) -WindowStyle Hidden | Out-Null}
    }
    # Only this run's exact, resolved temporary directory may be removed recursively.
    $resolvedLiveData=[System.IO.Path]::GetFullPath($widgetLiveData)
    $resolvedLiveTemp=[System.IO.Path]::GetFullPath($env:TEMP).TrimEnd('\')+'\'
    if ($resolvedLiveData.StartsWith($resolvedLiveTemp,[StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedLiveData) -like 'mypaper-widget-parity-*') {
        Remove-Item -LiteralPath $resolvedLiveData -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($widgetLiveLockOwned) {Remove-Item -LiteralPath $widgetLiveLock -ErrorAction SilentlyContinue}
}

